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
            Token::Return => self.parse_return_statement(),
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
        self.parser_primary()
    }

    fn parser_primary(&mut self) -> Expression {
        match self.advance() {
            Token::Number(n) => Expression::Number(n),
            Token::StringLit(s) => Expression::String(s),
            Token::Boolean(b) => Expression::Boolean(b),
            Token::Ident(name) => Expression::Identifier(name),
            other => panic!("Unexpected token in expression: {other:?}"),
        }
    }

    fn parse_return_statement(&mut self) -> Statement {
        // return
        self.advance();

        // 引数は省略可能。次が ; か } EoFなら省略
        let argument = if matches!(self.peek(), Token::Semicolon | Token::RCurly | Token::EoF) {
            None
        } else {
            Some(self.parser_expression())
        };

        // 末尾の ; があれば消費
        if matches!(self.peek(), Token::Semicolon) {
            self.advance();
        }

        Statement::Return { argument }
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

    #[test]
    fn parse_return_with_number() {
        let tokens = tokenize("return 42;");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::Return {
                argument: Some(Expression::Number(42))
            }]
        );
    }

    #[test]
    fn parse_return_empty() {
        let tokens = tokenize("return;");
        let stmts = parse(tokens);
        assert_eq!(stmts, vec![Statement::Return { argument: None }]);
    }

    #[test]
    fn parse_multiple_statements() {
        let tokens = tokenize("const x = 1; const y = 2;");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![
                Statement::ConstDeclaration {
                    name: "x".to_string(),
                    type_annotation: None,
                    init: Expression::Number(1),
                },
                Statement::ConstDeclaration {
                    name: "y".to_string(),
                    type_annotation: None,
                    init: Expression::Number(2),
                },
            ]
        );
    }

    #[test]
    fn parse_const_string() {
        let tokens = tokenize(r#"const msg = "hello";"#);
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "msg".to_string(),
                type_annotation: None,
                init: Expression::String("hello".to_string()),
            }]
        );
    }

    #[test]
    fn parse_const_boolean() {
        let tokens = tokenize("const flag = true;");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "flag".to_string(),
                type_annotation: None,
                init: Expression::Boolean(true),
            }]
        );
    }

    #[test]
    fn parse_const_identifier() {
        let tokens = tokenize("const y = x;");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "y".to_string(),
                type_annotation: None,
                init: Expression::Identifier("x".to_string()),
            }]
        );
    }
}
