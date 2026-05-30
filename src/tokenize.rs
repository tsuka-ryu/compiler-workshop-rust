#[derive(Debug, PartialEq)]
pub enum Token {
    // Keywords
    Const,

    // Operators and punctuation
    Arrow,       // =>
    Ternary,     // ?
    Colon,       // :
    Eq,          // =
    Pipe,        // |
    LessThan,    // <
    GreaterThan, // >
    Multiply,    // *
    Plus,        // +
    LParen,      // (
    RParen,      // )
    LCurly,      // {
    RCurly,      // }
    LBracket,    // [
    RBracket,    // ]
    Comma,       // ,
    Semicolon,   // ;

    // Literals and identifiers
    Number(i64),
    Ident(String),
}

pub fn tokenize(src: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = src.char_indices().peekable();

    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }

        match c {
            '=' => {
                chars.next();
                if let Some(&(_, '>')) = chars.peek() {
                    chars.next();
                    tokens.push(Token::Arrow);
                } else {
                    tokens.push(Token::Eq);
                }
            }
            ';' => {
                chars.next();
                tokens.push(Token::Semicolon);
            }
            '?' => {
                chars.next();
                tokens.push(Token::Ternary);
            }
            ':' => {
                chars.next();
                tokens.push(Token::Colon);
            }
            '|' => {
                chars.next();
                tokens.push(Token::Pipe);
            }
            '<' => {
                chars.next();
                tokens.push(Token::LessThan);
            }
            '>' => {
                chars.next();
                tokens.push(Token::GreaterThan);
            }
            '*' => {
                chars.next();
                tokens.push(Token::Multiply);
            }
            '+' => {
                chars.next();
                tokens.push(Token::Plus);
            }
            ',' => {
                chars.next();
                tokens.push(Token::Comma);
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            '{' => {
                chars.next();
                tokens.push(Token::LCurly);
            }
            '}' => {
                chars.next();
                tokens.push(Token::RCurly);
            }
            '[' => {
                chars.next();
                tokens.push(Token::LBracket);
            }
            ']' => {
                chars.next();
                tokens.push(Token::RBracket);
            }
            '0'..='9' => {
                let start = chars.peek().unwrap().0;
                let mut end = start;
                while let Some(&(i, c)) = chars.peek() {
                    if c.is_ascii_digit() {
                        end = i + c.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let n: i64 = src[start..end].parse().unwrap();
                tokens.push(Token::Number(n));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = chars.peek().unwrap().0;
                let mut end = start;
                while let Some(&(i, c)) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        end = i + c.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let word = &src[start..end];
                let token = match word {
                    "const" => Token::Const,
                    _ => Token::Ident(word.to_string()),
                };
                tokens.push(token);
            }
            _ => panic!("Unexpected char: {c:?}"),
        }
    }

    tokens
}

#[test]
fn tokenize_eq_and_semicolon() {
    assert_eq!(tokenize("= ;"), vec![Token::Eq, Token::Semicolon]);
}

#[test]
fn tokenize_number() {
    assert_eq!(tokenize("123"), vec![Token::Number(123)]);
}

#[test]
fn tokenize_number_and_eq_and_semicolon() {
    assert_eq!(
        tokenize("5= 10;"),
        vec![
            Token::Number(5),
            Token::Eq,
            Token::Number(10),
            Token::Semicolon
        ]
    );
}

#[test]
fn tokenize_arrow() {
    assert_eq!(tokenize("=>"), vec![Token::Arrow]);
}

#[test]
fn tokenize_eq_then_gt() {
    assert_eq!(tokenize("= >"), vec![Token::Eq, Token::GreaterThan]);
}
