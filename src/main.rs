use compiler_workshop::diagnostics::emit_parse_error;
use compiler_workshop::parse_result::parse_result;
use compiler_workshop::tokenize_span::tokenize_span;

fn main() {
    let source = "const x = (1 + 2;";

    match parse_result(tokenize_span(source)) {
        Ok(stmts) => {
            println!("parsed {} statements", stmts.len());
        }
        Err(err) => {
            emit_parse_error(source, &err);
        }
    }
}

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

    #[test]
    fn full_pipeline_clean() {
        let result = compiler_workshop::compile_full("const x = 1 + 2;");
        assert!(result.naming_errors.is_empty());
        assert!(result.type_errors.is_empty());
    }

    #[test]
    fn full_pipeline_naming_error() {
        let result = compiler_workshop::compile_full("const x = y;");
        assert_eq!(result.naming_errors.len(), 1);
        assert!(result.type_errors.is_empty());
    }

    #[test]
    fn full_pipeline_type_error() {
        let result = compiler_workshop::compile_full(r#"const x = 1 + "hi";"#);
        assert!(result.naming_errors.is_empty());
        assert_eq!(result.type_errors.len(), 1);
    }
}
