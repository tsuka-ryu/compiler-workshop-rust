//! 抽象構文木 (AST)。ast 章。
//!
//! estree (<https://github.com/estree/estree>) を参考にしたノード定義。
//! Rust には継承がないので、各構造体に `Node` を持たせる「継承の代わりの合成」で
//! 開始/終了オフセットを表現する。
//!
//! 章では最適化として 2 つ紹介されているが、ロードマップ方針に従い本体には
//! **enum のボックス化だけ**を入れる:
//! - enum のサイズ削減: 各バリアントを `Box` でくるみ enum 自体は 16 バイトに保つ
//!   (`no_bloat_enum_sizes` テストで担保)。
//! - メモリアリーナ (`bumpalo`) / 文字列 interning (`Atom`) / serde は紹介スニペットのみ。
//!   本体は `Box` + `String` で進める (章の「次の章はアリーナを使わない」注記に従う)。

/// 任意の AST ノードの基本要素。ソース内の開始/終了オフセット (UTF-8 バイト)。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    /// ソース内の開始オフセット
    pub start: usize,
    /// ソース内の終了オフセット
    pub end: usize,
}

impl Node {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// プログラム全体 (estree の `Program`)。
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub node: Node,
    pub body: Vec<Statement>,
}

// ---------------------------------------------------------------------------
// Statement
// ---------------------------------------------------------------------------

/// 文。多数のノード種に拡張されるので enum。
///
/// 各バリアントを `Box` でくるんで enum 自体は 16 バイト (ポインタ 8 + タグ) に保つ。
/// 200 バイト超の enum を `matches!` のたびにコピーするのを避けるための最適化。
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    VariableDeclaration(Box<VariableDeclaration>),
    ExpressionStatement(Box<ExpressionStatement>),
}

/// `var a`, `let b = 1`, `const c = 2` のような変数宣言。
#[derive(Debug, Clone, PartialEq)]
pub struct VariableDeclaration {
    pub node: Node,
    pub kind: VariableDeclarationKind,
    pub declarations: Vec<VariableDeclarator>,
}

/// `var` / `let` / `const`。estree の `VariableDeclaration.kind` に対応。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableDeclarationKind {
    Var,
    Let,
    Const,
}

/// 変数宣言子。`a = 1` のような 1 つの束縛。
#[derive(Debug, Clone, PartialEq)]
pub struct VariableDeclarator {
    pub node: Node,
    pub id: BindingIdentifier,
    pub init: Option<Expression>,
}

/// 式文。`foo;` のように式が 1 つの文になったもの。
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionStatement {
    pub node: Node,
    pub expression: Expression,
}

// ---------------------------------------------------------------------------
// Expression
// ---------------------------------------------------------------------------

/// 式。これも多数のノード種に拡張されるので enum。
///
/// Statement と同様に各バリアントを `Box` でくるんで 16 バイトに保つ。
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    NumberLiteral(Box<NumberLiteral>),
    StringLiteral(Box<StringLiteral>),
    Identifier(Box<IdentifierReference>),
    AwaitExpression(Box<AwaitExpression>),
    YieldExpression(Box<YieldExpression>),
}

/// 数値リテラル。`1`, `1.5` など。lexer に揃えて値は `f64`。
#[derive(Debug, Clone, PartialEq)]
pub struct NumberLiteral {
    pub node: Node,
    pub value: f64,
}

/// 文字列リテラル。`"foo"`, `'bar'` など。
#[derive(Debug, Clone, PartialEq)]
pub struct StringLiteral {
    pub node: Node,
    pub value: String,
}

/// 参照としての識別子 (estree では `Identifier`)。
///
/// 章の serde 例では `IdentifierReference` と `BindingIdentifier` を
/// estree 互換のため両方 `"Identifier"` に rename している。ここでは serde を
/// 入れないので Rust 上は別の型として区別する。
#[derive(Debug, Clone, PartialEq)]
pub struct IdentifierReference {
    pub node: Node,
    pub name: String,
}

/// 束縛側の識別子 (estree では `Identifier`)。変数宣言の `id` などで使う。
#[derive(Debug, Clone, PartialEq)]
pub struct BindingIdentifier {
    pub node: Node,
    pub name: String,
}

/// `await expr`。章の自己参照の例 (`Box` が必要な理由) に対応。
#[derive(Debug, Clone, PartialEq)]
pub struct AwaitExpression {
    pub node: Node,
    pub expression: Expression,
}

/// `yield expr`。同上。
#[derive(Debug, Clone, PartialEq)]
pub struct YieldExpression {
    pub node: Node,
    pub expression: Expression,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    /// 章の "no bloat enum sizes" テスト。各バリアントをボックス化したので
    /// enum 自体はポインタ + タグの 16 バイトに収まる。
    #[test]
    fn no_bloat_enum_sizes() {
        assert_eq!(size_of::<Statement>(), 16);
        assert_eq!(size_of::<Expression>(), 16);
    }

    #[test]
    fn build_var_a() {
        // `var a` の AST を手で組む (ASTExplorer の acorn 出力に対応)。
        let decl = VariableDeclaration {
            node: Node::new(0, 5),
            kind: VariableDeclarationKind::Var,
            declarations: vec![VariableDeclarator {
                node: Node::new(4, 5),
                id: BindingIdentifier {
                    node: Node::new(4, 5),
                    name: "a".to_string(),
                },
                init: None,
            }],
        };
        let program = Program {
            node: Node::new(0, 5),
            body: vec![Statement::VariableDeclaration(Box::new(decl))],
        };
        assert_eq!(program.body.len(), 1);
    }
}