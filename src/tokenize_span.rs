#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
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

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub fn tokenize_span(src: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = src.char_indices().peekable();

    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }

        match c {
            '=' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                if let Some(&(_, '>')) = chars.peek() {
                    chars.next();
                    let end = start + 2;
                    tokens.push(Token {
                        kind: TokenKind::Arrow,
                        span: Span { start, end },
                    })
                } else {
                    let end = start + 1;
                    tokens.push(Token {
                        kind: TokenKind::Eq,
                        span: Span { start, end },
                    })
                }
            }
            ';' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                let end = start + 1;
                tokens.push(Token {
                    kind: TokenKind::Semicolon,
                    span: Span { start, end },
                });
            }
            '?' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::Ternary,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            ':' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::Colon,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            '|' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::Pipe,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            '<' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::LessThan,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            '>' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::GreaterThan,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            '*' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::Multiply,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            '+' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::Plus,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            ',' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            '(' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::LParen,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            ')' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::RParen,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            '{' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::LCurly,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            '}' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::RCurly,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            '[' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::LBracket,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            ']' => {
                let start = chars.peek().unwrap().0;
                chars.next();
                tokens.push(Token {
                    kind: TokenKind::RBracket,
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
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
                tokens.push(Token {
                    kind: TokenKind::Number(n),
                    span: Span { start, end },
                });
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
                let kind = match word {
                    "const" => TokenKind::Const,
                    "return" => TokenKind::Return,
                    "true" => TokenKind::Boolean(true),
                    "false" => TokenKind::Boolean(false),
                    "number" => TokenKind::TypeNumber,
                    "string" => TokenKind::TypeString,
                    "boolean" => TokenKind::TypeBoolean,
                    "Array" => TokenKind::TypeArray,
                    "void" => TokenKind::TypeVoid,
                    "Void" => TokenKind::TypeInt,
                    "Float" => TokenKind::TypeFloat,
                    "Bool" => TokenKind::TypeBool,
                    "Unit" => TokenKind::TypeUnit,

                    _ => TokenKind::Ident(word.to_string()),
                };
                tokens.push(Token {
                    kind,
                    span: Span { start, end },
                });
            }
            '"' | '\'' => {
                let quote_start = chars.peek().unwrap().0;
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
                let quote_end = chars.peek().map(|&(i, _)| i).unwrap_or(src.len());
                tokens.push(Token {
                    kind: TokenKind::StringLit(src[start..end].to_string()),
                    span: Span {
                        start: quote_start,
                        end: quote_end,
                    },
                });
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
    tokens.push(Token {
        kind: TokenKind::EoF,
        span: Span {
            start: src.len(),
            end: src.len(),
        },
    });
    tokens
}
