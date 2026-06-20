//! 節10 `naming_indexed` の span 付き版 (節12 で節14 から前倒し)。
//!
//! `naming_indexed` は `crate::parse` AST 上で span を持たなかった。ここでは `ast_span` AST の上に
//! 同じ index 型 naming を作り直し、`Symbol` / `Reference` に span を載せる。
//! さらに **`HashMap<(usize, usize), SymbolId>`**(identifier ノードの span → 解決先 symbol)を
//! 構築して返す。これが rename(節12) / go-to-definition(節14) の索引になる。
//!
//! 設計: **Box AST + Index Semantic**。AST 本体は `ast_span`(Box ベース) のまま、
//! symbol/scope/reference は `Vec` + `u32` 添字で外付け。`ast_span` には一切手を入れず、
//! identifier ノードの span を NodeId 代わりに使って「ノード → symbol」を引く。
//!
//! 系譜: `naming.rs`(HashSet, parse AST) → `naming_indexed.rs`(index, parse AST) →
//! `naming_span.rs`(index + span AST)。

use crate::ast_span::{Expression, Parameter, Statement};
use crate::tokenize_span::Span;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub struct NamingError {
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReferenceId(pub u32);

/// 宣言された変数の実体。`naming_indexed` の `Symbol` に **宣言位置の span** を足したもの。
#[derive(Debug)]
pub struct Symbol {
    pub name: String,
    pub scope_id: ScopeId,
    /// 宣言位置。const は文の span、param は param の span。
    pub span: Span,
}

#[derive(Debug)]
pub struct Scope {
    pub parent: Option<ScopeId>,
    pub bindings: HashMap<String, SymbolId>,
}

/// 識別子の使用箇所。`naming_indexed` の `Reference` に **使用位置の span** を足したもの。
#[derive(Debug)]
pub struct Reference {
    pub name: String,
    pub scope_id: ScopeId,
    /// 使用位置 (identifier ノードの span)。
    pub span: Span,
    pub resolved: Option<SymbolId>,
}

/// span をマップのキーにするための値。`Span` は `Hash` を持たない (tokenize_span は不改造) ので
/// `(start, end)` のタプルに落として使う。identifier ノードの位置は一意なので NodeId 代わりになる。
pub type SpanKey = (usize, usize);

pub fn span_key(span: Span) -> SpanKey {
    (span.start, span.end)
}

/// 解析結果。後段 (rename / LSP) はこれを受け取って索引を引く。
pub struct Semantic {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    pub references: Vec<Reference>,
    /// identifier ノードの span → 解決先 symbol。未解決の使用箇所は入らない。
    pub resolved: HashMap<SpanKey, SymbolId>,
    pub errors: Vec<NamingError>,
}

impl Semantic {
    /// identifier ノードの span から解決先 symbol を引く (rename / go-to-definition 用)。
    pub fn symbol_at(&self, span: Span) -> Option<SymbolId> {
        self.resolved.get(&span_key(span)).copied()
    }
}

struct SemanticBuilder {
    symbols: Vec<Symbol>,
    scopes: Vec<Scope>,
    references: Vec<Reference>,
    resolved: HashMap<SpanKey, SymbolId>,
    current_scope: ScopeId,
    errors: Vec<NamingError>,
}

impl SemanticBuilder {
    fn new() -> Self {
        Self {
            symbols: Vec::new(),
            scopes: vec![Scope {
                parent: None,
                bindings: HashMap::new(),
            }],
            references: Vec::new(),
            resolved: HashMap::new(),
            current_scope: ScopeId(0),
            errors: Vec::new(),
        }
    }

    fn enter_scope(&mut self) -> ScopeId {
        let new_id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(Scope {
            parent: Some(self.current_scope),
            bindings: HashMap::new(),
        });
        let saved = self.current_scope;
        self.current_scope = new_id;
        saved
    }

    fn leave_scope(&mut self, saved: ScopeId) {
        self.current_scope = saved;
    }

    fn declare(&mut self, name: &str, span: Span) -> Option<SymbolId> {
        let idx = self.current_scope.0 as usize;
        if self.scopes[idx].bindings.contains_key(name) {
            self.errors.push(NamingError {
                message: format!("Duplicate declaration of variable: {name}"),
            });
            return None;
        }
        let symbol_id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol {
            name: name.to_string(),
            scope_id: self.current_scope,
            span,
        });
        self.scopes[idx]
            .bindings
            .insert(name.to_string(), symbol_id);
        Some(symbol_id)
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        let mut current = Some(self.current_scope);
        while let Some(scope_id) = current {
            let scope = &self.scopes[scope_id.0 as usize];
            if let Some(&symbol_id) = scope.bindings.get(name) {
                return Some(symbol_id);
            }
            current = scope.parent;
        }
        None
    }

    fn visit_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::ConstDeclaration {
                name, init, span, ..
            } => {
                // init を先に訪問してから declare (節10 と同じ)。
                self.visit_expression(init);
                self.declare(name, *span);
            }
            Statement::Return { argument, .. } => {
                if let Some(expr) = argument {
                    self.visit_expression(expr);
                }
            }
        }
    }

    fn visit_expression(&mut self, expr: &Expression) {
        match expr {
            Expression::Identifier { name, span } => {
                let resolved = self.resolve(name);
                if resolved.is_none() {
                    self.errors.push(NamingError {
                        message: format!("Reference to undeclared variable: {name}"),
                    });
                }
                if let Some(symbol_id) = resolved {
                    // 外部マップ: この identifier ノード (span) → symbol。
                    self.resolved.insert(span_key(*span), symbol_id);
                }
                self.references.push(Reference {
                    name: name.clone(),
                    scope_id: self.current_scope,
                    span: *span,
                    resolved,
                });
            }

            Expression::Number { .. }
            | Expression::String { .. }
            | Expression::Boolean { .. } => {}

            Expression::Binary { left, right, .. } => {
                self.visit_expression(left);
                self.visit_expression(right);
            }
            Expression::Conditional {
                test,
                consequent,
                alternate,
                ..
            } => {
                self.visit_expression(test);
                self.visit_expression(consequent);
                self.visit_expression(alternate);
            }
            Expression::Call {
                callee, arguments, ..
            } => {
                self.visit_expression(callee);
                for arg in arguments {
                    self.visit_expression(arg);
                }
            }
            Expression::Array { elements, .. } => {
                for elem in elements {
                    self.visit_expression(elem);
                }
            }
            Expression::Member { object, index, .. } => {
                self.visit_expression(object);
                self.visit_expression(index);
            }
            Expression::ArrowFunction { params, body, .. } => {
                let saved = self.enter_scope();
                for param in params {
                    let Parameter { name, span, .. } = param;
                    self.declare(name, *span);
                }
                for stmt in body {
                    self.visit_statement(stmt);
                }
                self.leave_scope(saved);
            }
        }
    }
}

/// `ast_span` AST を解析して `Semantic` を返す。
pub fn analyze(statements: &[Statement]) -> Semantic {
    let mut builder = SemanticBuilder::new();
    for stmt in statements {
        builder.visit_statement(stmt);
    }
    Semantic {
        symbols: builder.symbols,
        scopes: builder.scopes,
        references: builder.references,
        resolved: builder.resolved,
        errors: builder.errors,
    }
}

/// 名前解決エラーだけが欲しい場合の薄いラッパ (`naming_indexed::name_check` 相当)。
pub fn name_check(statements: &[Statement]) -> Vec<NamingError> {
    analyze(statements).errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_result::parse_result;
    use crate::tokenize_span::tokenize_span;

    fn parse(src: &str) -> Vec<Statement> {
        parse_result(tokenize_span(src)).unwrap()
    }

    // --- naming_indexed と同じ名前解決の同値テスト ---

    #[test]
    fn no_errors_for_simple_const() {
        assert_eq!(name_check(&parse("const x = 5;")), vec![]);
    }

    #[test]
    fn detects_undeclared_reference() {
        let errors = name_check(&parse("const x = y;"));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("y"));
    }

    #[test]
    fn detects_duplicate_declaration() {
        let errors = name_check(&parse("const x = 1; const x = 2;"));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Duplicate"));
    }

    #[test]
    fn declared_variable_can_be_referenced() {
        assert_eq!(name_check(&parse("const x = 1; const y = x;")), vec![]);
    }

    #[test]
    fn check_ternary_all_referenced() {
        let errors = name_check(&parse("const result = a ? b : c;"));
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn arrow_function_param_visible_in_body() {
        assert_eq!(
            name_check(&parse("const f = (x) => { return x; };")),
            vec![]
        );
    }

    #[test]
    fn arrow_function_param_not_visible_outside() {
        let errors = name_check(&parse("const f = (x) => { return 1; }; const y = x;"));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("x"));
    }

    #[test]
    fn nested_arrow_functions() {
        assert_eq!(
            name_check(&parse("const f = (x) => { return (y) => { return x; }; };")),
            vec![]
        );
    }

    #[test]
    fn duplicate_param_names() {
        let errors = name_check(&parse("const f = (x, x) => { return x; };"));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Duplicate"));
    }

    // --- span 付きならではのテスト ---

    #[test]
    fn symbols_carry_decl_span() {
        let sem = analyze(&parse("const x = 1;"));
        assert_eq!(sem.symbols.len(), 1);
        // 宣言の span が記録されている (空ではない)。
        assert!(sem.symbols[0].span.end > sem.symbols[0].span.start);
    }

    #[test]
    fn resolved_map_links_use_to_symbol() {
        // const a = 1; const b = a; の "a" 使用箇所が a の宣言 symbol に解決される。
        let stmts = parse("const a = 1; const b = a;");
        let sem = analyze(&stmts);

        // 使用箇所の identifier ノード (Expression::Identifier) の span を取り出す。
        let use_span = if let Statement::ConstDeclaration { init, .. } = &stmts[1] {
            if let Expression::Identifier { span, .. } = init {
                *span
            } else {
                panic!("expected identifier");
            }
        } else {
            panic!("expected const decl");
        };

        let sym = sem.symbol_at(use_span).expect("a should resolve");
        assert_eq!(sem.symbols[sym.0 as usize].name, "a");
    }

    #[test]
    fn shadowing_resolves_to_inner() {
        // 外側 x と param x。body 内の x は param (内側) に解決されるべき。
        let stmts = parse("const x = 1; const f = (x) => { return x; };");
        let sem = analyze(&stmts);

        // body 内の return x の span を取り出す。
        let inner_use = if let Statement::ConstDeclaration { init, .. } = &stmts[1] {
            if let Expression::ArrowFunction { body, .. } = init {
                if let Statement::Return {
                    argument: Some(Expression::Identifier { span, .. }),
                    ..
                } = &body[0]
                {
                    *span
                } else {
                    panic!("expected return identifier");
                }
            } else {
                panic!("expected arrow");
            }
        } else {
            panic!("expected const decl");
        };

        let sym = sem.symbol_at(inner_use).expect("x should resolve");
        // 解決先 symbol が param スコープ (ScopeId(1)) のもの = 内側の x。
        let resolved_scope = sem.symbols[sym.0 as usize].scope_id;
        assert_ne!(resolved_scope, ScopeId(0), "should resolve to inner param, not top-level");
    }
}