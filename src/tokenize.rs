#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    // Keywords
    Const,
    Return,

    // Type annotation keywords
    TypeNumber,
    TypeString,
    TypeBoolean,
    TypeArray,
    TypeVoid,
    TypeInt, // "Void" (JS版のコメントどおり Our Void type → TYPE_INT)
    TypeFloat,
    TypeBool,
    TypeUnit,

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
    Boolean(bool),
    Ident(String),
    Number(i64),
    StringLit(String),

    // EoF
    EoF,
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
                    "return" => Token::Return,
                    "true" => Token::Boolean(true),
                    "false" => Token::Boolean(false),
                    "number" => Token::TypeNumber,
                    "string" => Token::TypeString,
                    "boolean" => Token::TypeBoolean,
                    "Array" => Token::TypeArray,
                    "void" => Token::TypeVoid,
                    "Void" => Token::TypeInt,
                    "Float" => Token::TypeFloat,
                    "Bool" => Token::TypeBool,
                    "Unit" => Token::TypeUnit,

                    _ => Token::Ident(word.to_string()),
                };
                tokens.push(token);
            }
            '"' | '\'' => {
                let quote = c; // ダブルクォートかシングルクォートを覚える
                chars.next(); // クォートを消費
                let start = chars.peek().map(|&(i, _)| i).unwrap_or(src.len());
                let mut end = start;
                while let Some(&(i, ch)) = chars.peek() {
                    if ch == quote {
                        chars.next(); //クォートを消費
                        break;
                    }
                    if ch == '\\' {
                        // バックスラッシュの次は何がきても無条件で消費
                        chars.next();
                        if let Some(&(j, esc)) = chars.peek() {
                            end = j + esc.len_utf8();
                            chars.next();
                        }
                        continue;
                    }
                    end = i + ch.len_utf8();
                    chars.next();
                }
                tokens.push(Token::StringLit(src[start..end].to_string()));
            }
            '/' => {
                chars.next(); // 最初の'/'を消費
                match chars.peek() {
                    Some(&(_, '/')) => {
                        // 単行コメントは行末 or EoFまで読み飛ばす
                        chars.next();
                        while let Some(&(_, ch)) = chars.peek() {
                            chars.next();
                            if ch == '\n' {
                                break;
                            }
                        }
                    }
                    Some(&(_, '*')) => {
                        // 複数行コメントは'*/'まで読みとばす
                        chars.next();
                        while let Some(&(_, ch)) = chars.peek() {
                            chars.next();
                            if ch == '*' {
                                if let Some(&(_, '/')) = chars.peek() {
                                    chars.next();
                                    break;
                                }
                            }
                        }
                    }
                    _ => panic!("Unexpected chars: /"),
                }
            }
            _ => panic!("Unexpected char: {c:?}"),
        }
    }
    tokens.push(Token::EoF);
    tokens
}

#[test]
fn tokenize_eq_and_semicolon() {
    assert_eq!(
        tokenize("= ;"),
        vec![Token::Eq, Token::Semicolon, Token::EoF]
    );
}

#[test]
fn tokenize_number() {
    assert_eq!(tokenize("123"), vec![Token::Number(123), Token::EoF]);
}

#[test]
fn tokenize_number_and_eq_and_semicolon() {
    assert_eq!(
        tokenize("5= 10;"),
        vec![
            Token::Number(5),
            Token::Eq,
            Token::Number(10),
            Token::Semicolon,
            Token::EoF
        ]
    );
}

#[test]
fn tokenize_arrow() {
    assert_eq!(tokenize("=>"), vec![Token::Arrow, Token::EoF]);
}

#[test]
fn tokenize_eq_then_gt() {
    assert_eq!(
        tokenize("= >"),
        vec![Token::Eq, Token::GreaterThan, Token::EoF]
    );
}

#[test]
fn tokenize_return_and_bool() {
    assert_eq!(
        tokenize("return true false"),
        vec![
            Token::Return,
            Token::Boolean(true),
            Token::Boolean(false),
            Token::EoF
        ]
    );
}

#[test]
fn tokenize_type_keywords() {
    assert_eq!(
        tokenize("number string boolean Array void Void Float Bool Unit"),
        vec![
            Token::TypeNumber,
            Token::TypeString,
            Token::TypeBoolean,
            Token::TypeArray,
            Token::TypeVoid,
            Token::TypeInt,
            Token::TypeFloat,
            Token::TypeBool,
            Token::TypeUnit,
            Token::EoF,
        ]
    );
}

#[test]
fn tokenize_keyword_prefix_is_ident() {
    // "constant" は "const" ではなく Ident
    assert_eq!(
        tokenize("constant returns"),
        vec![
            Token::Ident("constant".to_string()),
            Token::Ident("returns".to_string()),
            Token::EoF,
        ]
    );
}

#[test]
fn tokenize_string_double() {
    assert_eq!(
        tokenize("\"hello\""),
        vec![Token::StringLit("hello".to_string()), Token::EoF]
    );
}

#[test]
fn tokenize_string_single() {
    assert_eq!(
        tokenize("'world'"),
        vec![Token::StringLit("world".to_string()), Token::EoF]
    );
}

#[test]
fn tokenize_string_with_escape() {
    assert_eq!(
        tokenize(r#""a\"b""#),
        vec![Token::StringLit(r#"a\"b"#.to_string()), Token::EoF]
    );
}

#[test]
fn tokenize_line_comment() {
    assert_eq!(
        tokenize("const // this is a comment\nx"),
        vec![Token::Const, Token::Ident("x".to_string()), Token::EoF]
    );
}

#[test]
fn tokenize_block_comment() {
    assert_eq!(
        tokenize("const /* hello */ x"),
        vec![Token::Const, Token::Ident("x".to_string()), Token::EoF]
    );
}

#[test]
fn tokenize_with_eof() {
    assert_eq!(tokenize(""), vec![Token::EoF]);
}
