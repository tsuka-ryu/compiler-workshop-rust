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

    fn looks_like_arrow_fn(&self) -> bool {
        // ( はすでに消費積み、現在は ( の次
        let mut depth = 1;
        let mut i = self.pos;
        while i < self.tokens.len() {
            match &self.tokens[i] {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        let mut j = i + 1;
                        // 戻り値の型注釈`: SomeType`をスキップ（簡易版）
                        // 戻り値の型が`Array<T>`やT[]だと壊れる
                        if matches!(self.tokens.get(j), Some(Token::Colon)) {
                            j += 1;
                            // 型トークンを1つだけスキップ。Array<T> や T[] は未対応
                            if matches!(
                                self.tokens.get(j),
                                Some(
                                    Token::TypeNumber
                                        | Token::TypeString
                                        | Token::TypeBoolean
                                        | Token::TypeVoid
                                        | Token::TypeInt
                                        | Token::TypeFloat
                                        | Token::TypeBool
                                        | Token::TypeUnit
                                        | Token::TypeArray
                                        | Token::Ident(_)
                                )
                            ) {
                                j += 1;
                            }
                        }
                        return matches!(self.tokens.get(j), Some(Token::Arrow));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn parse_statement(&mut self) -> Statement {
        match self.peek() {
            Token::Const => self.parse_const_declaration(),
            Token::Return => self.parse_return_statement(),
            other => panic!("Unexpected token: {other:?}"),
        }
    }

    fn parse_const_declaration(&mut self) -> Statement {
        self.advance(); // const

        // 識別子
        let name = match self.advance() {
            Token::Ident(s) => s,
            other => panic!("Expected identifier, got {other:?}"),
        };

        // : 型があれば読む
        let type_annotation = if matches!(self.peek(), Token::Colon) {
            self.advance(); // : を消費
            Some(self.parse_type_annotation())
        } else {
            None
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
            type_annotation,
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
            Token::LParen => {
                if self.looks_like_arrow_fn() {
                    self.parse_arrow_function()
                } else {
                    let expr = self.parse_expression();
                    match self.advance() {
                        Token::RParen => {}
                        other => panic!("Expected ')', got {other:?}"),
                    }
                    expr
                }
            }
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

    fn parse_arrow_function(&mut self) -> Expression {
        // ( は消費積み

        let mut params = Vec::new();

        if !matches!(self.peek(), Token::RParen) {
            loop {
                let name = match self.advance() {
                    Token::Ident(s) => s,
                    other => panic!("Expected param name, got {other:?}"),
                };

                // 引数の型注釈
                let type_annotation = if matches!(self.peek(), Token::Colon) {
                    self.advance();
                    Some(self.parse_type_annotation())
                } else {
                    None
                };

                params.push(Parameter {
                    name,
                    type_annotation,
                });

                match self.peek() {
                    Token::Comma => {
                        self.advance();
                    }
                    _ => break,
                }
            }
        }

        // ) を消費
        match self.advance() {
            Token::RParen => {}
            other => panic!("Expected ')', got {other:?}"),
        }

        // 戻り値の型注釈
        let return_type = if matches!(self.peek(), Token::Colon) {
            self.advance();
            Some(self.parse_type_annotation())
        } else {
            None
        };

        // => を消費
        match self.advance() {
            Token::Arrow => {}
            other => panic!("Expected '=>', got {other:?}"),
        }

        // { を消費
        match self.advance() {
            Token::LCurly => {}
            other => panic!("Expected '{{', got {other:?}"),
        }

        // 本体の文を読む
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RCurly) {
            body.push(self.parse_statement());
        }

        // } を消費
        self.advance();

        Expression::ArrowFunction {
            params,
            return_type,
            body,
        }
    }

    fn parse_type_annotation(&mut self) -> TypeAnnotation {
        // : は呼び出し側で消費済み
        let base = match self.advance() {
            Token::TypeNumber => TypeAnnotation::Named("number".to_string()),
            Token::TypeString => TypeAnnotation::Named("string".to_string()),
            Token::TypeBoolean => TypeAnnotation::Named("boolean".to_string()),
            Token::TypeVoid => TypeAnnotation::Named("void".to_string()),
            Token::TypeInt => TypeAnnotation::Named("Void".to_string()),
            Token::TypeFloat => TypeAnnotation::Named("Float".to_string()),
            Token::TypeBool => TypeAnnotation::Named("Bool".to_string()),
            Token::TypeUnit => TypeAnnotation::Named("Unit".to_string()),
            Token::TypeArray => {
                // Array<T> 形式
                if matches!(self.peek(), Token::LessThan) {
                    self.advance(); // <
                    let elem = self.parse_type_annotation();
                    match self.advance() {
                        Token::GreaterThan => {}
                        other => panic!("Expected '>', got {other:?}"),
                    }
                    return TypeAnnotation::Array(Box::new(elem));
                }
                TypeAnnotation::Named("Array".to_string())
            }
            Token::Ident(name) => TypeAnnotation::Named(name),
            other => panic!("Expected type annotation, got {other:?}"),
        };

        // T[] 形式の後置 []
        if matches!(self.peek(), Token::LBracket) {
            self.advance();
            match self.advance() {
                Token::RBracket => {}
                other => panic!("Expected ']' in array type, got {other:?}"),
            }
            return TypeAnnotation::Array(Box::new(base));
        }

        base
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

    #[test]
    fn parse_arrow_no_params() {
        let tokens = tokenize("const f = () => { return 1; };");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            if let Expression::ArrowFunction { params, body, .. } = init {
                assert_eq!(params.len(), 0);
                assert_eq!(body.len(), 1);
            } else {
                panic!("expected ArrowFunction");
            }
        }
    }

    #[test]
    fn parse_arrow_with_params() {
        let tokens = tokenize("const add = (a, b) => { return a + b; };");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            if let Expression::ArrowFunction { params, .. } = init {
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "a");
                assert_eq!(params[1].name, "b");
            }
        }
    }

    #[test]
    fn parse_paren_expression() {
        // ( がアロー関数じゃない場合は括弧式
        let tokens = tokenize("const x = (1 + 2);");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            assert!(matches!(init, Expression::Binary { .. }));
        }
    }

    #[test]
    fn parse_const_with_type() {
        let tokens = tokenize("const x: number = 5;");
        let stmts = parse(tokens);
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: Some(TypeAnnotation::Named("number".to_string())),
                init: Expression::Number(5),
            }]
        );
    }

    #[test]
    fn parse_arrow_with_types() {
        let tokens = tokenize("const add = (a: number, b: number): number => { return a + b; };");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            if let Expression::ArrowFunction {
                params,
                return_type,
                ..
            } = init
            {
                assert_eq!(
                    params[0].type_annotation,
                    Some(TypeAnnotation::Named("number".to_string()))
                );
                assert_eq!(
                    *return_type,
                    Some(TypeAnnotation::Named("number".to_string()))
                );
            }
        }
    }

    #[test]
    fn parse_array_type() {
        let tokens = tokenize("const xs: number[] = [];");
        let stmts = parse(tokens);
        if let Statement::ConstDeclaration {
            type_annotation, ..
        } = &stmts[0]
        {
            assert!(matches!(type_annotation, Some(TypeAnnotation::Array(_))));
        }
    }
}
