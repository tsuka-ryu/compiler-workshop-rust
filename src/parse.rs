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

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        self.pos += 1;
        tok
    }

    fn at_end(&self) -> bool {
        matches!(self.peek(), Token::EoF)
    }

    fn parse_statement(&mut self) -> Statement {
        match self.peek() {
            Token::Const => self.parse_const_declaration(),
            other => panic!("Unexpected token: {other:?}"),
        }
    }

    fn parse_const_declaration(&mut self) -> Statement {
        // const
        self.advance();

        // 識別子
        let name = match self.advance() {
            Token::Ident(s) => s,
            other => panic!("Expected identifier, got {other:?}"),
        };

        // =
        match self.advance() {
            Token::Eq => {}
            other => panic!("Expected '=', got {other:?}"),
        }

        // 初期化式
        let init = self.parser_expression();

        // 末尾の;があれば消費
        if matches!(self.peek(), Token::Semicolon) {
            self.advance();
        }

        Statement::ConstDeclaration {
            name,
            type_annotation: None,
            init,
        }
    }

    fn parser_expression(&mut self) -> Expression {
        match self.advance() {
            Token::Number(n) => Expression::Number(n),
            other => panic!("Unexpected token in expression: {other:?}"),
        }
    }
}

pub fn parse(tokens: Vec<Token>) -> Vec<Statement> {
    let mut parser = Parser { tokens, pos: 0 };
    let mut statements = Vec::new();
    while !parser.at_end() {
        statements.push(parser.parse_statement());
    }
    statements
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenize::tokenize;

    #[test]
    fn parse_const_number() {
        let tokens = tokenize("const x = 5;");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: None,
                init: Expression::Number(5),
            }]
        );
    }
}
