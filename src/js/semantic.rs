//! 意味解析 (semantic analysis)。semantic_analysis 章。
//!
//! パース後の AST を走査して **スコープツリー** を作り、
//! - 宣言 (`var`/`let`/`const`) を `Symbol` として各スコープに登録
//! - 使用箇所 (`IdentifierReference`) を `Reference` として親チェーンで解決
//! - Early Error (同一スコープでの重複宣言) を検出
//! する。
//!
//! 設計は toy の `crate::naming_indexed` と同じ:
//! `SymbolId`/`ScopeId` を `u32` newtype にし、間接参照を添字で表す (`'a` が要らない)。
//! 走査は visitor パターン (深さ優先・pre-order) で、ブロックに入る/出るで
//! `enter_scope`/`leave_scope` を呼んでスコープツリーを組む。
//!
//! subset の簡略化:
//! - `var` の関数スコープ巻き上げ (hoisting) や TDZ は扱わない。重複は種別に関係なく
//!   「同一スコープに同名」を一律 early error にする (実 JS の `var a; var a;` 許容は省略)。
//! - 初期化子 → 宣言の順で走るので `let x = x` の `x` は自分自身には解決されない。

use std::collections::HashMap;

use super::ast::*;

/// シンボル (宣言された名前) の id。`symbols` Vec への添字。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

/// スコープの id。`scopes` Vec への添字。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

/// スコープの種類。章の `ScopeFlags` の subset 版 (関数/アローが無いので 2 種だけ)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// プログラム最上位。
    Top,
    /// `{ ... }` ブロック。
    Block,
}

/// 宣言された名前の実体。
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: VariableDeclarationKind,
    /// この名前が属するスコープ。
    pub scope: ScopeId,
    /// 宣言箇所 (束縛側識別子のスパン)。
    pub span: Node,
}

/// スコープツリーの 1 ノード。
#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
    pub kind: ScopeKind,
    /// 親スコープ。最上位だけ `None`。これを辿って名前解決する。
    pub parent: Option<ScopeId>,
    /// このスコープで宣言された名前 → シンボル。
    pub bindings: HashMap<String, SymbolId>,
}

/// 識別子の使用箇所。`resolved` が `None` なら未解決 (どのスコープにも宣言が無い)。
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub name: String,
    pub span: Node,
    pub resolved: Option<SymbolId>,
}

/// 意味解析で見つかる Early Error。
#[derive(Debug, Clone, PartialEq)]
pub enum SemanticError {
    /// 同一スコープでの重複宣言 (`let a; let a;`)。
    DuplicateDeclaration { name: String, span: Node },
}

/// 意味解析の結果一式。
#[derive(Debug, Clone, PartialEq)]
pub struct Semantic {
    pub scopes: Vec<Scope>,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub errors: Vec<SemanticError>,
}

impl Semantic {
    /// シンボルを id で引く。
    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.0 as usize]
    }

    /// スコープを id で引く。
    pub fn scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.0 as usize]
    }
}

/// AST を解析して `Semantic` を返すエントリポイント。
pub fn analyze(program: &Program) -> Semantic {
    let mut builder = SemanticBuilder::new();
    builder.visit_program(program);
    builder.finish()
}

/// スコープツリーを組みながら AST を走査するビルダー。
struct SemanticBuilder {
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    references: Vec<Reference>,
    errors: Vec<SemanticError>,
    /// 現在いるスコープ。`enter`/`leave` で付け替える。
    current_scope: ScopeId,
}

impl SemanticBuilder {
    fn new() -> Self {
        // 最上位スコープを 1 つ作っておく。
        let top = Scope {
            kind: ScopeKind::Top,
            parent: None,
            bindings: HashMap::new(),
        };
        Self {
            scopes: vec![top],
            symbols: Vec::new(),
            references: Vec::new(),
            errors: Vec::new(),
            current_scope: ScopeId(0),
        }
    }

    fn finish(self) -> Semantic {
        Semantic {
            scopes: self.scopes,
            symbols: self.symbols,
            references: self.references,
            errors: self.errors,
        }
    }

    // -- スコープ操作 (章の enter_scope / leave_scope) ------------------------

    /// 新しい子スコープを作って current を付け替える。実体は Vec に積む。
    fn enter_scope(&mut self, kind: ScopeKind) {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(Scope {
            kind,
            parent: Some(self.current_scope),
            bindings: HashMap::new(),
        });
        self.current_scope = id;
    }

    /// current を親へ戻す。スコープ実体 (とその bindings) は Vec に残す。
    fn leave_scope(&mut self) {
        let parent = self.scopes[self.current_scope.0 as usize].parent;
        self.current_scope = parent.expect("最上位スコープからは出られない");
    }

    /// 現在のスコープに名前を宣言する。重複なら early error。
    fn declare(&mut self, name: &str, kind: VariableDeclarationKind, span: Node) {
        let scope = &self.scopes[self.current_scope.0 as usize];
        if scope.bindings.contains_key(name) {
            self.errors.push(SemanticError::DuplicateDeclaration {
                name: name.to_string(),
                span,
            });
            // 最初の宣言を残し、2 つ目以降は登録しない。
            return;
        }
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol {
            name: name.to_string(),
            kind,
            scope: self.current_scope,
            span,
        });
        self.scopes[self.current_scope.0 as usize]
            .bindings
            .insert(name.to_string(), id);
    }

    /// current から親チェーンを辿って名前を解決する。見つからなければ `None`。
    fn resolve(&self, name: &str) -> Option<SymbolId> {
        let mut scope = Some(self.current_scope);
        while let Some(id) = scope {
            let s = &self.scopes[id.0 as usize];
            if let Some(&symbol) = s.bindings.get(name) {
                return Some(symbol);
            }
            scope = s.parent;
        }
        None
    }

    // -- 走査 (visitor) ------------------------------------------------------

    fn visit_program(&mut self, program: &Program) {
        for stmt in &program.body {
            self.visit_statement(stmt);
        }
    }

    fn visit_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    // 初期化子を先に走査してから宣言する (使用 → 宣言の順)。
                    if let Some(init) = &declarator.init {
                        self.visit_expression(init);
                    }
                    self.declare(&declarator.id.name, decl.kind, declarator.id.node);
                }
            }
            Statement::BlockStatement(block) => {
                self.enter_scope(ScopeKind::Block);
                for stmt in &block.body {
                    self.visit_statement(stmt);
                }
                self.leave_scope();
            }
            Statement::ExpressionStatement(stmt) => self.visit_expression(&stmt.expression),
            Statement::DebuggerStatement(_) => {}
        }
    }

    fn visit_expression(&mut self, expr: &Expression) {
        match expr {
            Expression::Identifier(ident) => {
                let resolved = self.resolve(&ident.name);
                self.references.push(Reference {
                    name: ident.name.clone(),
                    span: ident.node,
                    resolved,
                });
            }
            Expression::UnaryExpression(unary) => self.visit_expression(&unary.argument),
            Expression::BinaryExpression(bin) => {
                self.visit_expression(&bin.left);
                self.visit_expression(&bin.right);
            }
            Expression::ConditionalExpression(cond) => {
                self.visit_expression(&cond.test);
                self.visit_expression(&cond.consequent);
                self.visit_expression(&cond.alternate);
            }
            Expression::AwaitExpression(a) => self.visit_expression(&a.expression),
            Expression::YieldExpression(y) => self.visit_expression(&y.expression),
            // リテラルは名前を持たないので何もしない。
            Expression::NumberLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::parser::parse;

    fn analyze_src(source: &str) -> Semantic {
        analyze(&parse(source).expect("should parse"))
    }

    #[test]
    fn declares_top_level_symbols() {
        let sem = analyze_src("let a = 1; const b = 2;");
        assert_eq!(sem.symbols.len(), 2);
        assert_eq!(sem.symbols[0].name, "a");
        assert_eq!(sem.symbols[0].kind, VariableDeclarationKind::Let);
        assert_eq!(sem.symbols[1].name, "b");
        assert_eq!(sem.symbols[1].kind, VariableDeclarationKind::Const);
        assert!(sem.errors.is_empty());
        // 最上位スコープ 1 つだけ。
        assert_eq!(sem.scopes.len(), 1);
    }

    #[test]
    fn duplicate_declaration_is_error() {
        let sem = analyze_src("let a; let a;");
        assert_eq!(
            sem.errors,
            vec![SemanticError::DuplicateDeclaration {
                name: "a".to_string(),
                span: Node::new(11, 12),
            }]
        );
        // 最初の宣言だけが残る。
        assert_eq!(sem.symbols.len(), 1);
    }

    #[test]
    fn reference_resolves_to_declaration() {
        // let x = 1; x;
        let sem = analyze_src("let x = 1; x;");
        assert_eq!(sem.references.len(), 1);
        let r = &sem.references[0];
        assert_eq!(r.name, "x");
        let resolved = r.resolved.expect("x should resolve");
        assert_eq!(sem.symbol(resolved).name, "x");
    }

    #[test]
    fn undeclared_reference_is_unresolved() {
        // 宣言が無い名前は解決されない (JS ではグローバル参照になるのでエラーにはしない)。
        let sem = analyze_src("foo;");
        assert_eq!(sem.references.len(), 1);
        assert_eq!(sem.references[0].resolved, None);
    }

    #[test]
    fn block_creates_child_scope() {
        // ブロック内の宣言はブロックスコープに閉じる。
        let sem = analyze_src("{ let a; }");
        assert_eq!(sem.scopes.len(), 2); // Top + Block
        assert_eq!(sem.scopes[1].kind, ScopeKind::Block);
        assert_eq!(sem.scopes[1].parent, Some(ScopeId(0)));
        // a はブロックスコープ (id 1) に属する。
        assert_eq!(sem.symbols[0].scope, ScopeId(1));
    }

    #[test]
    fn inner_scope_can_reference_outer() {
        // 外側の宣言を内側ブロックから参照できる (親チェーン解決)。
        let sem = analyze_src("let a = 1; { a; }");
        let r = sem.references.iter().find(|r| r.name == "a").unwrap();
        let resolved = r.resolved.expect("a should resolve to outer declaration");
        assert_eq!(sem.symbol(resolved).scope, ScopeId(0)); // 最上位
    }

    #[test]
    fn shadowing_resolves_to_inner() {
        // 内側の同名宣言が外側を隠す。
        let sem = analyze_src("let a = 1; { let a = 2; a; }");
        // シンボルは 2 つ (外 a / 内 a)。重複エラーは出ない (別スコープ)。
        assert_eq!(sem.symbols.len(), 2);
        assert!(sem.errors.is_empty());
        let r = sem.references.iter().find(|r| r.name == "a").unwrap();
        let resolved = r.resolved.unwrap();
        // 内側ブロックスコープ (id 1) の a に解決される。
        assert_eq!(sem.symbol(resolved).scope, ScopeId(1));
    }

    #[test]
    fn same_name_in_sibling_scopes_is_ok() {
        let sem = analyze_src("{ let a; } { let a; }");
        assert_eq!(sem.symbols.len(), 2);
        assert!(sem.errors.is_empty());
    }
}