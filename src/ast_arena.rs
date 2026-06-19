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
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::ConstDeclaration { span, .. } | Statement::Return { span, .. } => *span,
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
            | Expression::ArrowFunction { span, .. } => *span,
        }
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
