//! WebAssembly コード生成
//!
//! 構文木を WASM バイナリに変換する。
//!
//! - トップレベルの `const xxx = (...) => {...}` は **別関数** として抽出
//! - それ以外のトップレベル文は `main` 関数にまとめられ、最後の const の値が戻り値になる
//! - Number は f64、Boolean は i32 で表現

use crate::parse::{BinaryOp, Expression, Statement, TypeAnnotation};
use std::collections::HashMap;

// =============================================================================
// WASM バイナリ仕様の定数
// =============================================================================

// セクション ID
const SECTION_TYPE: u8 = 1;
const SECTION_FUNCTION: u8 = 3;
const SECTION_MEMORY: u8 = 5;
const SECTION_EXPORT: u8 = 7;
const SECTION_CODE: u8 = 10;
const SECTION_DATA: u8 = 11;

// 型タグ
const TYPE_FUNC: u8 = 0x60; // 関数型のヘッダ
const TYPE_F64: u8 = 0x7c; // f64 (Number に対応)
const TYPE_I32: u8 = 0x7f; // i32 (Boolean に対応)

// 命令 (opcode)
const OP_END: u8 = 0x0b;
const OP_IF: u8 = 0x04;
const OP_ELSE: u8 = 0x05;
const OP_CALL: u8 = 0x10;
const OP_LOCAL_GET: u8 = 0x20;
const OP_LOCAL_SET: u8 = 0x21;
const OP_I32_CONST: u8 = 0x41;
const OP_F64_CONST: u8 = 0x44;
const OP_F64_ADD: u8 = 0xa0;
const OP_F64_MUL: u8 = 0xa2;

// Export の種別
const EXPORT_FUNC: u8 = 0x00;

// =============================================================================
// バイナリエンコーダ
// =============================================================================

/// Unsigned LEB128 形式でエンコードして bytes に追記する。
///
/// LEB128 は WASM のサイズ・インデックスなど、可変長整数の標準形式。
/// 7 ビットずつ取り出して、続きがあれば MSB を 1 にする。
pub fn encode_uleb128(value: u64, bytes: &mut Vec<u8>) {
    let mut v = value;
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if v == 0 {
            break;
        }
    }
}

/// Signed LEB128 形式でエンコードして bytes に追記する。
///
/// 負の数も扱える可変長整数。`i32.const` などで使われる。
pub fn encode_sleb128(value: i64, bytes: &mut Vec<u8>) {
    let mut v = value;
    let mut more = true;
    while more {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        // 符号拡張で「これ以上書いても全部 1 (= -1) or 全部 0」なら打ち切る
        let sign_bit_set = (byte & 0x40) != 0;
        if (v == 0 && !sign_bit_set) || (v == -1 && sign_bit_set) {
            more = false;
        }
        bytes.push(if more { byte | 0x80 } else { byte });
    }
}

/// f64 を IEEE 754 形式 (リトルエンディアン 8 バイト) で bytes に追記する。
pub fn encode_f64(value: f64, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// WASM のセクションを書き出す。
///
/// 形式: `[section_id, payload_size (ULEB128), payload...]`
fn write_section(section_id: u8, payload: &[u8], out: &mut Vec<u8>) {
    out.push(section_id);
    encode_uleb128(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

// =============================================================================
// 関数を組み立てる内部表現
// =============================================================================

/// ローカル変数の情報（インデックスと WASM 型）
struct LocalInfo {
    index: u32,
    #[allow(dead_code)] // 将来 main の戻り値型決定で使う
    wasm_type: u8,
}

/// 1 つの関数を組み立てる作業用バッファ。
///
/// パラメータ・ローカル変数の登録と、命令列の書き出しを担う。
struct FunctionBuilder {
    /// 名前 → ローカル情報 (パラメータも含む。WASM の規約で param は local の一部)
    locals: HashMap<String, LocalInfo>,
    /// 非パラメータのローカル変数の型 (宣言順)
    local_types: Vec<u8>,
    /// 命令列 (END なし)
    code: Vec<u8>,
    /// 関数名 → 関数 index (CALL 命令に必要)
    function_indices: HashMap<String, u32>,
    /// 文字列内容 → メモリオフセット (StringLit emit 用)
    string_offsets: HashMap<String, u32>,
}

impl FunctionBuilder {
    fn new(
        function_indices: HashMap<String, u32>,
        string_offsets: HashMap<String, u32>,
    ) -> Self {
        Self {
            locals: HashMap::new(),
            local_types: Vec::new(),
            code: Vec::new(),
            function_indices,
            string_offsets,
        }
    }

    /// パラメータを登録する (普通のローカル登録より先に呼ぶこと)
    fn declare_param(&mut self, name: String, wasm_type: u8) {
        let index = self.locals.len() as u32;
        self.locals.insert(name, LocalInfo { index, wasm_type });
    }

    /// 普通のローカル変数を登録し、そのインデックスを返す
    fn declare_local(&mut self, name: String, wasm_type: u8) -> u32 {
        let index = self.locals.len() as u32;
        self.locals.insert(name, LocalInfo { index, wasm_type });
        self.local_types.push(wasm_type);
        index
    }

    fn lookup(&self, name: &str) -> Option<&LocalInfo> {
        self.locals.get(name)
    }
}

/// 文字列リテラル → メモリ内オフセットの対応表。
///
/// プログラム中に登場する文字列を事前に収集し、それぞれにメモリ上のオフセットを割り当てる。
/// 同じ内容の文字列は同じオフセットを共有する (intern)。
#[derive(Default)]
struct StringTable {
    /// 文字列内容 → offset
    map: HashMap<String, u32>,
    /// (offset, bytes) の並び (Data Section 書き出し用、宣言順)
    entries: Vec<(u32, Vec<u8>)>,
    next_offset: u32,
}

impl StringTable {
    /// 文字列を登録し、そのオフセットを返す (同じ内容なら同じオフセット)
    fn intern(&mut self, s: &str) {
        if self.map.contains_key(s) {
            return;
        }
        let off = self.next_offset;
        let bytes = s.as_bytes().to_vec();
        self.next_offset += bytes.len() as u32;
        self.map.insert(s.to_string(), off);
        self.entries.push((off, bytes));
    }
}

/// 完成した関数の情報。モジュール書き出し時に Type / Function / Code セクションに使う。
struct FunctionInfo {
    #[allow(dead_code)] // デバッグ用。将来エラーメッセージなどで使うかも
    name: String,
    param_types: Vec<u8>,
    return_type: u8,
    /// 命令列 (END なし)
    body: Vec<u8>,
    /// 非パラメータのローカル変数の型
    local_types: Vec<u8>,
}

// =============================================================================
// 公開 API
// =============================================================================

/// 空の `main` 関数だけを export する最小の WASM モジュールを生成する。
///
/// 主に「マジックヘッダ + 必要な全セクション」の最小形を確認するためのリファレンス用。
pub fn build_empty_module() -> Vec<u8> {
    let mut out = Vec::new();

    // マジック \0asm + バージョン 1
    out.extend_from_slice(b"\0asm");
    out.extend_from_slice(&[1, 0, 0, 0]);

    // Type: 関数型 0 = () → ()
    let mut type_section = Vec::new();
    encode_uleb128(1, &mut type_section); // 関数型の数
    type_section.push(TYPE_FUNC);
    encode_uleb128(0, &mut type_section); // params: 0
    encode_uleb128(0, &mut type_section); // results: 0
    write_section(SECTION_TYPE, &type_section, &mut out);

    // Function: 関数 0 は type 0
    let mut func_section = Vec::new();
    encode_uleb128(1, &mut func_section);
    encode_uleb128(0, &mut func_section);
    write_section(SECTION_FUNCTION, &func_section, &mut out);

    // Export: "main" として関数 0 を公開
    let mut export_section = Vec::new();
    encode_uleb128(1, &mut export_section);
    let name = b"main";
    encode_uleb128(name.len() as u64, &mut export_section);
    export_section.extend_from_slice(name);
    export_section.push(EXPORT_FUNC);
    encode_uleb128(0, &mut export_section);
    write_section(SECTION_EXPORT, &export_section, &mut out);

    // Code: 関数 0 の本体 = [locals=0, END]
    let mut code_section = Vec::new();
    encode_uleb128(1, &mut code_section);
    let mut body = Vec::new();
    encode_uleb128(0, &mut body); // ローカル宣言グループ数: 0
    body.push(OP_END);
    encode_uleb128(body.len() as u64, &mut code_section);
    code_section.extend_from_slice(&body);
    write_section(SECTION_CODE, &code_section, &mut out);

    out
}

/// 構文木を WASM バイナリにコンパイルする。
///
/// アルゴリズム:
/// 1. トップレベルの `const xxx = (...) => {...}` を**別関数**として抽出
/// 2. 残りの top-level 文は **main** 関数にまとめる (最後の const の値が戻り値)
/// 3. 全関数を Type / Function / Code セクションに書き出す
pub fn compile_to_wasm(statements: &[Statement]) -> Vec<u8> {
    // --- 0. 文字列リテラルを事前収集してオフセットを割り当てる ---
    let mut string_table = StringTable::default();
    for stmt in statements {
        collect_strings_stmt(stmt, &mut string_table);
    }
    let string_offsets = string_table.map.clone();

    let mut user_functions: Vec<FunctionInfo> = Vec::new();
    let mut main_body_stmts: Vec<&Statement> = Vec::new();
    let mut function_indices: HashMap<String, u32> = HashMap::new();

    // --- 1. ユーザー定義関数を抽出 ---
    for stmt in statements {
        if let Statement::ConstDeclaration { name, init, .. } = stmt {
            if let Expression::ArrowFunction {
                params,
                body,
                return_type,
                ..
            } = init
            {
                let idx = user_functions.len() as u32;
                function_indices.insert(name.clone(), idx);

                let param_types: Vec<u8> = params
                    .iter()
                    .map(|p| {
                        p.type_annotation
                            .as_ref()
                            .map(wasm_type_from_annotation)
                            .unwrap_or(TYPE_F64)
                    })
                    .collect();
                let ret_type = return_type
                    .as_ref()
                    .map(wasm_type_from_annotation)
                    .unwrap_or(TYPE_F64);

                let mut fb =
                    FunctionBuilder::new(function_indices.clone(), string_offsets.clone());
                for (param, &ty) in params.iter().zip(param_types.iter()) {
                    fb.declare_param(param.name.clone(), ty);
                }
                for stmt in body {
                    emit_statement(stmt, &mut fb);
                }

                user_functions.push(FunctionInfo {
                    name: name.clone(),
                    param_types,
                    return_type: ret_type,
                    body: fb.code,
                    local_types: fb.local_types,
                });
                continue;
            }
        }
        main_body_stmts.push(stmt);
    }

    // --- 2. main 関数を組み立て ---
    let main_idx = user_functions.len() as u32;
    let mut main_fb = FunctionBuilder::new(function_indices.clone(), string_offsets.clone());
    let mut last_name: Option<String> = None;
    for stmt in &main_body_stmts {
        emit_statement(stmt, &mut main_fb);
        if let Statement::ConstDeclaration { name, .. } = stmt {
            last_name = Some(name.clone());
        }
    }

    // 最後の const の値を local.get でスタックに残す (これが main の戻り値)
    let main_return = if let Some(name) = &last_name {
        if let Some(info) = main_fb.lookup(name) {
            let idx = info.index;
            let ty = info.wasm_type;
            main_fb.code.push(OP_LOCAL_GET);
            encode_uleb128(idx as u64, &mut main_fb.code);
            ty
        } else {
            TYPE_F64
        }
    } else {
        TYPE_F64
    };

    user_functions.push(FunctionInfo {
        name: "main".to_string(),
        param_types: vec![],
        return_type: main_return,
        body: main_fb.code,
        local_types: main_fb.local_types,
    });

    // --- 3. バイナリ書き出し ---
    write_module(&user_functions, main_idx, &string_table)
}

// =============================================================================
// モジュールのバイト列書き出し
// =============================================================================

fn write_module(functions: &[FunctionInfo], main_idx: u32, strings: &StringTable) -> Vec<u8> {
    let mut out = Vec::new();

    // マジック + バージョン
    out.extend_from_slice(b"\0asm");
    out.extend_from_slice(&[1, 0, 0, 0]);

    // --- Type Section: 各関数のシグネチャ ---
    // 簡略化: 関数 i のシグネチャは type i に1対1対応 (シェアしない)
    let mut type_section = Vec::new();
    encode_uleb128(functions.len() as u64, &mut type_section);
    for f in functions {
        type_section.push(TYPE_FUNC);
        encode_uleb128(f.param_types.len() as u64, &mut type_section);
        for &ty in &f.param_types {
            type_section.push(ty);
        }
        encode_uleb128(1, &mut type_section); // results: 必ず 1 個
        type_section.push(f.return_type);
    }
    write_section(SECTION_TYPE, &type_section, &mut out);

    // --- Function Section: 関数 i が使う type index ---
    let mut func_section = Vec::new();
    encode_uleb128(functions.len() as u64, &mut func_section);
    for i in 0..functions.len() as u32 {
        encode_uleb128(i as u64, &mut func_section);
    }
    write_section(SECTION_FUNCTION, &func_section, &mut out);

    // --- Memory Section: 1 ページ (64KB) を確保 ---
    // 文字列を持つ可能性があるので常に宣言しておく
    let mut memory_section = Vec::new();
    encode_uleb128(1, &mut memory_section); // メモリ数: 1
    memory_section.push(0x00); // flags: 上限なし
    encode_uleb128(1, &mut memory_section); // 最小ページ数: 1
    write_section(SECTION_MEMORY, &memory_section, &mut out);

    // --- Export Section: main だけ export ---
    let mut export_section = Vec::new();
    encode_uleb128(1, &mut export_section);
    let name = b"main";
    encode_uleb128(name.len() as u64, &mut export_section);
    export_section.extend_from_slice(name);
    export_section.push(EXPORT_FUNC);
    encode_uleb128(main_idx as u64, &mut export_section);
    write_section(SECTION_EXPORT, &export_section, &mut out);

    // --- Code Section: 各関数の本体 ---
    let mut code_section = Vec::new();
    encode_uleb128(functions.len() as u64, &mut code_section);
    for f in functions {
        let mut body = Vec::new();
        // ローカル宣言: 「型グループ」の配列 (簡略化: 1個ずつ別グループにする)
        // 本来は同じ型を連続させて圧縮できるが、教材のため単純化
        encode_uleb128(f.local_types.len() as u64, &mut body);
        for &ty in &f.local_types {
            encode_uleb128(1, &mut body); // この型のローカルが 1 個
            body.push(ty);
        }
        body.extend_from_slice(&f.body);
        body.push(OP_END);

        encode_uleb128(body.len() as u64, &mut code_section);
        code_section.extend_from_slice(&body);
    }
    write_section(SECTION_CODE, &code_section, &mut out);

    // --- Data Section: 文字列リテラルをメモリに初期化 ---
    if !strings.entries.is_empty() {
        let mut data_section = Vec::new();
        encode_uleb128(strings.entries.len() as u64, &mut data_section);
        for (offset, bytes) in &strings.entries {
            data_section.push(0x00); // mode: active, メモリ index 0
            // オフセット式: i32.const <offset> + end
            data_section.push(OP_I32_CONST);
            encode_sleb128(*offset as i64, &mut data_section);
            data_section.push(OP_END);
            // バイト列本体
            encode_uleb128(bytes.len() as u64, &mut data_section);
            data_section.extend_from_slice(bytes);
        }
        write_section(SECTION_DATA, &data_section, &mut out);
    }

    out
}

// =============================================================================
// 文字列リテラルの事前収集
// =============================================================================

/// 文の AST を歩いて、含まれる文字列リテラルをすべて `table` に登録する。
fn collect_strings_stmt(stmt: &Statement, table: &mut StringTable) {
    match stmt {
        Statement::ConstDeclaration { init, .. } => collect_strings_expr(init, table),
        Statement::Return { argument: Some(e) } => collect_strings_expr(e, table),
        Statement::Return { argument: None } => {}
    }
}

/// 式の AST を歩いて、含まれる文字列リテラルをすべて `table` に登録する。
fn collect_strings_expr(expr: &Expression, table: &mut StringTable) {
    match expr {
        Expression::String(s) => {
            table.intern(s);
        }
        Expression::Binary { left, right, .. } => {
            collect_strings_expr(left, table);
            collect_strings_expr(right, table);
        }
        Expression::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_strings_expr(test, table);
            collect_strings_expr(consequent, table);
            collect_strings_expr(alternate, table);
        }
        Expression::Call { callee, arguments } => {
            collect_strings_expr(callee, table);
            for a in arguments {
                collect_strings_expr(a, table);
            }
        }
        Expression::Array(elements) => {
            for e in elements {
                collect_strings_expr(e, table);
            }
        }
        Expression::Member { object, index } => {
            collect_strings_expr(object, table);
            collect_strings_expr(index, table);
        }
        Expression::ArrowFunction { body, .. } => {
            for s in body {
                collect_strings_stmt(s, table);
            }
        }
        // リテラル・識別子は収集対象なし
        Expression::Number(_) | Expression::Boolean(_) | Expression::Identifier(_) => {}
    }
}

// =============================================================================
// 文・式 → WASM 命令への変換
// =============================================================================

fn emit_statement(stmt: &Statement, fb: &mut FunctionBuilder) {
    match stmt {
        Statement::ConstDeclaration {
            name,
            init,
            type_annotation,
        } => {
            // 右辺を評価 → スタックトップに値が乗る
            emit_expression(init, fb);

            // 値の型を決める (annotation 優先、なければ式の形から推測)
            let ty = type_annotation
                .as_ref()
                .map(wasm_type_from_annotation)
                .unwrap_or_else(|| infer_simple_type(init));

            // ローカルを確保して local.set で格納
            let idx = fb.declare_local(name.clone(), ty);
            fb.code.push(OP_LOCAL_SET);
            encode_uleb128(idx as u64, &mut fb.code);
        }
        Statement::Return {
            argument: Some(expr),
        } => {
            emit_expression(expr, fb);
        }
        Statement::Return { argument: None } => {
            panic!("return without value is not supported");
        }
    }
}

fn emit_expression(expr: &Expression, fb: &mut FunctionBuilder) {
    match expr {
        Expression::Number(n) => {
            // f64.const + 8 バイト
            fb.code.push(OP_F64_CONST);
            encode_f64(*n as f64, &mut fb.code);
        }
        Expression::Boolean(b) => {
            // i32.const + SLEB128 (0 or 1)
            fb.code.push(OP_I32_CONST);
            encode_sleb128(if *b { 1 } else { 0 }, &mut fb.code);
        }
        Expression::String(s) => {
            // 文字列は事前収集でメモリに置かれた offset (i32 ポインタ) で表現
            let offset = fb
                .string_offsets
                .get(s)
                .copied()
                .unwrap_or_else(|| panic!("string not in table: {s:?}"));
            fb.code.push(OP_I32_CONST);
            encode_sleb128(offset as i64, &mut fb.code);
        }
        Expression::Identifier(name) => {
            // ローカルから取り出してスタックに積む
            let idx = fb
                .lookup(name)
                .unwrap_or_else(|| panic!("undeclared variable: {name}"))
                .index;
            fb.code.push(OP_LOCAL_GET);
            encode_uleb128(idx as u64, &mut fb.code);
        }
        Expression::Binary { left, op, right } => {
            // スタックマシン式: left → right → op の順
            emit_expression(left, fb);
            emit_expression(right, fb);
            match op {
                BinaryOp::Add => fb.code.push(OP_F64_ADD),
                BinaryOp::Multiply => fb.code.push(OP_F64_MUL),
            }
        }
        Expression::Conditional {
            test,
            consequent,
            alternate,
        } => {
            // test (i32) → if (result f64) → then → else → end
            emit_expression(test, fb);
            fb.code.push(OP_IF);
            fb.code.push(TYPE_F64); // ブロックの結果型
            emit_expression(consequent, fb);
            fb.code.push(OP_ELSE);
            emit_expression(alternate, fb);
            fb.code.push(OP_END);
        }
        Expression::Call { callee, arguments } => {
            // 引数を順にスタックに積む
            for arg in arguments {
                emit_expression(arg, fb);
            }
            // CALL <function index>
            let Expression::Identifier(name) = callee.as_ref() else {
                panic!("only identifier callees are supported");
            };
            let idx = fb
                .function_indices
                .get(name)
                .copied()
                .unwrap_or_else(|| panic!("unknown function: {name}"));
            fb.code.push(OP_CALL);
            encode_uleb128(idx as u64, &mut fb.code);
        }
        _ => panic!("unsupported expression: {expr:?}"),
    }
}

// =============================================================================
// 型の判定
// =============================================================================

/// TypeAnnotation を WASM の型タグに変換する。
fn wasm_type_from_annotation(ta: &TypeAnnotation) -> u8 {
    match ta {
        TypeAnnotation::Named(name) => match name.as_str() {
            "number" | "Float" => TYPE_F64,
            "boolean" | "Bool" => TYPE_I32,
            _ => TYPE_F64, // 不明な型は f64 (暫定)
        },
        _ => TYPE_F64,
    }
}

/// 型注釈がないときの簡易型推測 (式の形だけ見る)。
///
/// 本格的には typecheck の結果を渡すべきだが、現状は wasm 単体で動かすため暫定実装。
fn infer_simple_type(expr: &Expression) -> u8 {
    match expr {
        Expression::Boolean(_) => TYPE_I32,
        Expression::String(_) => TYPE_I32, // 文字列はポインタなので i32
        _ => TYPE_F64,
    }
}

// =============================================================================
// テスト
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- エンコーダ ---

    #[test]
    fn uleb128_small() {
        let mut b = Vec::new();
        encode_uleb128(0, &mut b);
        assert_eq!(b, vec![0x00]);
    }

    #[test]
    fn uleb128_127() {
        let mut b = Vec::new();
        encode_uleb128(127, &mut b);
        assert_eq!(b, vec![0x7f]);
    }

    #[test]
    fn uleb128_128() {
        let mut b = Vec::new();
        encode_uleb128(128, &mut b);
        assert_eq!(b, vec![0x80, 0x01]);
    }

    #[test]
    fn uleb128_624485() {
        // Wikipedia の例: 624485 → E5 8E 26
        let mut b = Vec::new();
        encode_uleb128(624485, &mut b);
        assert_eq!(b, vec![0xe5, 0x8e, 0x26]);
    }

    #[test]
    fn sleb128_positive() {
        let mut b = Vec::new();
        encode_sleb128(0, &mut b);
        assert_eq!(b, vec![0x00]);
    }

    #[test]
    fn sleb128_negative_one() {
        let mut b = Vec::new();
        encode_sleb128(-1, &mut b);
        assert_eq!(b, vec![0x7f]); // 符号拡張で全ビット 1
    }

    #[test]
    fn sleb128_negative_large() {
        // Wikipedia の例: -12345 → C7 9F 7F
        let mut b = Vec::new();
        encode_sleb128(-12345, &mut b);
        assert_eq!(b, vec![0xc7, 0x9f, 0x7f]);
    }

    #[test]
    fn f64_one() {
        let mut b = Vec::new();
        encode_f64(1.0, &mut b);
        assert_eq!(b, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f]);
    }

    #[test]
    fn f64_pi() {
        let mut b = Vec::new();
        encode_f64(std::f64::consts::PI, &mut b);
        assert_eq!(b, std::f64::consts::PI.to_le_bytes().to_vec());
    }

    // --- 最小モジュール ---

    #[test]
    fn empty_module_parses() {
        let bytes = build_empty_module();
        let parser = wasmparser::Parser::new(0);

        let mut saw_main_export = false;
        let mut function_count = 0;
        let mut type_count = 0;

        for payload in parser.parse_all(&bytes) {
            match payload.expect("valid wasm") {
                wasmparser::Payload::TypeSection(reader) => type_count = reader.count(),
                wasmparser::Payload::FunctionSection(reader) => function_count = reader.count(),
                wasmparser::Payload::ExportSection(reader) => {
                    for export in reader {
                        let export = export.unwrap();
                        if export.name == "main"
                            && export.kind == wasmparser::ExternalKind::Func
                        {
                            saw_main_export = true;
                        }
                    }
                }
                _ => {}
            }
        }

        assert_eq!(type_count, 1);
        assert_eq!(function_count, 1);
        assert!(saw_main_export);
    }

    // --- ソースコードから WASM へ ---

    /// テスト用ヘルパー: 関数本体の命令列を全部取り出す
    fn collect_ops(bytes: &[u8]) -> Vec<wasmparser::Operator<'_>> {
        let parser = wasmparser::Parser::new(0);
        let mut ops = Vec::new();
        for payload in parser.parse_all(bytes) {
            if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    ops.push(reader.read().unwrap());
                }
            }
        }
        ops
    }

    #[test]
    fn compile_single_const_returns_42() {
        let stmts = crate::compile("const x = 42;");
        let bytes = compile_to_wasm(&stmts);

        let mut const_value = None;
        let mut export_name = None;

        let parser = wasmparser::Parser::new(0);
        for payload in parser.parse_all(&bytes) {
            match payload.expect("valid wasm") {
                wasmparser::Payload::ExportSection(reader) => {
                    for e in reader {
                        let e = e.unwrap();
                        if e.kind == wasmparser::ExternalKind::Func {
                            export_name = Some(e.name.to_string());
                        }
                    }
                }
                wasmparser::Payload::CodeSectionEntry(body) => {
                    let mut reader = body.get_operators_reader().unwrap();
                    while !reader.eof() {
                        if let wasmparser::Operator::F64Const { value } = reader.read().unwrap() {
                            const_value = Some(f64::from_bits(value.bits()));
                        }
                    }
                }
                _ => {}
            }
        }

        assert_eq!(export_name.as_deref(), Some("main"));
        assert_eq!(const_value, Some(42.0));
    }

    #[test]
    fn compile_addition() {
        let stmts = crate::compile("const x = 1 + 2;");
        let bytes = compile_to_wasm(&stmts);
        let ops = collect_ops(&bytes);
        assert!(matches!(
            ops.first(),
            Some(wasmparser::Operator::F64Const { .. })
        ));
        assert!(ops.iter().any(|op| matches!(op, wasmparser::Operator::F64Add)));
    }

    #[test]
    fn compile_mixed_operators() {
        let stmts = crate::compile("const x = 1 + 2 * 3;");
        let bytes = compile_to_wasm(&stmts);
        let ops = collect_ops(&bytes);
        assert!(ops.iter().any(|op| matches!(op, wasmparser::Operator::F64Add)));
        assert!(ops.iter().any(|op| matches!(op, wasmparser::Operator::F64Mul)));
    }

    #[test]
    fn compile_two_consts_uses_locals() {
        let stmts = crate::compile("const x = 5; const y = x + 1;");
        let bytes = compile_to_wasm(&stmts);
        let ops = collect_ops(&bytes);
        assert!(ops.iter().any(|op| matches!(op, wasmparser::Operator::LocalSet { .. })));
        assert!(ops.iter().any(|op| matches!(op, wasmparser::Operator::LocalGet { .. })));
    }

    #[test]
    fn compile_const_declares_locals() {
        let stmts = crate::compile("const x = 42;");
        let bytes = compile_to_wasm(&stmts);

        let parser = wasmparser::Parser::new(0);
        for payload in parser.parse_all(&bytes) {
            if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
                let locals: Vec<_> = body
                    .get_locals_reader()
                    .unwrap()
                    .into_iter()
                    .map(|r| r.unwrap())
                    .collect();
                assert!(!locals.is_empty(), "should declare locals");
            }
        }
    }

    #[test]
    fn compile_boolean_literal() {
        let stmts = crate::compile("const x = true;");
        let bytes = compile_to_wasm(&stmts);
        // モジュールが組み立てられること自体を確認
        let _ = bytes;
    }

    #[test]
    fn compile_ternary() {
        let stmts = crate::compile("const x = true ? 1 : 2;");
        let bytes = compile_to_wasm(&stmts);
        let ops = collect_ops(&bytes);
        assert!(ops.iter().any(|op| matches!(op, wasmparser::Operator::If { .. })));
        assert!(ops.iter().any(|op| matches!(op, wasmparser::Operator::Else)));
    }

    #[test]
    fn compile_string_literal() {
        let stmts = crate::compile(r#"const s = "hi";"#);
        let bytes = compile_to_wasm(&stmts);

        let parser = wasmparser::Parser::new(0);
        let mut has_memory = false;
        let mut data_segments = 0;

        for payload in parser.parse_all(&bytes) {
            match payload.unwrap() {
                wasmparser::Payload::MemorySection(reader) => {
                    has_memory = reader.count() > 0;
                }
                wasmparser::Payload::DataSection(reader) => {
                    data_segments = reader.count();
                }
                _ => {}
            }
        }

        assert!(has_memory, "should declare memory");
        assert_eq!(data_segments, 1, "should have 1 data segment");
    }

    #[test]
    fn compile_repeated_strings_share_offset() {
        // 同じ内容なら同じオフセットを使う (intern)
        let stmts = crate::compile(r#"const a = "hi"; const b = "hi";"#);
        let bytes = compile_to_wasm(&stmts);

        let parser = wasmparser::Parser::new(0);
        for payload in parser.parse_all(&bytes) {
            if let wasmparser::Payload::DataSection(reader) = payload.unwrap() {
                assert_eq!(reader.count(), 1, "should reuse the same string");
            }
        }
    }

    // --- end-to-end (monomorphize → wasm) ---

    #[test]
    fn end_to_end_polymorphic_single_type() {
        let bytes = crate::compile_to_wasm_full(
            "const id = (x) => { return x; };
             const a = id(5);
             const b = id(10);",
        );

        let parser = wasmparser::Parser::new(0);
        let mut func_count = 0;
        for payload in parser.parse_all(&bytes) {
            if let wasmparser::Payload::FunctionSection(reader) = payload.unwrap() {
                func_count = reader.count();
            }
        }
        // id_Number と main で 2 関数
        assert_eq!(func_count, 2);
    }

    #[test]
    fn end_to_end_multiple_instantiations() {
        let bytes = crate::compile_to_wasm_full(
            r#"const id = (x) => { return x; };
               const a = id(5);
               const b = id("hi");"#,
        );

        let parser = wasmparser::Parser::new(0);
        let mut func_count = 0;
        for payload in parser.parse_all(&bytes) {
            if let wasmparser::Payload::FunctionSection(reader) = payload.unwrap() {
                func_count = reader.count();
            }
        }
        // id_Number, id_String, main で 3 関数
        assert_eq!(func_count, 3);
    }

    #[test]
    fn compile_function_definition_and_call() {
        let stmts = crate::compile(
            "const add = (a: number, b: number) => { return a + b; };
             const r = add(1, 2);",
        );
        let bytes = compile_to_wasm(&stmts);

        let parser = wasmparser::Parser::new(0);
        let mut func_count = 0;
        let mut has_call = false;

        for payload in parser.parse_all(&bytes) {
            match payload.unwrap() {
                wasmparser::Payload::FunctionSection(reader) => {
                    func_count = reader.count();
                }
                wasmparser::Payload::CodeSectionEntry(body) => {
                    let mut r = body.get_operators_reader().unwrap();
                    while !r.eof() {
                        if matches!(r.read().unwrap(), wasmparser::Operator::Call { .. }) {
                            has_call = true;
                        }
                    }
                }
                _ => {}
            }
        }

        assert_eq!(func_count, 2, "add + main");
        assert!(has_call);
    }
}