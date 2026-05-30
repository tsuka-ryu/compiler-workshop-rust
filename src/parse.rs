use crate::tokenize::Token;

#[derive(Debug, PartialEq)]
pub enum Statement {
    ConstDeclaration {
        name: String,
        type_annotation: Option<TypeAnnotation>,
        init: Expression,
    },
    Return {
        argument: Option<Expression>,
    },
}

#[derive(Debug, PartialEq)]
pub enum Expression {
    Number(i64),
    String(String),
    Boolean(bool),
    Identifier(String),
    Binary {
        left: Box<Expression>,
        op: BinaryOp,
        right: Box<Expression>,
    },
    Conditional {
        test: Box<Expression>,
        consequent: Box<Expression>,
        alternate: Box<Expression>,
    },
    Call {
        callee: Box<Expression>,
        arguments: Vec<Expression>,
    },
    Array(Vec<Expression>),
    Member {
        object: Box<Expression>,
        index: Box<Expression>,
    },
    ArrowFunction {
        params: Vec<Parameter>,
        return_type: Option<TypeAnnotation>,
        body: Vec<Statement>,
    },
}

#[derive(Debug, PartialEq)]
pub enum BinaryOp {
    Add,
    Multiply,
}

#[derive(Debug, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub type_annotation: Option<TypeAnnotation>,
}

#[derive(Debug, PartialEq)]
pub enum TypeAnnotation {
    Named(String),
    Array(Box<TypeAnnotation>),
    Function {
        params: Vec<Parameter>,
        return_type: Box<TypeAnnotation>,
    },
}

pub fn parse(tokens: Vec<Token>) -> Vec<Statement> {
    todo!()
}
