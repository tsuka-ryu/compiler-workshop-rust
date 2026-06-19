use crate::ast_resilient::{BinaryOp, Dummy, Expression, Parameter, Statement, TypeAnnotation};
use crate::tokenize_span::{Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

/// oxc の FatalError 相当。errors_len = fatal が立った時点の errors の長さ。
/// 巻き戻し中に積まれた recoverable error を後で truncate するため。
struct FatalError {
    error: ParseError,
    errors_len: usize,
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<ParseError>,         // recoverable
    fatal_error: Option<FatalError>, // 巻き戻し用
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
        matches!(self.peek().kind, TokenKind::EoF)
    }

    fn at(&self, kind: &TokenKind) -> bool {
        &self.peek().kind == kind
    }

    /// 合致してたら食って true。そうでなければ false。
    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// 合致してたら食う（なくても何もしない）
    fn bump(&mut self, kind: &TokenKind) {
        if self.at(kind) {
            self.advance();
        }
    }

    fn expect(&mut self, kind: &TokenKind) {
        if self.at(kind) {
            self.advance();
        } else {
            let tok = self.peek();
            self.set_fatal_error(ParseError {
                message: format!("expected {:?}, get {:?}", kind, tok.kind),
                span: tok.span,
            });
        }
    }

    fn advance_to_end(&mut self) {
        todo!()
    }

    fn error(&mut self, error: ParseError) {
        self.errors.push(error)
    }

    fn errors_count(&self) -> usize {
        self.errors.len()
    }

    /// fatal をセットして lexer を末尾へ。1個目の fatal だけ記録する
    fn set_fatal_error(&mut self, error: ParseError) {
        if self.fatal_error.is_none() {
            self.advance_to_end();
            self.fatal_error = Some(FatalError {
                error,
                errors_len: self.errors_count(),
            })
        }
    }

    /// fatal をセットしてダミーノードを返す
    fn fatal_error<T: Dummy>(&mut self, error: ParseError) -> T {
        let span = self.peek().span;
        self.set_fatal_error(error);
        T::dummy(span)
    }

    fn set_unexpected(&mut self) {
        let tok = self.peek();
        let error = ParseError {
            message: format!("unexpected token: {:?}", tok.kind),
            span: tok.span,
        };
        self.set_fatal_error(error);
    }

    fn unexpected<T: Dummy>(&mut self) -> T {
        let span = self.peek().span;
        self.set_unexpected();
        T::dummy(span)
    }

    fn has_fatal_error(&self) -> bool {
        matches!(self.peek().kind, TokenKind::EoF) || self.fatal_error.is_some()
    }

    fn looks_like_arrow_fn(&self) -> bool {
        let mut depth = 1;
        let mut i = self.pos;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        let mut j = i + 1;
                        if matches!(self.tokens.get(j).map(|t| &t.kind), Some(TokenKind::Colon)) {
                            j += 1;
                            if self.tokens.get(j).is_some_and(|t| {
                                matches!(
                                    t.kind,
                                    TokenKind::TypeNumber
                                        | TokenKind::TypeString
                                        | TokenKind::TypeBoolean
                                        | TokenKind::TypeVoid
                                        | TokenKind::TypeInt
                                        | TokenKind::TypeFloat
                                        | TokenKind::TypeBool
                                        | TokenKind::TypeUnit
                                        | TokenKind::TypeArray
                                        | TokenKind::Ident(_)
                                )
                            }) {
                                j += 1;
                            }
                        }
                        return self
                            .tokens
                            .get(j)
                            .is_some_and(|t| matches!(t.kind, TokenKind::Arrow));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn parse_statement(&mut self) -> Statement {
        match self.peek().kind {
            TokenKind::Const => self.parse_const_declaration(),
            TokenKind::Return => self.parse_return_statement(),
            _ => panic!("Unexpected token: {:?}", self.peek()),
        }
    }
    fn parse_const_declaration(&mut self) -> Statement {
        let start_span = self.advance().span;

        let ident_tok = self.advance();
        let name = match ident_tok.kind {
            TokenKind::Ident(s) => s,
            other => panic!("Expected identifier, got {other:?}"),
        };

        // : 型があれば読む
        let type_annotation = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance(); // : を消費
            Some(self.parse_type_annotation())
        } else {
            None
        };

        // =
        let eq_tok = self.advance();
        match eq_tok.kind {
            TokenKind::Eq => {}
            other => panic!("Expected '=', got {other:?}"),
        }

        // 初期化式
        let init = self.parse_expression();

        // 末尾の;があれば消費
        let end_span = if matches!(self.peek().kind, TokenKind::Semicolon) {
            self.advance().span
        } else {
            init.span()
        };

        Statement::ConstDeclaration {
            name,
            type_annotation,
            init,
            span: start_span.merge(end_span),
        }
    }

    fn parse_return_statement(&mut self) -> Statement {
        let start_span = self.advance().span; // return

        let argument = if matches!(
            self.peek().kind,
            TokenKind::Semicolon | TokenKind::RCurly | TokenKind::EoF
        ) {
            None
        } else {
            Some(self.parse_expression())
        };

        let end_span = if matches!(self.peek().kind, TokenKind::Semicolon) {
            self.advance().span
        } else if let Some(arg) = &argument {
            arg.span()
        } else {
            start_span
        };

        Statement::Return {
            argument,
            span: start_span.merge(end_span),
        }
    }

    fn parse_expression(&mut self) -> Expression {
        let test = self.parse_binary();

        if self.eat(&TokenKind::Ternary) {
            let consequent = self.parse_expression();
            self.expect(&TokenKind::Colon);
            let alternate = self.parse_expression();
            let span = test.span().merge(alternate.span());
            Expression::Conditional {
                test: Box::new(test),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
                span,
            }
        } else {
            test
        }
    }

    fn parse_binary(&mut self) -> Expression {
        let mut left = self.parse_primary();

        loop {
            let op = match &self.peek().kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Multiply => BinaryOp::Multiply,
                _ => break,
            };
            self.advance();
            let right = self.parse_primary();
            let span = left.span().merge(right.span());
            left = Expression::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }

        left
    }

    fn parse_primary(&mut self) -> Expression {
        let tok = self.advance();
        let tok_span = tok.span;
        let mut expr = match tok.kind {
            TokenKind::Number(n) => Expression::Number {
                value: n,
                span: tok_span,
            },
            TokenKind::StringLit(s) => Expression::String {
                value: s,
                span: tok_span,
            },
            TokenKind::Boolean(b) => Expression::Boolean {
                value: b,
                span: tok_span,
            },
            TokenKind::Ident(name) => Expression::Identifier {
                name,
                span: tok_span,
            },
            TokenKind::LBracket => self.parse_array(tok_span),
            TokenKind::LParen => {
                if self.looks_like_arrow_fn() {
                    self.parse_arrow_function(tok_span)
                } else {
                    let inner = self.parse_expression();
                    self.expect(&TokenKind::RParen);
                    inner
                }
            }
            other => {
                return self.fatal_error(ParseError {
                    message: format!("unexpected token in expression: {other:?}"),
                    span: tok_span,
                });
            }
        };

        // 後置: ( で呼び出し [ でメンバアクセス
        loop {
            match &self.peek().kind {
                TokenKind::LParen => {
                    expr = self.parse_call(expr);
                }
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expression();
                    let close_span = self.peek().span;
                    self.expect(&TokenKind::RBracket);
                    let span = expr.span().merge(close_span);
                    expr = Expression::Member {
                        object: Box::new(expr),
                        index: Box::new(index),
                        span,
                    };
                }
                _ => break,
            }
        }

        expr
    }

    fn parse_call(&mut self, callee: Expression) -> Expression {
        self.advance(); // (

        let mut arguments = Vec::new();

        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                arguments.push(self.parse_expression());
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let close_span = self.peek().span;
        self.expect(&TokenKind::RParen);
        let span = callee.span().merge(close_span);
        Expression::Call {
            callee: Box::new(callee),
            arguments,
            span,
        }
    }

    fn parse_array(&mut self, lbracket_span: Span) -> Expression {
        let mut elements = Vec::new();

        if !matches!(self.peek().kind, TokenKind::RBracket) {
            loop {
                elements.push(self.parse_expression());
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let close_span = self.peek().span;
        self.expect(&TokenKind::RBracket);
        Expression::Array {
            elements,
            span: lbracket_span.merge(close_span),
        }
    }

    fn parse_arrow_function(&mut self, lparen_span: Span) -> Expression {
        let mut params = Vec::new();

        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                let name_tok = self.advance();
                let name_span = name_tok.span;
                let name = match name_tok.kind {
                    TokenKind::Ident(s) => s,
                    other => panic!("Expected param name, got {other:?}"),
                };

                let (type_annotation, param_end_span) =
                    if matches!(self.peek().kind, TokenKind::Colon) {
                        self.advance();
                        let ty = self.parse_type_annotation();
                        let ty_span = ty.span();
                        (Some(ty), ty_span)
                    } else {
                        (None, name_span)
                    };

                params.push(Parameter {
                    name,
                    type_annotation,
                    span: name_span.merge(param_end_span),
                });

                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let rparen = self.advance();
        match rparen.kind {
            TokenKind::RParen => {}
            other => panic!("Expected ')', got {other:?}"),
        }

        let return_type = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_annotation())
        } else {
            None
        };

        let arrow_tok = self.advance();
        match arrow_tok.kind {
            TokenKind::Arrow => {}
            other => panic!("Expected '=>', got {other:?}"),
        }

        let lcurly = self.advance();
        match lcurly.kind {
            TokenKind::LCurly => {}
            other => panic!("Expected '{{', got {other:?}"),
        }

        let mut body = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RCurly) {
            body.push(self.parse_statement());
        }

        let rcurly = self.advance(); // }

        Expression::ArrowFunction {
            params,
            return_type,
            body,
            span: lparen_span.merge(rcurly.span),
        }
    }

    fn parse_type_annotation(&mut self) -> TypeAnnotation {
        let tok = self.advance();
        let start_span = tok.span;
        let base = match tok.kind {
            TokenKind::TypeNumber => TypeAnnotation::Named {
                name: "number".to_string(),
                span: start_span,
            },
            TokenKind::TypeString => TypeAnnotation::Named {
                name: "string".to_string(),
                span: start_span,
            },
            TokenKind::TypeBoolean => TypeAnnotation::Named {
                name: "boolean".to_string(),
                span: start_span,
            },
            TokenKind::TypeVoid => TypeAnnotation::Named {
                name: "void".to_string(),
                span: start_span,
            },
            TokenKind::TypeInt => TypeAnnotation::Named {
                name: "Void".to_string(),
                span: start_span,
            },
            TokenKind::TypeFloat => TypeAnnotation::Named {
                name: "Float".to_string(),
                span: start_span,
            },
            TokenKind::TypeBool => TypeAnnotation::Named {
                name: "Bool".to_string(),
                span: start_span,
            },
            TokenKind::TypeUnit => TypeAnnotation::Named {
                name: "Unit".to_string(),
                span: start_span,
            },
            TokenKind::TypeArray => {
                if matches!(self.peek().kind, TokenKind::LessThan) {
                    self.advance(); // <
                    let elem = self.parse_type_annotation();
                    let gt = self.advance();
                    match gt.kind {
                        TokenKind::GreaterThan => {}
                        other => panic!("Expected '>', got {other:?}"),
                    }
                    return TypeAnnotation::Array {
                        element: Box::new(elem),
                        span: start_span.merge(gt.span),
                    };
                }
                TypeAnnotation::Named {
                    name: "Array".to_string(),
                    span: start_span,
                }
            }
            TokenKind::Ident(name) => TypeAnnotation::Named {
                name,
                span: start_span,
            },
            other => panic!("Expected type annotation, got {other:?}"),
        };

        // 後置 T[]
        if matches!(self.peek().kind, TokenKind::LBracket) {
            self.advance();
            let rbracket = self.advance();
            match rbracket.kind {
                TokenKind::RBracket => {}
                other => panic!("Expected ']' in array type, got {other:?}"),
            }
            let base_span = base.span();
            return TypeAnnotation::Array {
                element: Box::new(base),
                span: base_span.merge(rbracket.span),
            };
        }

        base
    }
}

pub fn parse_resilient(tokens: Vec<Token>) -> Vec<Statement> {
    let mut parser = Parser {
        tokens,
        pos: 0,
        errors: Vec::new(),
        fatal_error: None,
    };
    let mut statements = Vec::new();
    while !parser.at_end() {
        statements.push(parser.parse_statement());
    }
    statements
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenize_span::tokenize_span;

    #[test]
    fn const_decl_span_covers_whole_statement() {
        // "const x = 5;"
        //  0          12
        let stmts = parse_resilient(tokenize_span("const x = 5;"));
        assert_eq!(stmts[0].span(), Span { start: 0, end: 12 });
    }

    #[test]
    fn binary_span_covers_both_operands() {
        // "const x = 1 + 2;"
        //           10  14
        let stmts = parse_resilient(tokenize_span("const x = 1 + 2;"));
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            assert_eq!(init.span(), Span { start: 10, end: 15 });
        } else {
            panic!("expected ConstDeclaration");
        }
    }
}
