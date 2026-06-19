use crate::tokenize_span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    ConstDeclaration {
        name: String,
        type_annotation: Option<TypeAnnotation>,
        init: Expression,
        span: Span,
    },
    Return {
        argument: Option<Expression>,
        span: Span,
    },
    Error {
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Number {
        value: i64,
        span: Span,
    },
    String {
        value: String,
        span: Span,
    },
    Boolean {
        value: bool,
        span: Span,
    },
    Identifier {
        name: String,
        span: Span,
    },
    Binary {
        left: Box<Expression>,
        op: BinaryOp,
        right: Box<Expression>,
        span: Span,
    },
    Conditional {
        test: Box<Expression>,
        consequent: Box<Expression>,
        alternate: Box<Expression>,
        span: Span,
    },
    Call {
        callee: Box<Expression>,
        arguments: Vec<Expression>,
        span: Span,
    },
    Array {
        elements: Vec<Expression>,
        span: Span,
    },
    Member {
        object: Box<Expression>,
        index: Box<Expression>,
        span: Span,
    },
    ArrowFunction {
        params: Vec<Parameter>,
        return_type: Option<TypeAnnotation>,
        body: Vec<Statement>,
        span: Span,
    },
    Error {
        span: Span,
    },
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::ConstDeclaration { span, .. }
            | Statement::Return { span, .. }
            | Statement::Error { span, .. } => *span,
        }
    }
}

impl Expression {
    pub fn span(&self) -> Span {
        match self {
            Expression::Number { span, .. }
            | Expression::String { span, .. }
            | Expression::Boolean { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::Binary { span, .. }
            | Expression::Conditional { span, .. }
            | Expression::Call { span, .. }
            | Expression::Array { span, .. }
            | Expression::Member { span, .. }
            | Expression::ArrowFunction { span, .. }
            | Expression::Error { span, .. } => *span,
        }
    }
}

/// oxc の `Dummy` trait 相当。fatal error 時に「型を満たすためだけの捨て値」を作る。
/// fatal が立つと木ごと捨てられるので、中身は表に出ない。
pub trait Dummy {
    fn dummy(span: Span) -> Self;
}

impl Dummy for Expression {
    fn dummy(span: Span) -> Self {
        Expression::Error { span }
    }
}

impl Dummy for Statement {
    fn dummy(span: Span) -> Self {
        Statement::Error { span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Multiply,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub type_annotation: Option<TypeAnnotation>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation {
    Named {
        name: String,
        span: Span,
    },
    Array {
        element: Box<TypeAnnotation>,
        span: Span,
    },
    Function {
        params: Vec<Parameter>,
        return_type: Box<TypeAnnotation>,
        span: Span,
    },
}

impl TypeAnnotation {
    pub fn span(&self) -> Span {
        match self {
            TypeAnnotation::Named { span, .. }
            | TypeAnnotation::Array { span, .. }
            | TypeAnnotation::Function { span, .. } => *span,
        }
    }
}
