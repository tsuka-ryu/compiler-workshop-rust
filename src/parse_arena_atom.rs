use crate::ast_arena_atom::{BinaryOp, Expression, Parameter, Statement, TypeAnnotation};
use crate::atom::Interner;
use crate::tokenize_span::{Span, Token, TokenKind};
use bumpalo::Bump;

struct Parser<'a> {
    bump: &'a Bump,
    interner: Interner<'a>,
    tokens: Vec<Token>,
    pos: usize,
}

impl<'a> Parser<'a> {
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

    fn parse_statement(&mut self) -> Statement<'a> {
        match self.peek().kind {
            TokenKind::Const => self.parse_const_declaration(),
            TokenKind::Return => self.parse_return_statement(),
            _ => panic!("Unexpected token: {:?}", self.peek()),
        }
    }
    fn parse_const_declaration(&mut self) -> Statement<'a> {
        let start_span = self.advance().span;

        let ident_tok = self.advance();
        let name = match ident_tok.kind {
            TokenKind::Ident(s) => self.interner.intern(&s),
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

    fn parse_return_statement(&mut self) -> Statement<'a> {
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

    fn parse_expression(&mut self) -> Expression<'a> {
        let test = self.parse_binary();

        if matches!(self.peek().kind, TokenKind::Ternary) {
            self.advance(); // ?
            let consequent = self.parse_expression();

            let colon_tok = self.advance();
            match colon_tok.kind {
                TokenKind::Colon => {}
                other => panic!("Expected ':' in ternary, got {other:?}"),
            }

            let alternate = self.parse_expression();
            let span = test.span().merge(alternate.span());
            Expression::Conditional {
                test: self.bump.alloc(test),
                consequent: self.bump.alloc(consequent),
                alternate: self.bump.alloc(alternate),
                span,
            }
        } else {
            test
        }
    }

    fn parse_binary(&mut self) -> Expression<'a> {
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
                left: self.bump.alloc(left),
                op,
                right: self.bump.alloc(right),
                span,
            };
        }

        left
    }

    fn parse_primary(&mut self) -> Expression<'a> {
        let tok = self.advance();
        let tok_span = tok.span;
        let mut expr = match tok.kind {
            TokenKind::Number(n) => Expression::Number {
                value: n,
                span: tok_span,
            },
            TokenKind::StringLit(s) => Expression::String {
                value: self.interner.intern(&s),
                span: tok_span,
            },
            TokenKind::Boolean(b) => Expression::Boolean {
                value: b,
                span: tok_span,
            },
            TokenKind::Ident(name) => Expression::Identifier {
                name: self.interner.intern(&name),
                span: tok_span,
            },
            TokenKind::LBracket => self.parse_array(tok_span),
            TokenKind::LParen => {
                if self.looks_like_arrow_fn() {
                    self.parse_arrow_function(tok_span)
                } else {
                    let inner = self.parse_expression();
                    let rparen = self.advance();
                    match rparen.kind {
                        TokenKind::RParen => {}
                        other => panic!("Expected ')', got {other:?}"),
                    }
                    inner
                }
            }
            other => panic!("Unexpected token in expression: {other:?}"),
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
                    let rbracket = self.advance();
                    match rbracket.kind {
                        TokenKind::RBracket => {}
                        other => panic!("Expected ']' in member access, got {other:?}"),
                    }
                    let span = expr.span().merge(rbracket.span);
                    expr = Expression::Member {
                        object: self.bump.alloc(expr),
                        index: self.bump.alloc(index),
                        span,
                    };
                }
                _ => break,
            }
        }

        expr
    }

    fn parse_call(&mut self, callee: Expression<'a>) -> Expression<'a> {
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

        let rparen = self.advance();
        match rparen.kind {
            TokenKind::RParen => {}
            other => panic!("Expected ')', got {other:?}"),
        }

        let span = callee.span().merge(rparen.span);
        Expression::Call {
            callee: self.bump.alloc(callee),
            arguments,
            span,
        }
    }

    fn parse_array(&mut self, lbracket_span: Span) -> Expression<'a> {
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

        let rbracket = self.advance();
        match rbracket.kind {
            TokenKind::RBracket => {}
            other => panic!("Expected ']', got {other:?}"),
        }

        Expression::Array {
            elements,
            span: lbracket_span.merge(rbracket.span),
        }
    }

    fn parse_arrow_function(&mut self, lparen_span: Span) -> Expression<'a> {
        let mut params = Vec::new();

        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                let name_tok = self.advance();
                let name_span = name_tok.span;
                let name = match name_tok.kind {
                    TokenKind::Ident(s) => self.interner.intern(&s),
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

    fn parse_type_annotation(&mut self) -> TypeAnnotation<'a> {
        let tok = self.advance();
        let start_span = tok.span;
        let base = match tok.kind {
            TokenKind::TypeNumber => TypeAnnotation::Named {
                name: self.interner.intern("number"),
                span: start_span,
            },
            TokenKind::TypeString => TypeAnnotation::Named {
                name: self.interner.intern("string"),
                span: start_span,
            },
            TokenKind::TypeBoolean => TypeAnnotation::Named {
                name: self.interner.intern("boolean"),
                span: start_span,
            },
            TokenKind::TypeVoid => TypeAnnotation::Named {
                name: self.interner.intern("void"),
                span: start_span,
            },
            TokenKind::TypeInt => TypeAnnotation::Named {
                name: self.interner.intern("Void"),
                span: start_span,
            },
            TokenKind::TypeFloat => TypeAnnotation::Named {
                name: self.interner.intern("Float"),
                span: start_span,
            },
            TokenKind::TypeBool => TypeAnnotation::Named {
                name: self.interner.intern("Bool"),
                span: start_span,
            },
            TokenKind::TypeUnit => TypeAnnotation::Named {
                name: self.interner.intern("Unit"),
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
                        element: self.bump.alloc(elem),
                        span: start_span.merge(gt.span),
                    };
                }
                TypeAnnotation::Named {
                    name: self.interner.intern("Array"),
                    span: start_span,
                }
            }
            TokenKind::Ident(name) => TypeAnnotation::Named {
                name: self.interner.intern(&name),
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
                element: self.bump.alloc(base),
                span: base_span.merge(rbracket.span),
            };
        }

        base
    }
}

pub fn parse_span<'a>(bump: &'a Bump, tokens: Vec<Token>) -> Vec<Statement<'a>> {
    let mut parser = Parser {
        bump,
        interner: Interner::new(bump),
        tokens,
        pos: 0,
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
        let bump = Bump::new();
        let stmts = parse_span(&bump, tokenize_span("const x = 5;"));
        assert_eq!(stmts[0].span(), Span { start: 0, end: 12 });
    }

    #[test]
    fn binary_span_covers_both_operands() {
        // "const x = 1 + 2;"
        //           10  14
        let bump = Bump::new();
        let stmts = parse_span(&bump, tokenize_span("const x = 1 + 2;"));
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            assert_eq!(init.span(), Span { start: 10, end: 15 });
        } else {
            panic!("expected ConstDeclaration");
        }
    }

    #[test]
    fn repeated_identifier_is_interned_to_same_pointer() {
        // "const a = a + a;" の右辺 a + a は同じ識別子 a が2回。
        // parser が Interner を通しているので、両者は同じ arena 上の
        // 文字列を共有する (ptr_eq が true)。
        let bump = Bump::new();
        let stmts = parse_span(&bump, tokenize_span("const a = a + a;"));
        let Statement::ConstDeclaration { init, .. } = &stmts[0] else {
            panic!("expected ConstDeclaration");
        };
        let Expression::Binary { left, right, .. } = init else {
            panic!("expected Binary");
        };
        let (Expression::Identifier { name: l, .. }, Expression::Identifier { name: r, .. }) =
            (left, right)
        else {
            panic!("expected two Identifiers");
        };
        assert!(l.ptr_eq(r), "同じ識別子なのにポインタが別 = interning が効いていない");
    }
}
