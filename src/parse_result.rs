use crate::ast_span::{BinaryOp, Expression, Parameter, Statement, TypeAnnotation};
use crate::tokenize_span::{Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

type ParseResult<T> = Result<T, ParseError>;

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
        matches!(self.peek().kind, TokenKind::EoF)
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

    fn parse_statement(&mut self) -> ParseResult<Statement> {
        match self.peek().kind {
            TokenKind::Const => self.parse_const_declaration(),
            TokenKind::Return => self.parse_return_statement(),
            _ => {
                let tok = self.peek();
                Err(ParseError {
                    message: format!("Unexpected token: {:?}", tok.kind),
                    span: tok.span,
                })
            }
        }
    }
    fn parse_const_declaration(&mut self) -> ParseResult<Statement> {
        let start_span = self.advance().span;

        let ident_tok = self.advance();
        let ident_span = ident_tok.span;
        let name = match ident_tok.kind {
            TokenKind::Ident(s) => s,
            other => {
                return Err(ParseError {
                    message: format!("Expected identifier, got {other:?}"),
                    span: ident_span,
                });
            }
        };

        // : 型があれば読む
        let type_annotation = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance(); // : を消費
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        // =
        let eq_tok = self.advance();
        match eq_tok.kind {
            TokenKind::Eq => {}
            other => {
                return Err(ParseError {
                    message: format!("Expected '=', got {other:?}"),
                    span: eq_tok.span,
                });
            }
        }

        // 初期化式
        let init = self.parse_expression()?;

        // 末尾の;があれば消費
        let end_span = if matches!(self.peek().kind, TokenKind::Semicolon) {
            self.advance().span
        } else {
            init.span()
        };

        Ok(Statement::ConstDeclaration {
            name,
            type_annotation,
            init,
            span: start_span.merge(end_span),
        })
    }

    fn parse_return_statement(&mut self) -> ParseResult<Statement> {
        let start_span = self.advance().span; // return

        let argument = if matches!(
            self.peek().kind,
            TokenKind::Semicolon | TokenKind::RCurly | TokenKind::EoF
        ) {
            None
        } else {
            Some(self.parse_expression()?)
        };

        let end_span = if matches!(self.peek().kind, TokenKind::Semicolon) {
            self.advance().span
        } else if let Some(arg) = &argument {
            arg.span()
        } else {
            start_span
        };

        Ok(Statement::Return {
            argument,
            span: start_span.merge(end_span),
        })
    }

    fn parse_expression(&mut self) -> ParseResult<Expression> {
        let test = self.parse_binary()?;

        if matches!(self.peek().kind, TokenKind::Ternary) {
            self.advance(); // ?
            let consequent = self.parse_expression()?;

            let colon_tok = self.advance();
            match colon_tok.kind {
                TokenKind::Colon => {}
                other => {
                    return Err(ParseError {
                        message: format!("Expected ':' in ternary, got {other:?}"),
                        span: colon_tok.span,
                    });
                }
            }

            let alternate = self.parse_expression()?;
            let span = test.span().merge(alternate.span());
            Ok(Expression::Conditional {
                test: Box::new(test),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
                span,
            })
        } else {
            Ok(test)
        }
    }

    fn parse_binary(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_primary()?;

        loop {
            let op = match &self.peek().kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Multiply => BinaryOp::Multiply,
                _ => break,
            };
            self.advance();
            let right = self.parse_primary()?;
            let span = left.span().merge(right.span());
            left = Expression::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_primary(&mut self) -> ParseResult<Expression> {
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
            TokenKind::LBracket => self.parse_array(tok_span)?,
            TokenKind::LParen => {
                if self.looks_like_arrow_fn() {
                    self.parse_arrow_function(tok_span)?
                } else {
                    let inner = self.parse_expression()?;
                    let rparen = self.advance();
                    match rparen.kind {
                        TokenKind::RParen => {}
                        other => {
                            return Err(ParseError {
                                message: format!("Expected ')', got {other:?}"),
                                span: rparen.span,
                            });
                        }
                    }
                    inner
                }
            }
            other => {
                return Err(ParseError {
                    message: format!("Unexpected token in expression: {other:?}"),
                    span: tok_span,
                });
            }
        };

        // 後置: ( で呼び出し [ でメンバアクセス
        loop {
            match &self.peek().kind {
                TokenKind::LParen => {
                    expr = self.parse_call(expr)?;
                }
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expression()?;
                    let rbracket = self.advance();
                    match rbracket.kind {
                        TokenKind::RBracket => {}
                        other => {
                            return Err(ParseError {
                                message: format!("Expected ']' in member access, got {other:?}"),
                                span: rbracket.span,
                            });
                        }
                    }
                    let span = expr.span().merge(rbracket.span);
                    expr = Expression::Member {
                        object: Box::new(expr),
                        index: Box::new(index),
                        span,
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_call(&mut self, callee: Expression) -> ParseResult<Expression> {
        self.advance(); // (

        let mut arguments = Vec::new();

        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                arguments.push(self.parse_expression()?);
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
            other => {
                return Err(ParseError {
                    message: format!("Expected ')', got {other:?}"),
                    span: rparen.span,
                });
            }
        }

        let span = callee.span().merge(rparen.span);
        Ok(Expression::Call {
            callee: Box::new(callee),
            arguments,
            span,
        })
    }

    fn parse_array(&mut self, lbracket_span: Span) -> ParseResult<Expression> {
        let mut elements = Vec::new();

        if !matches!(self.peek().kind, TokenKind::RBracket) {
            loop {
                elements.push(self.parse_expression()?);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let rbracket = self.advance();
        match rbracket.kind {
            TokenKind::RBracket => {}
            other => {
                return Err(ParseError {
                    message: format!("Expected ']', got {other:?}"),
                    span: rbracket.span,
                });
            }
        }

        Ok(Expression::Array {
            elements,
            span: lbracket_span.merge(rbracket.span),
        })
    }

    fn parse_arrow_function(&mut self, lparen_span: Span) -> ParseResult<Expression> {
        let mut params = Vec::new();

        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                let name_tok = self.advance();
                let name_span = name_tok.span;
                let name = match name_tok.kind {
                    TokenKind::Ident(s) => s,
                    other => {
                        return Err(ParseError {
                            message: format!("Expected param name, got {other:?}"),
                            span: name_span,
                        });
                    }
                };

                let (type_annotation, param_end_span) =
                    if matches!(self.peek().kind, TokenKind::Colon) {
                        self.advance();
                        let ty = self.parse_type_annotation()?;
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
            other => {
                return Err(ParseError {
                    message: format!("Expected ')', got {other:?}"),
                    span: rparen.span,
                });
            }
        }

        let return_type = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        let arrow_tok = self.advance();
        match arrow_tok.kind {
            TokenKind::Arrow => {}
            other => {
                return Err(ParseError {
                    message: format!("Expected '=>', got {other:?}"),
                    span: arrow_tok.span,
                });
            }
        }

        let lcurly = self.advance();
        match lcurly.kind {
            TokenKind::LCurly => {}
            other => {
                return Err(ParseError {
                    message: format!("Expected '{{', got {other:?}"),
                    span: lcurly.span,
                });
            }
        }

        let mut body = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RCurly) {
            body.push(self.parse_statement()?);
        }

        let rcurly = self.advance(); // }

        Ok(Expression::ArrowFunction {
            params,
            return_type,
            body,
            span: lparen_span.merge(rcurly.span),
        })
    }

    fn parse_type_annotation(&mut self) -> ParseResult<TypeAnnotation> {
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
                    let elem = self.parse_type_annotation()?;
                    let gt = self.advance();
                    match gt.kind {
                        TokenKind::GreaterThan => {}
                        other => {
                            return Err(ParseError {
                                message: format!("Expected '>', got {other:?}"),
                                span: gt.span,
                            });
                        }
                    }
                    return Ok(TypeAnnotation::Array {
                        element: Box::new(elem),
                        span: start_span.merge(gt.span),
                    });
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
            other => {
                return Err(ParseError {
                    message: format!("Expected type annotation, got {other:?}"),
                    span: start_span,
                });
            }
        };

        // 後置 T[]
        if matches!(self.peek().kind, TokenKind::LBracket) {
            self.advance();
            let rbracket = self.advance();
            match rbracket.kind {
                TokenKind::RBracket => {}
                other => {
                    return Err(ParseError {
                        message: format!("Expected ']' in array type, got {other:?}"),
                        span: rbracket.span,
                    });
                }
            }
            let base_span = base.span();
            return Ok(TypeAnnotation::Array {
                element: Box::new(base),
                span: base_span.merge(rbracket.span),
            });
        }

        Ok(base)
    }
}

pub fn parse_result(tokens: Vec<Token>) -> ParseResult<Vec<Statement>> {
    let mut parser = Parser { tokens, pos: 0 };
    let mut statements = Vec::new();
    while !parser.at_end() {
        statements.push(parser.parse_statement()?);
    }
    Ok(statements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenize_span::tokenize_span;

    #[test]
    fn const_decl_span_covers_whole_statement() {
        // "const x = 5;"
        //  0          12
        let stmts = parse_result(tokenize_span("const x = 5;")).unwrap();
        assert_eq!(stmts[0].span(), Span { start: 0, end: 12 });
    }

    #[test]
    fn binary_span_covers_both_operands() {
        // "const x = 1 + 2;"
        //           10  14
        let stmts = parse_result(tokenize_span("const x = 1 + 2;")).unwrap();
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            assert_eq!(init.span(), Span { start: 10, end: 15 });
        } else {
            panic!("expected ConstDeclaration");
        }
    }

    #[test]
    fn missing_equals_returns_error() {
        let err = parse_result(tokenize_span("const x 5;")).unwrap_err();
        assert!(err.message.contains("Expected '='"), "got: {}", err.message);
    }

    #[test]
    fn unclosed_paren_returns_error() {
        let err = parse_result(tokenize_span("const x = (1 + 2;")).unwrap_err();
        assert!(err.message.contains("Expected ')'"), "got: {}", err.message);
    }
}
