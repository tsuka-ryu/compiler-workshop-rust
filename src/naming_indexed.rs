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
        todo!("新 Scope を push し、current_scope を新 id に。旧 id を返す")
    }

    /// `current_scope` を `saved` に戻す。Vec からは消さない (ここが旧版との違い)。
    /// 旧版の `scopes.pop()` 相当だが、実体は残す。
    fn leave_scope(&mut self, _saved: ScopeId) {
        todo!("current_scope を saved に戻すだけ")
    }

    /// `current_scope` に名前を登録する。旧版の `declare` 相当。
    /// 重複は同一スコープ内だけ見る。
    fn declare(&mut self, _name: &str) -> Option<SymbolId> {
        todo!("bindings に重複チェックして Symbol を push、SymbolId を返す")
    }

    /// `current_scope` から `parent` チェーンを辿って名前を探す。
    /// 旧版の `is_declared` 相当だが bool ではなく `SymbolId` を返す。
    fn resolve(&self, _name: &str) -> Option<SymbolId> {
        todo!("current_scope から parent を Option<ScopeId> で辿って bindings を探す")
    }

    fn report(&mut self, message: String) {
        self.errors.push(NamingError { message });
    }

    fn visit_statement(&mut self, _stmt: &Statement) {
        todo!("naming.rs の visit_statement を index 型版に移植")
    }

    fn visit_expression(&mut self, _expr: &Expression) {
        todo!("naming.rs の visit_expression を index 型版に移植")
    }
}

pub fn name_check(statements: &[Statement]) -> Vec<NamingError> {
    let mut builder = SemanticBuilder::new();
    for stmt in statements {
        builder.visit_statement(stmt);
    }
    builder.errors
}