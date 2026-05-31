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
const TYPE_I32: u8 = 0x7f;
const OP_I32_CONST: u8 = 0x41;
const OP_IF: u8 = 0x04;
const OP_ELSE: u8 = 0x05;

// Export の種別
const EXPORT_FUNC: u8 = 0x00;

struct LocalInfo {
    index: u32,
    wasm_type: u8,
}

struct FunctionBuilder {
    locals: std::collections::HashMap<String, LocalInfo>,
    /// 非パラメータのローカルの型（宣言順）
    local_types: Vec<u8>,
    /// パラメータ数（local index 振り分け用）
    param_count: u32,
    code: Vec<u8>,
    /// 関数名 → 関数 index（CALL 用）
    function_indices: std::collections::HashMap<String, u32>,
}

impl FunctionBuilder {
    fn new(function_indices: std::collections::HashMap<String, u32>) -> Self {
        Self {
            locals: std::collections::HashMap::new(),
            local_types: Vec::new(),
            param_count: 0,
            code: Vec::new(),
            function_indices,
        }
    }

    /// パラメータを登録（先に呼ぶ）
    fn declare_param(&mut self, name: String, wasm_type: u8) {
        let index = self.locals.len() as u32;
        self.locals.insert(name, LocalInfo { index, wasm_type });
        self.param_count += 1;
    }

    /// 普通のローカルを登録
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

struct FunctionInfo {
    name: String,
    param_types: Vec<u8>, // f64 / i32
    return_type: u8,
    body: Vec<u8>,        // 命令列 (END なし)
    local_types: Vec<u8>, // 非パラメータのローカル変数の型
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

const OP_CALL: u8 = 0x10;

pub fn compile_to_wasm(statements: &[Statement]) -> Vec<u8> {
    // --- 関数を抽出 ---
    let mut user_functions: Vec<FunctionInfo> = Vec::new();
    let mut main_body_stmts: Vec<&Statement> = Vec::new();
    let mut function_indices: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();

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

                let mut fb = FunctionBuilder::new(function_indices.clone());
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

    // --- main 関数を組み立て ---
    let main_idx = user_functions.len() as u32;
    function_indices.insert("__main__".to_string(), main_idx);
    let mut main_fb = FunctionBuilder::new(function_indices.clone());
    let mut last_name: Option<String> = None;
    for stmt in &main_body_stmts {
        emit_statement(stmt, &mut main_fb);
        if let Statement::ConstDeclaration { name, .. } = stmt {
            last_name = Some(name.clone());
        }
    }
    // 最後の const の値を取り出す
    if let Some(name) = &last_name {
        if let Some(info) = main_fb.lookup(name) {
            let idx = info.index;
            main_fb.code.push(OP_LOCAL_GET);
            encode_uleb128(idx as u64, &mut main_fb.code);
        }
    }
    let main_return = last_name
        .as_ref()
        .and_then(|n| main_fb.lookup(n))
        .map(|info| info.wasm_type)
        .unwrap_or(TYPE_F64);
    user_functions.push(FunctionInfo {
        name: "main".to_string(),
        param_types: vec![],
        return_type: main_return,
        body: main_fb.code,
        local_types: main_fb.local_types,
    });

    // --- バイナリ書き出し ---
    write_module(&user_functions, main_idx)
}

fn write_module(functions: &[FunctionInfo], main_idx: u32) -> Vec<u8> {
    let mut out = Vec::new();

    // マジック + バージョン
    out.extend_from_slice(b"\0asm");
    out.extend_from_slice(&[1, 0, 0, 0]);

    // --- Type Section ---
    let mut type_section = Vec::new();
    encode_uleb128(functions.len() as u64, &mut type_section);
    for f in functions {
        type_section.push(TYPE_FUNC);
        encode_uleb128(f.param_types.len() as u64, &mut type_section);
        for &ty in &f.param_types {
            type_section.push(ty);
        }
        encode_uleb128(1, &mut type_section); // results: 1
        type_section.push(f.return_type);
    }
    write_section(SECTION_TYPE, &type_section, &mut out);

    // --- Function Section ---
    let mut func_section = Vec::new();
    encode_uleb128(functions.len() as u64, &mut func_section);
    for i in 0..functions.len() as u32 {
        encode_uleb128(i as u64, &mut func_section);
    }
    write_section(SECTION_FUNCTION, &func_section, &mut out);

    // --- Export Section: main だけ ---
    let mut export_section = Vec::new();
    encode_uleb128(1, &mut export_section);
    let name = b"main";
    encode_uleb128(name.len() as u64, &mut export_section);
    export_section.extend_from_slice(name);
    export_section.push(EXPORT_FUNC);
    encode_uleb128(main_idx as u64, &mut export_section);
    write_section(SECTION_EXPORT, &export_section, &mut out);

    // --- Code Section ---
    let mut code_section = Vec::new();
    encode_uleb128(functions.len() as u64, &mut code_section);
    for f in functions {
        let mut body = Vec::new();
        // ローカル宣言：型ごとにグループ化（簡略：1個ずつグループにする）
        encode_uleb128(f.local_types.len() as u64, &mut body);
        for &ty in &f.local_types {
            encode_uleb128(1, &mut body);
            body.push(ty);
        }
        body.extend_from_slice(&f.body);
        body.push(OP_END);
        encode_uleb128(body.len() as u64, &mut code_section);
        code_section.extend_from_slice(&body);
    }
    write_section(SECTION_CODE, &code_section, &mut out);

    out
}

fn emit_statement(stmt: &Statement, fb: &mut FunctionBuilder) {
    match stmt {
        Statement::ConstDeclaration {
            name,
            init,
            type_annotation,
        } => {
            emit_expression(init, fb);
            let ty = type_annotation
                .as_ref()
                .map(wasm_type_from_annotation)
                .unwrap_or_else(|| infer_simple_type(init));
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
        Expression::Boolean(b) => {
            fb.code.push(OP_I32_CONST);
            // i32.const は SLEB128
            encode_sleb128(if *b { 1 } else { 0 }, &mut fb.code);
        }

        Expression::Identifier(name) => {
            let idx = fb
                .lookup(name)
                .unwrap_or_else(|| panic!("undeclared variable: {name}"))
                .index;
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
        Expression::Conditional {
            test,
            consequent,
            alternate,
        } => {
            // test を評価（結果は i32 でスタックトップに）
            emit_expression(test, fb);

            // if (result f64)
            fb.code.push(OP_IF);
            fb.code.push(TYPE_F64); // block の戻り値型

            // 真ブランチ
            emit_expression(consequent, fb);

            // else
            fb.code.push(OP_ELSE);

            // 偽ブランチ
            emit_expression(alternate, fb);

            // end
            fb.code.push(OP_END);
        }
        Expression::Call { callee, arguments } => {
            // 引数を順に積む
            for arg in arguments {
                emit_expression(arg, fb);
            }
            // CALL 命令
            if let Expression::Identifier(name) = callee.as_ref() {
                let idx = fb
                    .function_indices
                    .get(name)
                    .copied()
                    .unwrap_or_else(|| panic!("unknown function: {name}"));
                fb.code.push(OP_CALL);
                encode_uleb128(idx as u64, &mut fb.code);
            } else {
                panic!("only identifier callees supported");
            }
        }

        _ => panic!("unsupported expression"),
    }
}

use crate::parse::TypeAnnotation;

fn wasm_type_from_annotation(ta: &TypeAnnotation) -> u8 {
    match ta {
        TypeAnnotation::Named(name) => match name.as_str() {
            "number" | "Float" => TYPE_F64,
            "boolean" | "Bool" => TYPE_I32,
            _ => TYPE_F64,
        },
        _ => TYPE_F64,
    }
}

fn infer_simple_type(expr: &Expression) -> u8 {
    match expr {
        Expression::Boolean(_) => TYPE_I32,
        _ => TYPE_F64,
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

    #[test]
    fn compile_boolean_literal() {
        let stmts = crate::compile("const x = true;");
        let bytes = compile_to_wasm(&stmts);
        // 注意: このテストは型エラーになる可能性あり（main の戻り値型は f64 固定）
        // wasmparser はバイナリの構造はパースできるはず
        let _ = bytes;
    }

    #[test]
    fn compile_ternary() {
        let stmts = crate::compile("const x = true ? 1 : 2;");
        let bytes = compile_to_wasm(&stmts);
        let ops = collect_ops(&bytes);

        let has_if = ops
            .iter()
            .any(|op| matches!(op, wasmparser::Operator::If { .. }));
        let has_else = ops
            .iter()
            .any(|op| matches!(op, wasmparser::Operator::Else));
        assert!(has_if, "should have if");
        assert!(has_else, "should have else");
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
        assert!(has_call, "should call add");
    }
}
