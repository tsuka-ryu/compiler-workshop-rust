use crate::tokenize::tokenize;

pub mod ast_span;
pub mod diagnostics;
pub mod monomorphize;
pub mod naming;
pub mod parse;
pub mod parse_result;
pub mod parse_span;
pub mod tokenize;
pub mod tokenize_span;
pub mod typecheck;
pub mod typecheck_mono;
pub mod visit;
pub mod wasm;

pub struct CompileResult {
    pub statements: Vec<parse::Statement>,
    pub naming_errors: Vec<naming::NamingError>,
    pub type_errors: Vec<typecheck::TypeError>,
}

pub fn compile(source: &str) -> Vec<parse::Statement> {
    parse::parse(tokenize(source))
}

pub fn compile_full(source: &str) -> CompileResult {
    let statements = parse::parse(tokenize(source));
    let naming_errors = naming::name_check(&statements);
    let type_errors = typecheck::type_check(&statements);
    CompileResult {
        statements,
        naming_errors,
        type_errors,
    }
}

/// ソースコードを単相化してから wasm バイナリにコンパイルする。
///
/// パイプライン: tokenize → parse → monomorphize → wasm
pub fn compile_to_wasm_full(source: &str) -> Vec<u8> {
    let statements = parse::parse(tokenize(source));
    let mono = monomorphize::monomorphize(&statements);
    wasm::compile_to_wasm(&mono)
}
