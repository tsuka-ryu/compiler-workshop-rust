use crate::tokenize::tokenize;

pub mod naming;
pub mod parse;
pub mod tokenize;
pub mod typecheck;
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
