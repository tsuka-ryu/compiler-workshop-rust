//! `naming.rs` の index 型版 (節10)。
//!
//! `naming.rs` は `Vec<HashSet<String>>` のスタックでスコープを管理し、
//! スコープを抜けると `pop` で**捨てて**いた。ここでは oxc の `oxc_semantic` に倣い、
//! スコープ・シンボル・参照をそれぞれ `Vec` に**捨てずに溜め**、`u32` の添字
//! (`ScopeId` / `SymbolId` / `ReferenceId`) で間接参照する。
//!
//! 違いの肝:
//! - スコープは抜けても Vec から消さない。`current_scope` (今いる場所) を 1 個持ち回るだけ。
//! - 親子関係は `Scope { parent: Option<ScopeId> }` で明示する (旧版はスタック順序が暗黙の親子)。
//! - 参照は bool 判定して捨てず、`Reference { resolved: Option<SymbolId> }` として残す。

use crate::parse::{Expression, Statement};
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub struct NamingError {
    pub message: String,
}

/// `symbols: Vec<Symbol>` の添字。宣言 1 個に 1 つ振る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

/// `scopes: Vec<Scope>` の添字。スコープ 1 個に 1 つ振る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

/// `references: Vec<Reference>` の添字。識別子の使用箇所 1 個に 1 つ振る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReferenceId(pub u32);

/// 宣言された変数の実体。旧版には存在せず、名前を `HashSet` に入れるだけだった。
#[derive(Debug)]
pub struct Symbol {
    pub name: String,
    /// どのスコープで宣言されたか。
    pub scope_id: ScopeId,
}

/// スコープ 1 階層。旧版の `HashSet<String>` 1 枚に相当するが、
/// 親への参照 (`parent`) を自分で持つ点が違う。
#[derive(Debug)]
pub struct Scope {
    /// 親スコープ。トップレベルだけ `None`。
    pub parent: Option<ScopeId>,
    /// この階層で宣言された名前 → SymbolId。親の宣言は含まない。
    pub bindings: HashMap<String, SymbolId>,
}

/// 識別子の使用箇所。`resolved` が `None` なら未宣言参照 (エラー)。
#[derive(Debug)]
pub struct Reference {
    pub name: String,
    /// どのスコープから参照したか。
    pub scope_id: ScopeId,
    /// 解決先の宣言。`None` は未解決 (未宣言)。
    pub resolved: Option<SymbolId>,
}

/// 解析中に symbol / scope / reference を溜めていく本体。
/// 旧版の `Resolver` 相当だが、スタックを持たず `current_scope` を 1 個だけ持つ。
pub struct SemanticBuilder {
    symbols: Vec<Symbol>,
    scopes: Vec<Scope>,
    references: Vec<Reference>,
    /// いま解析中のスコープ。旧版の「スタックの一番後ろ」に相当。
    current_scope: ScopeId,
    errors: Vec<NamingError>,
}

impl SemanticBuilder {
    fn new() -> Self {
        // トップレベルスコープ (ScopeId(0)) を最初に 1 枚積んでおく。
        let root = Scope {
            parent: None,
            bindings: HashMap::new(),
        };
        Self {
            symbols: Vec::new(),
            scopes: vec![root],
            references: Vec::new(),
            current_scope: ScopeId(0),
            errors: Vec::new(),
        }
    }

    /// 新しい子スコープに入り、`current_scope` を付け替える。
    /// 旧版の `scopes.push(HashSet::new())` 相当。戻り値の旧 ScopeId を
    /// `leave_scope` に渡して元に戻す。
    fn enter_scope(&mut self) -> ScopeId {
        // push 直前の len が、これから積む要素の添字になる。
        let new_id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(Scope {
            parent: Some(self.current_scope),
            bindings: HashMap::new(),
        });
        // 付け替える前の current_scope を退避し、それを返す。
        let saved = self.current_scope;
        self.current_scope = new_id;
        saved
    }

    /// `current_scope` を `saved` に戻す。Vec からは消さない (ここが旧版との違い)。
    /// 旧版の `scopes.pop()` 相当だが、実体は残す。
    fn leave_scope(&mut self, saved: ScopeId) {
        self.current_scope = saved;
    }

    /// `current_scope` に名前を登録する。旧版の `declare` 相当。
    /// 重複は同一スコープ内だけ見る。
    fn declare(&mut self, name: &str) -> Option<SymbolId> {
        let idx = self.current_scope.0 as usize;
        // 重複チェックは同一スコープ内だけ。借用を式内で完結させ self.report と衝突させない。
        if self.scopes[idx].bindings.contains_key(name) {
            self.report(format!("Duplicate declaration of variable: {name}"));
            return None;
        }
        // push 直前の len が新しい SymbolId。
        let symbol_id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol {
            name: name.to_string(),
            scope_id: self.current_scope,
        });
        self.scopes[idx]
            .bindings
            .insert(name.to_string(), symbol_id);
        Some(symbol_id)
    }

    /// `current_scope` から `parent` チェーンを辿って名前を探す。
    /// 旧版の `is_declared` 相当だが bool ではなく `SymbolId` を返す。
    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // current_scope から parent チェーンを辿る。旧版の scopes.iter().any() に相当するが、
        // 「今の枝の祖先だけ」を見る点が違う (スタックではなく木を上る)。
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

    fn report(&mut self, message: String) {
        self.errors.push(NamingError { message });
    }

    fn visit_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::ConstDeclaration { name, init, .. } => {
                // JS 版と同じ: init を先に訪問してから declare。
                self.visit_expression(init);
                self.declare(name);
            }
            Statement::Return { argument } => {
                if let Some(expr) = argument {
                    self.visit_expression(expr);
                }
            }
        }
    }

    fn visit_expression(&mut self, expr: &Expression) {
        match expr {
            Expression::Identifier(name) => {
                // 旧版は bool 判定して捨てていたが、ここでは Reference として残す。
                let resolved = self.resolve(name);
                if resolved.is_none() {
                    self.report(format!("Reference to undeclared variable: {name}"));
                }
                self.references.push(Reference {
                    name: name.clone(),
                    scope_id: self.current_scope,
                    resolved,
                });
            }

            // リテラル: 何もしない
            Expression::Number(_) | Expression::String(_) | Expression::Boolean(_) => {}

            // 子を全部訪問
            Expression::Binary { left, right, .. } => {
                self.visit_expression(left);
                self.visit_expression(right);
            }
            Expression::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.visit_expression(test);
                self.visit_expression(consequent);
                self.visit_expression(alternate);
            }
            Expression::Call { callee, arguments } => {
                self.visit_expression(callee);
                for arg in arguments {
                    self.visit_expression(arg);
                }
            }
            Expression::Array(elements) => {
                for elem in elements {
                    self.visit_expression(elem);
                }
            }
            Expression::Member { object, index } => {
                self.visit_expression(object);
                self.visit_expression(index);
            }

            // アロー関数: 新スコープに入って param を declare、本体を訪問して抜ける。
            Expression::ArrowFunction { params, body, .. } => {
                let saved = self.enter_scope();
                for param in params {
                    self.declare(&param.name);
                }
                for stmt in body {
                    self.visit_statement(stmt);
                }
                self.leave_scope(saved);
            }
        }
    }
}

pub fn name_check(statements: &[Statement]) -> Vec<NamingError> {
    let mut builder = SemanticBuilder::new();
    for stmt in statements {
        builder.visit_statement(stmt);
    }
    builder.errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile;

    #[test]
    fn no_errors_for_simple_const() {
        let stmts = compile("const x = 5;");
        assert_eq!(name_check(&stmts), vec![]);
    }

    #[test]
    fn detects_undeclared_reference() {
        let stmts = compile("const x = y;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("y"));
    }

    #[test]
    fn detects_duplicate_declaration() {
        let stmts = compile("const x = 1; const x = 2;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Duplicate"));
    }

    #[test]
    fn declared_variable_can_be_referenced() {
        let stmts = compile("const x = 1; const y = x;");
        assert_eq!(name_check(&stmts), vec![]);
    }

    #[test]
    fn check_binary_with_undeclared() {
        let stmts = compile("const a = 1; const b = a + c;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("c"));
    }

    #[test]
    fn check_ternary_all_referenced() {
        let stmts = compile("const result = a ? b : c;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 3); // a, b, c 全部未宣言
    }

    #[test]
    fn check_call_arguments() {
        let stmts = compile("const x = f(a, b);");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 3); // f, a, b 全部未宣言
    }

    #[test]
    fn check_array_elements() {
        let stmts = compile("const xs = [a, 1, b];");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 2); // a, b
    }

    #[test]
    fn check_nested_expressions() {
        let stmts = compile("const a = 1; const x = (a + unknown) * 2;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("unknown"));
    }

    #[test]
    fn arrow_function_param_visible_in_body() {
        let stmts = compile("const f = (x) => { return x; };");
        assert_eq!(name_check(&stmts), vec![]);
    }

    #[test]
    fn arrow_function_param_not_visible_outside() {
        let stmts = compile("const f = (x) => { return 1; }; const y = x;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("x"));
    }

    #[test]
    fn arrow_function_can_access_outer_const() {
        let stmts = compile("const a = 1; const f = (x) => { return a + x; };");
        assert_eq!(name_check(&stmts), vec![]);
    }

    #[test]
    fn arrow_function_undeclared_in_body() {
        let stmts = compile("const f = (x) => { return y; };");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("y"));
    }

    #[test]
    fn duplicate_param_names() {
        let stmts = compile("const f = (x, x) => { return x; };");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Duplicate"));
    }

    #[test]
    fn nested_arrow_functions() {
        let stmts = compile("const f = (x) => { return (y) => { return x; }; };");
        assert_eq!(name_check(&stmts), vec![]);
    }
}
