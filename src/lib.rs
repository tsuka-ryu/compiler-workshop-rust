use crate::tokenize::tokenize;

pub mod naming;
pub mod parse;
pub mod tokenize;
pub mod typecheck;

pub fn compile(source: &str) -> Vec<parse::Statement> {
    parse::parse(tokenize(source))
}
