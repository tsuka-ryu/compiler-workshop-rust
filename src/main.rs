fn main() {}

#[cfg(test)]
mod tests {
    use compiler_workshop::tokenize::{Token, tokenize};

    #[test]
    fn tokenize_const_decl() {
        let tokens = tokenize("const x = 5;");
        assert_eq!(
            tokens,
            vec![
                Token::Const,
                Token::Ident("x".into()),
                Token::Eq,
                Token::Number(5),
                Token::Semicolon,
                Token::EoF
            ]
        );
    }

    #[test]
    fn compile_end_to_end() {
        use compiler_workshop::compile;
        use compiler_workshop::parse::{BinaryOp, Expression, Statement, TypeAnnotation};

        let stmts = compile("const x: number = 1 + 2;");
        assert_eq!(
            stmts,
            vec![Statement::ConstDeclaration {
                name: "x".to_string(),
                type_annotation: Some(TypeAnnotation::Named("number".to_string())),
                init: Expression::Binary {
                    left: Box::new(Expression::Number(1)),
                    op: BinaryOp::Add,
                    right: Box::new(Expression::Number(2)),
                },
            }]
        );
    }
}
