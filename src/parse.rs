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
        let init = self.parse_expression();

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

    fn parse_return_statement(&mut self) -> Statement {
        // return
        self.advance();

        // 引数は省略可能。次が ; か } EoFなら省略
        let argument = if matches!(self.peek(), Token::Semicolon | Token::RCurly | Token::EoF) {
            None
        } else {
            Some(self.parse_expression())
        };

        // 末尾の ; があれば消費
        if matches!(self.peek(), Token::Semicolon) {
            self.advance();
        }

        Statement::Return { argument }
    }

    fn parse_expression(&mut self) -> Expression {
        let test = self.parse_binary();

        if matches!(self.peek(), Token::Ternary) {
            self.advance(); // ?を消費

            let consequent = self.parse_expression();

            // : を期待
            match self.advance() {
                Token::Colon => {}
                other => panic!("Expected ':' in ternary, got {other:?}"),
            }

            let alternate = self.parse_expression();

            Expression::Conditional {
                test: Box::new(test),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            }
        } else {
            test
        }
    }

    fn parse_binary(&mut self) -> Expression {
        let mut left = self.parse_primary();

        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOp::Add,
                Token::Multiply => BinaryOp::Multiply,
                _ => break,
            };
            self.advance(); // 演算子を消費
            let right = self.parse_primary();
            left = Expression::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            }
        }

        left
    }

    fn parse_primary(&mut self) -> Expression {
        let mut expr = match self.advance() {
            Token::Number(n) => Expression::Number(n),
            Token::StringLit(s) => Expression::String(s),
            Token::Boolean(b) => Expression::Boolean(b),
            Token::Ident(name) => Expression::Identifier(name),
            Token::LBracket => self.parse_array(),
            other => panic!("Unexpected token in expression: {other:?}"),
        };

        // 後置: ( で呼び出し [ でメンバアクセスを繰り返す
        loop {
            match self.peek() {
                Token::LParen => {
                    expr = self.parse_call(expr);
                }
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expression();
                    match self.advance() {
                        Token::RBracket => {}
                        other => panic!("Expected ']' in member access, got {other:?}"),
                    }
                    expr = Expression::Member {
                        object: Box::new(expr),
                        index: Box::new(index),
                    }
                }
                _ => break,
            }
        }

        expr
    }

    fn parse_call(&mut self, callee: Expression) -> Expression {
        self.advance(); // ( を消費

        let mut arguments = Vec::new();

        // 空引数 ()
        if matches!(self.peek(), Token::RParen) {
            self.advance();
            return Expression::Call {
                callee: Box::new(callee),
                arguments,
            };
        }

        // 引数を1個以上 + カンマ区切り
        loop {
            arguments.push(self.parse_expression());
            match self.peek() {
                Token::Comma => {
                    self.advance();
                }
                _ => break,
            }
        }

        // ) を期待
        match self.advance() {
            Token::RParen => {}
            other => panic!("Expected ')', got {other:?}"),
        }

        Expression::Call {
            callee: Box::new(callee),
            arguments,
        }
    }

    fn parse_array(&mut self) -> Expression {
        // [ はすでに advance済み

        let mut elements = Vec::new();

        // 空配列 []
        if matches!(self.peek(), Token::RBracket) {
            self.advance();
            return Expression::Array(elements);
        }

        loop {
            elements.push(self.parse_expression());
            match self.peek() {
                Token::Comma => {
                    self.advance();
                }
                _ => break,
            }
        }

        match self.advance() {
            Token::RBracket => {}
            other => panic!("Expected ']', got {other:?}"),
        }

        Expression::Array(elements)
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

    #[test]
    fn parse_addition() {
        let tokens = tokenize("const x = 1 + 2;");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: None,
                init: Expression::Binary {
                    left: Box::new(Expression::Number(1)),
                    op: BinaryOp::Add,
                    right: Box::new(Expression::Number(2)),
                },
            }]
        );
    }

    #[test]
    fn parse_left_associative() {
        // 1 + 2 + 3 → ((1 + 2) + 3)
        let tokens = tokenize("const x = 1 + 2 + 3;");
        let stmts = parse(tokens);
        let expected = Expression::Binary {
            left: Box::new(Expression::Binary {
                left: Box::new(Expression::Number(1)),
                op: BinaryOp::Add,
                right: Box::new(Expression::Number(2)),
            }),
            op: BinaryOp::Add,
            right: Box::new(Expression::Number(3)),
        };
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: None,
                init: expected,
            }]
        );
    }

    #[test]
    fn parse_mixed_operators() {
        // 1 + 2 * 3 → ((1 + 2) * 3)（JS版は同じ優先順位の左結合）
        let tokens = tokenize("const x = 1 + 2 * 3;");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            // 一番外側が * になることを確認
            if let Expression::Binary { op, .. } = init {
                assert_eq!(*op, BinaryOp::Multiply);
            } else {
                panic!("expected Binary, got {init:?}");
            }
        }
    }

    #[test]
    fn parse_ternary() {
        let tokens = tokenize(r#"const x = true ? 1 : 2;"#);
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: None,
                init: Expression::Conditional {
                    test: Box::new(Expression::Boolean(true)),
                    consequent: Box::new(Expression::Number(1)),
                    alternate: Box::new(Expression::Number(2)),
                },
            }]
        );
    }

    #[test]
    fn parse_ternary_right_associative() {
        // a ? b : c ? d : e → a ? b : (c ? d : e)
        let tokens = tokenize("const x = a ? b : c ? d : e;");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            if let Expression::Conditional { alternate, .. } = init {
                // 外側の alternate が Conditional であることを確認
                assert!(matches!(**alternate, Expression::Conditional { .. }));
            } else {
                panic!("expected Conditional");
            }
        }
    }

    #[test]
    fn parse_call_no_args() {
        let tokens = tokenize("const x = f();");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: None,
                init: Expression::Call {
                    callee: Box::new(Expression::Identifier("f".to_string())),
                    arguments: vec![],
                },
            }]
        );
    }

    #[test]
    fn parse_call_with_args() {
        let tokens = tokenize("const x = add(1, 2);");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: None,
                init: Expression::Call {
                    callee: Box::new(Expression::Identifier("add".to_string())),
                    arguments: vec![Expression::Number(1), Expression::Number(2)],
                },
            }]
        );
    }

    #[test]
    fn parse_call_nested() {
        let tokens = tokenize("const x = f(g(1));");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            if let Expression::Call { arguments, .. } = init {
                assert!(matches!(arguments[0], Expression::Call { .. }));
            }
        }
    }

    #[test]
    fn parse_array_empty() {
        let tokens = tokenize("const x = [];");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: None,
                init: Expression::Array(vec![]),
            }]
        );
    }

    #[test]
    fn parse_array_numbers() {
        let tokens = tokenize("const x = [1, 2, 3];");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: None,
                init: Expression::Array(vec![
                    Expression::Number(1),
                    Expression::Number(2),
                    Expression::Number(3),
                ]),
            }]
        );
    }

    #[test]
    fn parse_array_mixed() {
        let tokens = tokenize(r#"const x = [1, "two", true];"#);
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration {
            init: Expression::Array(items),
            ..
        } = &stmts[0]
        {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0], Expression::Number(1)));
            assert!(matches!(items[1], Expression::String(_)));
            assert!(matches!(items[2], Expression::Boolean(true)));
        }
    }

    #[test]
    fn parse_member_access() {
        let tokens = tokenize("const x = arr[0];");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: None,
                init: Expression::Member {
                    object: Box::new(Expression::Identifier("arr".to_string())),
                    index: Box::new(Expression::Number(0)),
                },
            }]
        );
    }

    #[test]
    fn parse_member_chained() {
        // arr[0][1] → Member(Member(arr, 0), 1)
        let tokens = tokenize("const x = arr[0][1];");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            if let Expression::Member { object, index } = init {
                assert!(matches!(**index, Expression::Number(1)));
                assert!(matches!(**object, Expression::Member { .. }));
            }
        }
    }

    #[test]
    fn parse_call_then_member() {
        // f()[0]
        let tokens = tokenize("const x = f()[0];");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            assert!(matches!(init, Expression::Member { .. }));
        }
    }

    #[test]
    fn parse_member_then_call() {
        // arr[0]()
        let tokens = tokenize("const x = arr[0]();");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            assert!(matches!(init, Expression::Call { .. }));
        }
    }
}
