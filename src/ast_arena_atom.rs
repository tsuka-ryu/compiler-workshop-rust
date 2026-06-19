use crate::atom::Atom;
use crate::tokenize_span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Statement<'a> {
    ConstDeclaration {
        name: Atom<'a>,
        type_annotation: Option<TypeAnnotation<'a>>,
        init: Expression<'a>,
        span: Span,
    },
    Return {
        argument: Option<Expression<'a>>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression<'a> {
    Number {
        value: i64,
        span: Span,
    },
    String {
        value: Atom<'a>,
        span: Span,
    },
    Boolean {
        value: bool,
        span: Span,
    },
    Identifier {
        name: Atom<'a>,
        span: Span,
    },
    Binary {
        left: &'a Expression<'a>,
        op: BinaryOp,
        right: &'a Expression<'a>,
        span: Span,
    },
    Conditional {
        test: &'a Expression<'a>,
        consequent: &'a Expression<'a>,
        alternate: &'a Expression<'a>,
        span: Span,
    },
    Call {
        callee: &'a Expression<'a>,
        arguments: Vec<Expression<'a>>,
        span: Span,
    },
    Array {
        elements: Vec<Expression<'a>>,
        span: Span,
    },
    Member {
        object: &'a Expression<'a>,
        index: &'a Expression<'a>,
        span: Span,
    },
    ArrowFunction {
        params: Vec<Parameter<'a>>,
        return_type: Option<TypeAnnotation<'a>>,
        body: Vec<Statement<'a>>,
        span: Span,
    },
}

impl<'a> Statement<'a> {
    pub fn span(&self) -> Span {
        match self {
            Statement::ConstDeclaration { span, .. } | Statement::Return { span, .. } => *span,
        }
    }
}

impl<'a> Expression<'a> {
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
pub struct Parameter<'a> {
    pub name: Atom<'a>,
    pub type_annotation: Option<TypeAnnotation<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation<'a> {
    Named {
        name: Atom<'a>,
        span: Span,
    },
    Array {
        element: &'a TypeAnnotation<'a>,
        span: Span,
    },
    Function {
        params: Vec<Parameter<'a>>,
        return_type: &'a TypeAnnotation<'a>,
        span: Span,
    },
}

impl<'a> TypeAnnotation<'a> {
    pub fn span(&self) -> Span {
        match self {
            TypeAnnotation::Named { span, .. }
            | TypeAnnotation::Array { span, .. }
            | TypeAnnotation::Function { span, .. } => *span,
        }
    }
}