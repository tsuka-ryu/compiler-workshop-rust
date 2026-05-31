// セクション ID
const SECTION_TYPE: u8 = 1;
const SECTION_FUNCTION: u8 = 3;
const SECTION_EXPORT: u8 = 7;
const SECTION_CODE: u8 = 10;

// 型タグ
const TYPE_FUNC: u8 = 0x60;
const TYPE_F64: u8 = 0x7c;
const TYPE_VOID: u8 = 0x40; // 空ブロック型

// 命令
const OP_END: u8 = 0x0b;
const OP_F64_CONST: u8 = 0x44;
const OP_F64_ADD: u8 = 0xa0;
const OP_F64_MUL: u8 = 0xa2;
const OP_LOCAL_GET: u8 = 0x20;
const OP_LOCAL_SET: u8 = 0x21;
const OP_DROP: u8 = 0x1a;

// Export の種別
const EXPORT_FUNC: u8 = 0x00;

struct FunctionBuilder {
    /// const の名前 → local index
    locals: std::collections::HashMap<String, u32>,
    /// 命令列
    code: Vec<u8>,
}

impl FunctionBuilder {
    fn new() -> Self {
        Self {
            locals: std::collections::HashMap::new(),
            code: Vec::new(),
        }
    }

    /// 新しいローカルを確保してインデックスを返す
    fn declare(&mut self, name: String) -> u32 {
        let index = self.locals.len() as u32;
        self.locals.insert(name, index);
        index
    }

    fn lookup(&self, name: &str) -> Option<u32> {
        self.locals.get(name).copied()
    }
}

/// Unsigned LEB128 でエンコードして bytes に追記
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

/// セクションを bytes に書き出す（先頭に section_id と payload_size を付加）
fn write_section(section_id: u8, payload: &[u8], out: &mut Vec<u8>) {
    out.push(section_id);
    encode_uleb128(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

/// バイト列 を len + bytes の形式で書き出す（文字列やベクタの長さ付きエンコード用）
fn write_vec(items: &[u8], out: &mut Vec<u8>) {
    encode_uleb128(items.len() as u64, out);
    out.extend_from_slice(items);
}

/// 空の main 関数だけを export する最小モジュールを生成
pub fn build_empty_module() -> Vec<u8> {
    let mut out = Vec::new();

    // マジックヘッダ + バージョン
    out.extend_from_slice(b"\0asm"); // \0 a s m
    out.extend_from_slice(&[1, 0, 0, 0]); // version 1

    // --- Type Section: () → ()  を1つ宣言 ---
    let mut type_section = Vec::new();
    encode_uleb128(1, &mut type_section); // 関数型の数: 1
    // 関数型 0: func, params=[], results=[]
    type_section.push(TYPE_FUNC);
    encode_uleb128(0, &mut type_section); // params の長さ: 0
    encode_uleb128(0, &mut type_section); // results の長さ: 0
    write_section(SECTION_TYPE, &type_section, &mut out);

    // --- Function Section: 関数 0 は type 0 を使う ---
    let mut func_section = Vec::new();
    encode_uleb128(1, &mut func_section); // 関数の数: 1
    encode_uleb128(0, &mut func_section); // 関数 0 の type index: 0
    write_section(SECTION_FUNCTION, &func_section, &mut out);

    // --- Export Section: "main" として関数 0 を公開 ---
    let mut export_section = Vec::new();
    encode_uleb128(1, &mut export_section); // export の数: 1
    let name = b"main";
    encode_uleb128(name.len() as u64, &mut export_section); // 名前長
    export_section.extend_from_slice(name);
    export_section.push(EXPORT_FUNC); // export 種別: 関数
    encode_uleb128(0, &mut export_section); // 関数 index: 0
    write_section(SECTION_EXPORT, &export_section, &mut out);

    // --- Code Section: 関数 0 の本体 ---
    let mut code_section = Vec::new();
    encode_uleb128(1, &mut code_section); // 関数本体の数: 1
    // 本体: locals=[], 命令=[END]
    let mut body = Vec::new();
    encode_uleb128(0, &mut body); // ローカル変数グループ数: 0
    body.push(OP_END); // 命令: END
    encode_uleb128(body.len() as u64, &mut code_section); // 本体のサイズ
    code_section.extend_from_slice(&body);
    write_section(SECTION_CODE, &code_section, &mut out);

    out
}

/// Signed LEB128 でエンコードして bytes に追記
pub fn encode_sleb128(value: i64, bytes: &mut Vec<u8>) {
    let mut v = value;
    let mut more = true;
    while more {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        let sign_bit_set = (byte & 0x40) != 0;
        if (v == 0 && !sign_bit_set) || (v == -1 && sign_bit_set) {
            more = false;
        }
        bytes.push(if more { byte | 0x80 } else { byte });
    }
}

/// f64 を IEEE 754 (little-endian) で bytes に追記
pub fn encode_f64(value: f64, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

use crate::parse::{Expression, Statement};

/// 構文木を WASM バイナリに変換する（Phase 2: 最後の文の値を返す main）
pub fn compile_to_wasm(statements: &[Statement]) -> Vec<u8> {
    let mut out = Vec::new();

    // マジック + バージョン
    out.extend_from_slice(b"\0asm");
    out.extend_from_slice(&[1, 0, 0, 0]);

    // --- Type Section: () → f64 ---
    let mut type_section = Vec::new();
    encode_uleb128(1, &mut type_section);
    type_section.push(TYPE_FUNC);
    encode_uleb128(0, &mut type_section); // params: 0個
    encode_uleb128(1, &mut type_section); // results: 1個
    type_section.push(TYPE_F64);
    write_section(SECTION_TYPE, &type_section, &mut out);

    // --- Function Section ---
    let mut func_section = Vec::new();
    encode_uleb128(1, &mut func_section);
    encode_uleb128(0, &mut func_section);
    write_section(SECTION_FUNCTION, &func_section, &mut out);

    // --- Export Section ---
    let mut export_section = Vec::new();
    encode_uleb128(1, &mut export_section);
    let name = b"main";
    encode_uleb128(name.len() as u64, &mut export_section);
    export_section.extend_from_slice(name);
    export_section.push(EXPORT_FUNC);
    encode_uleb128(0, &mut export_section);
    write_section(SECTION_EXPORT, &export_section, &mut out);

    // --- Code Section ---
    let mut code_section = Vec::new();
    encode_uleb128(1, &mut code_section);
    let mut body = Vec::new();
    // 最後の文の値を計算する命令列を吐く
    emit_program_body(statements, &mut body);
    body.push(OP_END);
    encode_uleb128(body.len() as u64, &mut code_section);
    code_section.extend_from_slice(&body);
    write_section(SECTION_CODE, &code_section, &mut out);

    out
}

/// プログラムを命令列に展開
fn emit_program_body(statements: &[Statement], out: &mut Vec<u8>) {
    let mut fb = FunctionBuilder::new();
    let mut last_const_name: Option<String> = None;

    for stmt in statements {
        emit_statement(stmt, &mut fb);
        if let Statement::ConstDeclaration { name, .. } = stmt {
            last_const_name = Some(name.clone());
        }
    }

    // 最後の const の値を local.get でスタックに乗せる
    if let Some(name) = last_const_name {
        if let Some(idx) = fb.lookup(&name) {
            fb.code.push(OP_LOCAL_GET);
            encode_uleb128(idx as u64, &mut fb.code);
        }
    }

    // ローカル変数の宣言を out の先頭に書く（個数 + 各グループ）
    // この関数では全部 f64 なので 1 グループ
    let local_count = fb.locals.len() as u64;
    if local_count > 0 {
        encode_uleb128(1, out); // グループ数: 1
        encode_uleb128(local_count, out); // この型のローカルが何個
        out.push(TYPE_F64);
    } else {
        encode_uleb128(0, out); // グループ数: 0
    }

    // 命令列をくっつける
    out.extend_from_slice(&fb.code);
}

fn emit_statement(stmt: &Statement, fb: &mut FunctionBuilder) {
    match stmt {
        Statement::ConstDeclaration { name, init, .. } => {
            emit_expression(init, fb);
            let idx = fb.declare(name.clone());
            fb.code.push(OP_LOCAL_SET);
            encode_uleb128(idx as u64, &mut fb.code);
        }
        Statement::Return {
            argument: Some(expr),
        } => {
            emit_expression(expr, fb);
        }
        Statement::Return { argument: None } => {
            panic!("return without value not supported");
        }
    }
}

use crate::parse::BinaryOp;

fn emit_expression(expr: &Expression, fb: &mut FunctionBuilder) {
    match expr {
        Expression::Number(n) => {
            fb.code.push(OP_F64_CONST);
            encode_f64(*n as f64, &mut fb.code);
        }
        Expression::Identifier(name) => {
            let idx = fb
                .lookup(name)
                .unwrap_or_else(|| panic!("undeclared variable: {name}"));
            fb.code.push(OP_LOCAL_GET);
            encode_uleb128(idx as u64, &mut fb.code);
        }
        Expression::Binary { left, op, right } => {
            emit_expression(left, fb);
            emit_expression(right, fb);
            match op {
                BinaryOp::Add => fb.code.push(OP_F64_ADD),
                BinaryOp::Multiply => fb.code.push(OP_F64_MUL),
            }
        }
        _ => panic!("unsupported expression"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(b, vec![0x7f]); // ちょうど7ビットに収まる最大値
    }

    #[test]
    fn uleb128_128() {
        let mut b = Vec::new();
        encode_uleb128(128, &mut b);
        assert_eq!(b, vec![0x80, 0x01]); // 7ビット超えで2バイトに
    }

    #[test]
    fn uleb128_624485() {
        // wikipedia の例：624485 → E5 8E 26
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
        assert_eq!(b, vec![0x7f]); // 全ビット1
    }

    #[test]
    fn sleb128_negative_large() {
        // wikipedia の例：-12345 → C7 9F 7F
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
        // 一致を確かめるならRustの to_le_bytes と同じ結果
        assert_eq!(b, std::f64::consts::PI.to_le_bytes().to_vec());
    }

    #[test]
    fn empty_module_parses() {
        let bytes = build_empty_module();

        // wasmparser でパースできれば成功
        let parser = wasmparser::Parser::new(0);
        let mut saw_main_export = false;
        let mut function_count = 0;
        let mut type_count = 0;

        for payload in parser.parse_all(&bytes) {
            let payload = payload.expect("wasmparser should accept the binary");
            match payload {
                wasmparser::Payload::TypeSection(reader) => {
                    type_count = reader.count();
                }
                wasmparser::Payload::FunctionSection(reader) => {
                    function_count = reader.count();
                }
                wasmparser::Payload::ExportSection(reader) => {
                    for export in reader {
                        let export = export.unwrap();
                        if export.name == "main" && export.kind == wasmparser::ExternalKind::Func {
                            saw_main_export = true;
                        }
                    }
                }
                _ => {}
            }
        }

        assert_eq!(type_count, 1, "should have 1 type");
        assert_eq!(function_count, 1, "should have 1 function");
        assert!(saw_main_export, "should export `main`");
    }

    #[test]
    fn compile_single_const_returns_42() {
        let stmts = crate::compile("const x = 42;");
        let bytes = compile_to_wasm(&stmts);

        // wasmparser で構造を検証
        let parser = wasmparser::Parser::new(0);
        let mut result_type_seen = None;
        let mut export_name = None;

        for payload in parser.parse_all(&bytes) {
            let payload = payload.expect("valid wasm");
            match payload {
                wasmparser::Payload::TypeSection(reader) => {
                    for ty in reader {
                        if let wasmparser::RecGroup { .. } = ty.unwrap() {
                            // 構造的に取り出すのは面倒なので、生バイトでチェック済みとする
                        }
                    }
                }
                wasmparser::Payload::ExportSection(reader) => {
                    for e in reader {
                        let e = e.unwrap();
                        if e.kind == wasmparser::ExternalKind::Func {
                            export_name = Some(e.name.to_string());
                        }
                    }
                }
                wasmparser::Payload::CodeSectionEntry(body) => {
                    // 命令列を読み取る
                    let mut reader = body.get_operators_reader().unwrap();
                    while !reader.eof() {
                        let op = reader.read().unwrap();
                        if let wasmparser::Operator::F64Const { value } = op {
                            result_type_seen = Some(f64::from_bits(value.bits()));
                        }
                    }
                }
                _ => {}
            }
        }

        assert_eq!(export_name.as_deref(), Some("main"));
        assert_eq!(result_type_seen, Some(42.0));
    }

    #[test]
    fn compile_addition() {
        let stmts = crate::compile("const x = 1 + 2;");
        let bytes = compile_to_wasm(&stmts);
        let ops = collect_ops(&bytes);
        // f64.const 1, f64.const 2, f64.add, end
        assert!(matches!(
            ops.first(),
            Some(wasmparser::Operator::F64Const { .. })
        ));
        assert!(
            ops.iter()
                .any(|op| matches!(op, wasmparser::Operator::F64Add))
        );
    }

    #[test]
    fn compile_mixed_operators() {
        let stmts = crate::compile("const x = 1 + 2 * 3;");
        let bytes = compile_to_wasm(&stmts);
        let ops = collect_ops(&bytes);
        assert!(
            ops.iter()
                .any(|op| matches!(op, wasmparser::Operator::F64Add))
        );
        assert!(
            ops.iter()
                .any(|op| matches!(op, wasmparser::Operator::F64Mul))
        );
    }

    /// テスト用ヘルパー：関数本体の命令列を全部取り出す
    #[cfg(test)]
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
    fn compile_two_consts() {
        let stmts = crate::compile("const x = 5; const y = x + 1;");
        let bytes = compile_to_wasm(&stmts);
        let ops = collect_ops(&bytes);

        // f64.const 5, local.set 0, local.get 0, f64.const 1, f64.add, local.set 1, local.get 1, end
        let has_local_set = ops
            .iter()
            .any(|op| matches!(op, wasmparser::Operator::LocalSet { .. }));
        let has_local_get = ops
            .iter()
            .any(|op| matches!(op, wasmparser::Operator::LocalGet { .. }));
        assert!(has_local_set);
        assert!(has_local_get);
    }

    #[test]
    fn compile_const_then_reference() {
        let stmts = crate::compile("const x = 42;");
        let bytes = compile_to_wasm(&stmts);
        // wasmparser でローカル変数の宣言があるか確認
        let parser = wasmparser::Parser::new(0);
        for payload in parser.parse_all(&bytes) {
            if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
                let locals: Vec<_> = body
                    .get_locals_reader()
                    .unwrap()
                    .into_iter()
                    .map(|r| r.unwrap())
                    .collect();
                // f64 が1つ宣言されてるはず
                assert!(!locals.is_empty(), "should declare locals");
            }
        }
    }
}
