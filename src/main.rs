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
}
