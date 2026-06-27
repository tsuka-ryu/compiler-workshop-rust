//! 構文解析器 (パーサー)。parser 章 + errors 章。
//!
//! 手法は**再帰下降** + 式の **Pratt パーシング**。lexer がトークン化できる subset
//! (var/let/const, debugger, 数値/文字列/真偽/識別子, `+ - * / **`, `? :`, 括弧) を
//! AST (`super::ast`) に変換する。
//!
//! errors 章に従い、パース関数は `Result<T>` を返し `?` で伝播する。エラー型は
//! `SyntaxError`。章では `thiserror` / `miette` を勧めているが、依存を増やさず
//! `Display` + `std::error::Error` を手で実装する (本体に外部クレートを足さない方針)。

use std::fmt;

use super::ast::*;
use super::lexer::{Kind, Lexer, Token, TokenValue};

/// パース関数が返す結果型。errors 章の `pub type Result<T> = ...` に対応。
pub type Result<T> = std::result::Result<T, SyntaxError>;

/// 構文エラー。ECMAScript 仕様の「早期エラー」は構文エラーなので `SyntaxError`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxError {
    /// 特定のトークンを期待したが別のものが来た。
    UnexpectedToken {
        expected: Kind,
        found: Kind,
        offset: usize,
    },
    /// 式の先頭に来られないトークンが現れた。
    UnexpectedTokenInExpression { found: Kind, offset: usize },
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyntaxError::UnexpectedToken {
                expected,
                found,
                offset,
            } => write!(
                f,
                "expected {expected:?} but found {found:?} at offset {offset}"
            ),
            SyntaxError::UnexpectedTokenInExpression { found, offset } => {
                write!(f, "unexpected {found:?} in expression at offset {offset}")
            }
        }
    }
}

impl std::error::Error for SyntaxError {}

pub struct Parser<'a> {
    /// ソースコード
    source: &'a str,
    /// 字句解析器
    lexer: Lexer<'a>,
    /// レキサーから返された現在のトークン
    cur_token: Token,
    /// 前のトークンの終了オフセット (`finish_node` でノードの終端に使う)
    prev_token_end: usize,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut lexer = Lexer::new(source);
        // 最初のトークンを読み込んでおく (章の `cur_token: Token::default()` 相当を
        // Default を要求せずに済ませる)。
        let cur_token = lexer.next_token();
        Self {
            source,
            lexer,
            cur_token,
            prev_token_end: 0,
        }
    }

    /// プログラム全体をパースする。
    pub fn parse(&mut self) -> Result<Program> {
        let mut body = Vec::new();
        while !self.at(Kind::Eof) {
            body.push(self.parse_statement()?);
        }
        Ok(Program {
            node: Node::new(0, self.source.len()),
            body,
        })
    }

    // -- ヘルパー (parser 章) -------------------------------------------------

    /// 現在のトークンの開始位置でノードを開く (終端は後で `finish_node`)。
    fn start_node(&self) -> Node {
        Node::new(self.cur_token.start, 0)
    }

    /// 直前に消費したトークンの終端でノードを閉じる。
    fn finish_node(&self, node: Node) -> Node {
        Node::new(node.start, self.prev_token_end)
    }

    fn cur_kind(&self) -> Kind {
        self.cur_token.kind
    }

    /// 現在のトークンが `kind` かどうか。
    fn at(&self, kind: Kind) -> bool {
        self.cur_kind() == kind
    }

    /// 任意のトークンを 1 つ進める。
    fn bump_any(&mut self) {
        self.advance();
    }

    /// `kind` にいれば進めて true、いなければ false。
    fn eat(&mut self, kind: Kind) -> bool {
        if self.at(kind) {
            self.advance();
            return true;
        }
        false
    }

    /// 次のトークンへ進む。
    fn advance(&mut self) {
        let token = self.lexer.next_token();
        self.prev_token_end = self.cur_token.end;
        self.cur_token = token;
    }

    // -- エラー処理 (errors 章) -----------------------------------------------

    /// `kind` を期待する。違えばエラーを返す。
    fn expect(&mut self, kind: Kind) -> Result<()> {
        if !self.at(kind) {
            return Err(self.unexpected(kind));
        }
        self.advance();
        Ok(())
    }

    fn unexpected(&self, expected: Kind) -> SyntaxError {
        SyntaxError::UnexpectedToken {
            expected,
            found: self.cur_kind(),
            offset: self.cur_token.start,
        }
    }

    fn unexpected_in_expression(&self) -> SyntaxError {
        SyntaxError::UnexpectedTokenInExpression {
            found: self.cur_kind(),
            offset: self.cur_token.start,
        }
    }

    /// 現在のトークンの文字列値 (識別子名 / 文字列リテラルの中身)。
    fn cur_string_value(&self) -> String {
        match &self.cur_token.value {
            TokenValue::String(s) => s.clone(),
            _ => String::new(),
        }
    }

    /// 現在のトークンの数値。
    fn cur_number_value(&self) -> f64 {
        match &self.cur_token.value {
            TokenValue::Number(n) => *n,
            _ => f64::NAN,
        }
    }

    // -- 文 -------------------------------------------------------------------

    fn parse_statement(&mut self) -> Result<Statement> {
        match self.cur_kind() {
            Kind::Var | Kind::Let | Kind::Const => self.parse_variable_declaration(),
            Kind::Debugger => self.parse_debugger_statement(),
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_debugger_statement(&mut self) -> Result<Statement> {
        let node = self.start_node();
        self.expect(Kind::Debugger)?;
        self.eat(Kind::Semicolon);
        Ok(Statement::DebuggerStatement(Box::new(DebuggerStatement {
            node: self.finish_node(node),
        })))
    }

    fn parse_variable_declaration(&mut self) -> Result<Statement> {
        let node = self.start_node();
        let kind = match self.cur_kind() {
            Kind::Var => VariableDeclarationKind::Var,
            Kind::Let => VariableDeclarationKind::Let,
            Kind::Const => VariableDeclarationKind::Const,
            _ => unreachable!("parse_statement で var/let/const を確認済み"),
        };
        self.bump_any();
        // subset では宣言子は 1 つだけ (lexer に `,` トークンが無いため)。
        let declarations = vec![self.parse_variable_declarator()?];
        self.eat(Kind::Semicolon);
        Ok(Statement::VariableDeclaration(Box::new(
            VariableDeclaration {
                node: self.finish_node(node),
                kind,
                declarations,
            },
        )))
    }

    fn parse_variable_declarator(&mut self) -> Result<VariableDeclarator> {
        let node = self.start_node();
        let id = self.parse_binding_identifier()?;
        let init = if self.eat(Kind::Eq) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        Ok(VariableDeclarator {
            node: self.finish_node(node),
            id,
            init,
        })
    }

    fn parse_binding_identifier(&mut self) -> Result<BindingIdentifier> {
        let node = self.start_node();
        if !self.at(Kind::Identifier) {
            return Err(self.unexpected(Kind::Identifier));
        }
        let name = self.cur_string_value();
        self.bump_any();
        Ok(BindingIdentifier {
            node: self.finish_node(node),
            name,
        })
    }

    fn parse_expression_statement(&mut self) -> Result<Statement> {
        let node = self.start_node();
        let expression = self.parse_expression()?;
        self.eat(Kind::Semicolon);
        Ok(Statement::ExpressionStatement(Box::new(
            ExpressionStatement {
                node: self.finish_node(node),
                expression,
            },
        )))
    }

    // -- 式 (Pratt パーシング) ------------------------------------------------

    /// 式のエントリ。三項 `?:` は全二項より低優先かつ右結合なのでここで包む。
    fn parse_expression(&mut self) -> Result<Expression> {
        let node = self.start_node();
        let test = self.parse_binary_expression(0)?;
        if self.eat(Kind::Question) {
            let consequent = self.parse_expression()?;
            self.expect(Kind::Colon)?;
            let alternate = self.parse_expression()?; // 右結合
            return Ok(Expression::ConditionalExpression(Box::new(
                ConditionalExpression {
                    node: self.finish_node(node),
                    test,
                    consequent,
                    alternate,
                },
            )));
        }
        Ok(test)
    }

    /// 二項式を Pratt 法でパースする。`min_bp` 未満の優先度で止まる。
    fn parse_binary_expression(&mut self, min_bp: u8) -> Result<Expression> {
        let node = self.start_node();
        let mut left = self.parse_unary_expression()?;
        while let Some((l_bp, r_bp)) = binary_binding_power(self.cur_kind()) {
            if l_bp < min_bp {
                break;
            }
            let operator = binary_operator(self.cur_kind());
            self.bump_any();
            let right = self.parse_binary_expression(r_bp)?;
            left = Expression::BinaryExpression(Box::new(BinaryExpression {
                node: self.finish_node(node),
                left,
                operator,
                right,
            }));
        }
        Ok(left)
    }

    /// 前置単項 `-` / `+`。なければ primary へ。
    fn parse_unary_expression(&mut self) -> Result<Expression> {
        let operator = match self.cur_kind() {
            Kind::Minus => UnaryOperator::Minus,
            Kind::Plus => UnaryOperator::Plus,
            _ => return self.parse_primary_expression(),
        };
        let node = self.start_node();
        self.bump_any();
        let argument = self.parse_unary_expression()?; // 前置は右結合
        Ok(Expression::UnaryExpression(Box::new(UnaryExpression {
            node: self.finish_node(node),
            operator,
            argument,
        })))
    }

    /// 一次式: リテラル / 識別子 / 括弧。
    fn parse_primary_expression(&mut self) -> Result<Expression> {
        let node = self.start_node();
        match self.cur_kind() {
            Kind::Number => {
                let value = self.cur_number_value();
                self.bump_any();
                Ok(Expression::NumberLiteral(Box::new(NumberLiteral {
                    node: self.finish_node(node),
                    value,
                })))
            }
            Kind::String => {
                let value = self.cur_string_value();
                self.bump_any();
                Ok(Expression::StringLiteral(Box::new(StringLiteral {
                    node: self.finish_node(node),
                    value,
                })))
            }
            Kind::True | Kind::False => {
                let value = self.at(Kind::True);
                self.bump_any();
                Ok(Expression::BooleanLiteral(Box::new(BooleanLiteral {
                    node: self.finish_node(node),
                    value,
                })))
            }
            Kind::Identifier => {
                let name = self.cur_string_value();
                self.bump_any();
                Ok(Expression::Identifier(Box::new(IdentifierReference {
                    node: self.finish_node(node),
                    name,
                })))
            }
            // 括弧式は中身の式をそのまま返す (subset では ParenExpression ノードを作らない)。
            Kind::LParen => {
                self.bump_any();
                let expression = self.parse_expression()?;
                self.expect(Kind::RParen)?;
                Ok(expression)
            }
            _ => Err(self.unexpected_in_expression()),
        }
    }
}

/// 二項演算子の束縛力 (左, 右)。`l < r` なら左結合、`l > r` なら右結合。
/// 数値が大きいほど強く結合する。
fn binary_binding_power(kind: Kind) -> Option<(u8, u8)> {
    match kind {
        Kind::Plus | Kind::Minus => Some((1, 2)),    // 加減 (左結合)
        Kind::Star | Kind::Slash => Some((3, 4)),    // 乗除 (左結合)
        Kind::StarStar => Some((6, 5)),              // 冪 (右結合)
        _ => None,
    }
}

fn binary_operator(kind: Kind) -> BinaryOperator {
    match kind {
        Kind::Plus => BinaryOperator::Addition,
        Kind::Minus => BinaryOperator::Subtraction,
        Kind::Star => BinaryOperator::Multiplication,
        Kind::Slash => BinaryOperator::Division,
        Kind::StarStar => BinaryOperator::Exponentiation,
        _ => unreachable!("binary_binding_power が None を返した kind は来ない"),
    }
}

/// ソースをパースして `Program` を返す簡易エントリ。
pub fn parse(source: &str) -> Result<Program> {
    Parser::new(source).parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(source: &str) -> Program {
        parse(source).expect("should parse")
    }

    fn first_expression(source: &str) -> Expression {
        match program(source).body.into_iter().next().unwrap() {
            Statement::ExpressionStatement(stmt) => stmt.expression,
            other => panic!("expected expression statement, got {other:?}"),
        }
    }

    #[test]
    fn empty_program() {
        assert_eq!(program("").body.len(), 0);
    }

    #[test]
    fn debugger_statement() {
        let body = program("debugger;").body;
        assert!(matches!(body[0], Statement::DebuggerStatement(_)));
    }

    #[test]
    fn var_declaration_without_init() {
        // ast 章の `var a` の例。
        let Statement::VariableDeclaration(decl) = &program("var a").body[0] else {
            panic!("expected variable declaration");
        };
        assert_eq!(decl.kind, VariableDeclarationKind::Var);
        assert_eq!(decl.declarations[0].id.name, "a");
        assert!(decl.declarations[0].init.is_none());
        // スパンは `var a` の 0..5。
        assert_eq!((decl.node.start, decl.node.end), (0, 5));
    }

    #[test]
    fn const_declaration_with_init() {
        let Statement::VariableDeclaration(decl) = &program("const x = 1;").body[0] else {
            panic!("expected variable declaration");
        };
        assert_eq!(decl.kind, VariableDeclarationKind::Const);
        assert!(matches!(
            decl.declarations[0].init,
            Some(Expression::NumberLiteral(_))
        ));
    }

    #[test]
    fn precedence_mul_over_add() {
        // 1 + 2 * 3  =>  1 + (2 * 3)
        let Expression::BinaryExpression(add) = first_expression("1 + 2 * 3") else {
            panic!("expected binary expression");
        };
        assert_eq!(add.operator, BinaryOperator::Addition);
        assert!(matches!(add.left, Expression::NumberLiteral(_)));
        let Expression::BinaryExpression(mul) = add.right else {
            panic!("right side should be a multiplication");
        };
        assert_eq!(mul.operator, BinaryOperator::Multiplication);
    }

    #[test]
    fn exponentiation_is_right_associative() {
        // 2 ** 3 ** 2  =>  2 ** (3 ** 2)
        let Expression::BinaryExpression(outer) = first_expression("2 ** 3 ** 2") else {
            panic!("expected binary expression");
        };
        assert_eq!(outer.operator, BinaryOperator::Exponentiation);
        assert!(matches!(outer.left, Expression::NumberLiteral(_)));
        assert!(matches!(outer.right, Expression::BinaryExpression(_)));
    }

    #[test]
    fn parentheses_override_precedence() {
        // (1 + 2) * 3  =>  (1 + 2) * 3
        let Expression::BinaryExpression(mul) = first_expression("(1 + 2) * 3") else {
            panic!("expected binary expression");
        };
        assert_eq!(mul.operator, BinaryOperator::Multiplication);
        let Expression::BinaryExpression(add) = mul.left else {
            panic!("left side should be an addition");
        };
        assert_eq!(add.operator, BinaryOperator::Addition);
    }

    #[test]
    fn unary_minus() {
        let Expression::UnaryExpression(unary) = first_expression("-x") else {
            panic!("expected unary expression");
        };
        assert_eq!(unary.operator, UnaryOperator::Minus);
        assert!(matches!(unary.argument, Expression::Identifier(_)));
    }

    #[test]
    fn conditional_is_lower_than_binary() {
        // a + 1 ? b : c  =>  (a + 1) ? b : c
        let Expression::ConditionalExpression(cond) = first_expression("a + 1 ? b : c") else {
            panic!("expected conditional expression");
        };
        assert!(matches!(cond.test, Expression::BinaryExpression(_)));
        assert!(matches!(cond.consequent, Expression::Identifier(_)));
        assert!(matches!(cond.alternate, Expression::Identifier(_)));
    }

    #[test]
    fn boolean_and_string_literals() {
        assert!(matches!(
            first_expression("true"),
            Expression::BooleanLiteral(_)
        ));
        assert!(matches!(
            first_expression("\"hi\""),
            Expression::StringLiteral(_)
        ));
    }

    #[test]
    fn error_on_missing_rparen() {
        let err = parse("(1 + 2").unwrap_err();
        assert!(matches!(
            err,
            SyntaxError::UnexpectedToken {
                expected: Kind::RParen,
                ..
            }
        ));
    }

    #[test]
    fn error_on_missing_binding_identifier() {
        let err = parse("var 1").unwrap_err();
        assert!(matches!(
            err,
            SyntaxError::UnexpectedToken {
                expected: Kind::Identifier,
                ..
            }
        ));
    }
}
