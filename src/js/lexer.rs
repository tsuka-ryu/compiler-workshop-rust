//! 字句解析器 (lexer)。元チュートリアル lexer 章 (トークン / peek / JavaScript / トークンの値) を
//! subset 向けに実装したもの。
//!
//! toy 版 (`crate::tokenize`) は `Vec<char>` + index 方式だったが、こちらは
//! チュートリアル忠実に `Chars` イテレータ + `offset()` 方式で書く。
//!
//! 実装済み: whitespace/コメントのスキップ、識別子とキーワード (unicode-id-start)、
//! 数値・文字列リテラルの値抽出 (`TokenValue`)、subset で使う演算子・記号。
//!
//! 未対応 (後の節): 文字列のエスケープ / 数値の進数 (2,8,16) / エラー処理 (未終端等)。

use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// トークンの型
    pub kind: Kind,
    /// ソースにおけるオフセットの開始位置 (UTF-8 バイト)
    pub start: usize,
    /// ソースにおけるオフセットの終了位置 (UTF-8 バイト)
    pub end: usize,
    /// 識別子名 / 数値 / 文字列の中身。それ以外は `None`。
    pub value: TokenValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenValue {
    None,
    Number(f64),
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// ファイルの終端
    Eof,

    // --- リテラル / 識別子 ---
    Identifier,
    Number,
    String,

    // --- キーワード ---
    Var,
    Let,
    Const,
    Debugger,
    True,
    False,

    // --- 演算子 ---
    Plus,     // +
    Minus,    // -
    Star,     // *
    Slash,    // /
    StarStar, // **

    // --- 記号 ---
    LParen,    // (
    RParen,    // )
    LCurly,    // {
    RCurly,    // }
    Semicolon, // ;
    Eq,        // =
    Question,  // ?
    Colon,     // :
}

pub struct Lexer<'a> {
    /// ソースのテキスト
    source: &'a str,
    /// 残りの文字
    chars: Chars<'a>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.chars(),
        }
    }

    /// 次のトークンを 1 つ読む。最後は必ず `Kind::Eof`。
    pub fn next_token(&mut self) -> Token {
        self.skip_trivia();
        let start = self.offset();
        let kind = self.read_next_kind();
        let end = self.offset();
        let value = self.read_value(kind, start, end);
        Token { kind, start, end, value }
    }

    /// whitespace と コメント (`//`, `/* */`) を読み飛ばす。
    fn skip_trivia(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\r' | '\n' => {
                    self.chars.next();
                }
                '/' => match self.peek2() {
                    // 行コメント: 改行まで
                    Some('/') => {
                        self.chars.next();
                        self.chars.next();
                        while let Some(c) = self.peek() {
                            if c == '\n' {
                                break;
                            }
                            self.chars.next();
                        }
                    }
                    // ブロックコメント: `*/` まで
                    Some('*') => {
                        self.chars.next();
                        self.chars.next();
                        while let Some(c) = self.chars.next() {
                            if c == '*' && self.peek() == Some('/') {
                                self.chars.next();
                                break;
                            }
                        }
                    }
                    // `/` 単体は除算演算子なので trivia ではない
                    _ => break,
                },
                _ => break,
            }
        }
    }

    fn read_next_kind(&mut self) -> Kind {
        // 識別子/数値の slice を取るため、消費前の位置を控える
        let start = self.offset();
        let Some(c) = self.chars.next() else {
            return Kind::Eof;
        };
        match c {
            '+' => Kind::Plus,
            '-' => Kind::Minus,
            '*' => {
                if self.peek() == Some('*') {
                    self.chars.next();
                    Kind::StarStar
                } else {
                    Kind::Star
                }
            }
            '/' => Kind::Slash, // コメントは skip_trivia で処理済み
            '(' => Kind::LParen,
            ')' => Kind::RParen,
            '{' => Kind::LCurly,
            '}' => Kind::RCurly,
            ';' => Kind::Semicolon,
            '=' => Kind::Eq,
            '?' => Kind::Question,
            ':' => Kind::Colon,
            '"' | '\'' => self.read_string(c),
            c if c.is_ascii_digit() => self.read_number(),
            c if Self::is_id_start(c) => {
                while let Some(n) = self.peek() {
                    if Self::is_id_continue(n) {
                        self.chars.next();
                    } else {
                        break;
                    }
                }
                Self::match_keyword(&self.source[start..self.offset()])
            }
            // 未知の文字。エラー処理は後の節。今は読み飛ばして次へ。
            _ => self.read_next_kind(),
        }
    }

    /// 開きクォート消費済み。閉じクォートまで読む。
    fn read_string(&mut self, quote: char) -> Kind {
        while let Some(c) = self.chars.next() {
            if c == quote {
                break;
            }
        }
        Kind::String
    }

    /// 先頭の数字は消費済み。残りの数字と小数部を読む。
    fn read_number(&mut self) -> Kind {
        self.eat_digits();
        if self.peek() == Some('.') {
            self.chars.next();
            self.eat_digits();
        }
        Kind::Number
    }

    fn eat_digits(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.chars.next();
            } else {
                break;
            }
        }
    }

    /// 識別子文字列を見てキーワードか単なる識別子かを振り分ける。
    fn match_keyword(ident: &str) -> Kind {
        // キーワードは 3..=8 文字。範囲外は文字列比較を省いて即 Identifier。
        if ident.len() < 3 || ident.len() > 8 {
            return Kind::Identifier;
        }
        match ident {
            "var" => Kind::Var,
            "let" => Kind::Let,
            "const" => Kind::Const,
            "debugger" => Kind::Debugger,
            "true" => Kind::True,
            "false" => Kind::False,
            _ => Kind::Identifier,
        }
    }

    /// トークンの種別と範囲から `TokenValue` を取り出す。
    fn read_value(&self, kind: Kind, start: usize, end: usize) -> TokenValue {
        match kind {
            Kind::Number => {
                let n = self.source[start..end].parse().unwrap_or(f64::NAN);
                TokenValue::Number(n)
            }
            Kind::String => {
                // 前後のクォートを除く (未終端でも panic しない)
                let raw = &self.source[start..end];
                let inner = raw
                    .strip_prefix(&['"', '\''][..])
                    .unwrap_or(raw)
                    .strip_suffix(&['"', '\''][..])
                    .unwrap_or(raw);
                TokenValue::String(inner.to_string())
            }
            Kind::Identifier => TokenValue::String(self.source[start..end].to_string()),
            _ => TokenValue::None,
        }
    }

    fn is_id_start(c: char) -> bool {
        c == '$' || c == '_' || unicode_id_start::is_id_start(c)
    }

    fn is_id_continue(c: char) -> bool {
        c == '$' || c == '_' || unicode_id_start::is_id_continue(c)
    }

    /// 次の 1 文字を、イテレータを進めずに覗き見る。
    /// `chars` をクローンして 1 歩進めるだけ (clone は index コピーのみで安い)。
    fn peek(&self) -> Option<char> {
        self.chars.clone().next()
    }

    /// 次の次の 1 文字を覗き見る (`//` や `/*` の判定用)。
    fn peek2(&self) -> Option<char> {
        let mut it = self.chars.clone();
        it.next();
        it.next()
    }

    /// ソース先頭からの現在位置を UTF-8 バイトで取得する。
    /// `as_str().len()` はスライスのメタデータ参照なので O(1)。
    fn offset(&self) -> usize {
        self.source.len() - self.chars.as_str().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(source: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token();
            let is_eof = token.kind == Kind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        tokens
    }

    fn kinds(source: &str) -> Vec<Kind> {
        tokenize(source).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn empty_is_just_eof() {
        assert_eq!(kinds(""), vec![Kind::Eof]);
    }

    #[test]
    fn operators_and_punctuation() {
        assert_eq!(
            kinds("+ - * / ** ( ) { } ; = ? :"),
            vec![
                Kind::Plus, Kind::Minus, Kind::Star, Kind::Slash, Kind::StarStar,
                Kind::LParen, Kind::RParen, Kind::LCurly, Kind::RCurly,
                Kind::Semicolon, Kind::Eq, Kind::Question, Kind::Colon,
                Kind::Eof,
            ]
        );
    }

    #[test]
    fn star_star_vs_star() {
        assert_eq!(kinds("2 ** 3"), vec![Kind::Number, Kind::StarStar, Kind::Number, Kind::Eof]);
        assert_eq!(kinds("2 * 3"), vec![Kind::Number, Kind::Star, Kind::Number, Kind::Eof]);
    }

    #[test]
    fn keywords_vs_identifier() {
        assert_eq!(
            kinds("var let const debugger true false foo"),
            vec![
                Kind::Var, Kind::Let, Kind::Const, Kind::Debugger,
                Kind::True, Kind::False, Kind::Identifier, Kind::Eof,
            ]
        );
        // undefined はキーワードではない
        assert_eq!(kinds("undefined"), vec![Kind::Identifier, Kind::Eof]);
    }

    #[test]
    fn identifier_value() {
        let toks = tokenize("foo");
        assert_eq!(toks[0].kind, Kind::Identifier);
        assert_eq!(toks[0].value, TokenValue::String("foo".to_string()));
    }

    #[test]
    fn unicode_identifier() {
        // ID_Start を持つ文字は識別子になる
        assert_eq!(kinds("ಠ_ಠ"), vec![Kind::Identifier, Kind::Eof]);
        // 絵文字は ID_Start を持たないので読み飛ばされて Eof のみ
        assert_eq!(kinds("🦀"), vec![Kind::Eof]);
    }

    #[test]
    fn number_value() {
        assert_eq!(tokenize("1")[0].value, TokenValue::Number(1.0));
        assert_eq!(tokenize("1.5")[0].value, TokenValue::Number(1.5));
    }

    #[test]
    fn string_value() {
        assert_eq!(tokenize("\"hi\"")[0].value, TokenValue::String("hi".to_string()));
        assert_eq!(tokenize("'yo'")[0].value, TokenValue::String("yo".to_string()));
    }

    #[test]
    fn skips_whitespace_and_comments() {
        assert_eq!(kinds("a // line comment\n b"), vec![Kind::Identifier, Kind::Identifier, Kind::Eof]);
        assert_eq!(kinds("a /* block\n comment */ b"), vec![Kind::Identifier, Kind::Identifier, Kind::Eof]);
        // 単体の / は除算
        assert_eq!(kinds("a / b"), vec![Kind::Identifier, Kind::Slash, Kind::Identifier, Kind::Eof]);
    }

    #[test]
    fn small_program() {
        assert_eq!(
            kinds("const x = 1 + 2 * 3;"),
            vec![
                Kind::Const, Kind::Identifier, Kind::Eq,
                Kind::Number, Kind::Plus, Kind::Number, Kind::Star, Kind::Number,
                Kind::Semicolon, Kind::Eof,
            ]
        );
    }

    #[test]
    fn spans_are_tracked() {
        // "const" は 0..5
        let toks = tokenize("const x");
        assert_eq!((toks[0].start, toks[0].end), (0, 5));
        assert_eq!((toks[1].start, toks[1].end), (6, 7));
    }
}