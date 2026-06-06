# Visitor パターンと `src/visit.rs` の実装解説

## Visitor パターンの本質

> **データ構造を歩く処理（walk）と、歩いたついでに何かする処理（visit）を分離する**

これだけ。Linter / Formatter / Transformer など、AST を入力に取るツールの土台になるパターン。

### インプットは AST

Visitor の入力は AST（抽象構文木）。このリポジトリでは `parse_result` が返す `Vec<Statement>` がそれにあたる。

```
ソースコード文字列  "const x = a + b * c;"
       ↓ tokenize_span
トークン列         [const, x, =, a, +, b, *, c, ;]
       ↓ parse_result
AST                Vec<Statement>     ← Visitor のインプット
       ↓ Visitor （src/visit.rs）
集計結果           ["a", "b", "c"]
```

正確には「AST のうち Visitor が辿り始める起点ノード」が入力。トップレベルの `Vec<Statement>` 全体でもいいし、特定の式1つでもいい。

階層構造を持つデータならどれにでも適用できる汎用パターンだが、コンパイラの文脈では AST が主役。

## なぜこのパターンが必要か

AST に対する処理（識別子を集める、型名を集める、関数呼び出しを数える、コードを書き換える、等）を**用途ごとに**書きたい。

もし Visitor パターンがないと、用途ごとに以下を全部書き直す必要がある：

- AST の全ノード種別を網羅する `match`
- 子要素への再帰呼び出し
- 子なしノード（リーフ）の終端処理

これは退屈で、書き間違えると「特定の子を辿り忘れる」バグになる。

Visitor パターンを使うと、**巡回ロジック（`walk_*`）はライブラリ側に一本化**して、**用途固有のロジック（`visit_*` の上書き）だけ**を各利用者が書けばよくなる。

## `src/visit.rs` の実装

このリポジトリの `Visit` トレイト ([src/visit.rs:3-16](../src/visit.rs#L3-L16)) は、AST のノード種別ごとに `visit_*` メソッドを用意している。

```rust
pub trait Visit {
    fn visit_statement(&mut self, stmt: &Statement) {
        walk_statement(self, stmt);
    }
    fn visit_expression(&mut self, expr: &Expression) {
        walk_expression(self, expr);
    }
    fn visit_type_annotation(&mut self, ty: &TypeAnnotation) {
        walk_type_annotation(self, ty);
    }
    fn visit_parameter(&mut self, param: &Parameter) {
        walk_parameter(self, param);
    }
}
```

ポイントは **デフォルト実装が `walk_*` を呼ぶだけ** ということ。利用者は興味があるメソッドだけ上書きすればよく、上書きしなかったメソッドは「何もせずに子を辿るだけ」の挙動になる。

### `walk_*` 関数

各ノード種別ごとに、子要素を `visit_*` で再帰的に辿る関数を用意している。例として `walk_expression` ([src/visit.rs:38-94](../src/visit.rs#L38-L94)) は `Expression` の各バリアントについて子を辿る：

- `Number` / `String` / `Boolean` / `Identifier` などの**子なしノードは何もしない**
- `Binary { left, right, .. }` は `left` と `right` をそれぞれ `visit_expression` で辿る
- `ArrowFunction { params, return_type, body, .. }` は `visit_parameter` / `visit_type_annotation` / `visit_statement` を順に呼ぶ

`walk_*` のシグネチャは `V: Visit + ?Sized` というジェネリクスで、任意の Visit 実装に対応する。

### `visit` と `walk` の相互再帰

ここが Visitor パターンの一番つまずきやすい部分。実態は単純な相互再帰：

```
visit_expression (デフォルト)
    └→ walk_expression を呼ぶ
            └→ 子に対して visit_expression を呼ぶ
                    └→ walk_expression を呼ぶ
                            └→ ...
```

デフォルトのままだと「全部辿るだけで何もしない」処理になる。

ここで `visit_expression` を**上書き**すると：

```
visit_expression (上書き版)
    ├→ 識別子ならメモする        ← 自分の仕事を挟む
    └→ walk_expression を呼ぶ    ← 巡回は続ける（呼び忘れると子が辿られない）
            └→ 子に対して visit_expression を呼ぶ
                    ↑
                    self なので、ここでもまた上書き版が呼ばれる
```

**「自分の処理を挟みつつ、再帰は walk に任せる」**。これが Visitor パターンの動作の核。

## 使用例：`IdentifierCollector`

[src/visit.rs:127-148](../src/visit.rs#L127-L148) に、AST から識別子を収集する Visitor の実例がある。

```rust
struct IdentifierCollector {
    names: Vec<String>,
}

impl Visit for IdentifierCollector {
    fn visit_expression(&mut self, expr: &Expression) {
        if let Expression::Identifier { name, .. } = expr {
            self.names.push(name.clone());
        }
        walk_expression(self, expr);
    }
}
```

- **状態を持つ Visitor**: `names: Vec<String>` に結果を蓄積する。Visitor 自身が状態を持つことで、AST を辿りながら情報を集められる。
- **`visit_expression` だけを上書き**: 他の `visit_*` メソッドは書いていないので、デフォルト実装（`walk_*` を呼ぶだけ）が使われる。識別子は `Expression` のバリアントなので、`visit_expression` だけ書けば十分。
- **`if let` で識別子だけ拾う**: `Expression::Identifier { name, .. }` パターンにマッチしたときだけ push。他のバリアント（`Number`, `Binary`, ...）は無視。
- **最後に `walk_expression(self, expr)` を呼ぶ**: これを呼ばないと子が辿られない。例えば `a + b` の場合、自分は `Identifier` ではないので push されないが、`walk_expression` を呼ぶことで左右の子（`a`, `b`）について改めて `visit_expression` が呼ばれ、そこで収集される。

### トラバースの流れ

`const x = a + b * c;` を `IdentifierCollector` で処理する流れ：

```
visit_statement(ConstDecl)
  └ walk_statement
      └ visit_expression( a+(b*c) )           [Binary なのでスルー]
          └ walk_expression
              ├ visit_expression( a )         ★ "a" を push
              │   └ walk_expression           [Identifier は子なし]
              └ visit_expression( b*c )       [Binary なのでスルー]
                  └ walk_expression
                      ├ visit_expression( b ) ★ "b" を push
                      └ visit_expression( c ) ★ "c" を push
```

注意点として、`const x = ...` の `x` は `Statement::ConstDeclaration` の名前フィールドであって `Expression::Identifier` ではないため収集されない。期待値が `["a", "b", "c"]` で `x` を含まないのはそのため。

## 「上書き」という概念について

Rust のトレイトには**デフォルト実装**を書ける。`Visit` トレイトでは `visit_*` の中身が「`walk_*` を呼ぶだけ」というデフォルト実装になっている ([src/visit.rs:3-16](../src/visit.rs#L3-L16))。

`impl Visit for SomeType` で `visit_expression` を書くと、その型ではデフォルトではなく**自分で書いた版**が使われる。これが「上書き（オーバーライド）」。

`IdentifierCollector` の場合：

| メソッド | 上書きの有無 | 実際に呼ばれる実装 |
|---|---|---|
| `visit_statement` | していない | デフォルト（`walk_statement` を呼ぶだけ） |
| `visit_expression` | **している** | 自前版（識別子なら push して `walk_expression`） |
| `visit_type_annotation` | していない | デフォルト |
| `visit_parameter` | していない | デフォルト |

「全部のメソッドを書かなくていい、興味あるやつだけ書けばいい」というのがデフォルト実装の便利な点。

## 他の Visitor 例

このリポジトリには `TypeNameCollector` ([src/visit.rs:171-197](../src/visit.rs#L171-L197)) もある。これは `visit_type_annotation` を上書きして型名を集める Visitor。

```rust
struct TypeNameCollector {
    type_names: Vec<String>,
}

impl Visit for TypeNameCollector {
    fn visit_type_annotation(&mut self, ty: &TypeAnnotation) {
        if let TypeAnnotation::Named { name, .. } = ty {
            self.type_names.push(name.clone());
        }
        walk_type_annotation(self, ty);
    }
}
```

`IdentifierCollector` と全く同じ構造。**興味のあるノード種別の `visit_*` だけ上書きし、自分の処理を挟んだ後 `walk_*` を呼ぶ**というイディオムが、Visitor パターンの基本形。

## まとめ

1. **歩き方（walk）と歩きながらやること（visit）を分ける**
2. **歩きながらやることだけを各用途で書く。歩き方は使い回す**

`Visit` トレイトのデフォルト実装が「歩き方」を提供し、利用者は `impl Visit for MyType` で「やること」だけを書く。`visit_*` を上書きしたら、その中で `walk_*` を呼んで再帰を続けることだけ忘れない。これが基本パターン。

TypeScript の AST ライブラリ、Babel、Rust の `syn`、Go の `go/ast`、ESLint のルールなど、AST を扱うほぼ全てのツールがこのパターンを採用している。一度理解すれば多くのツールのソースが読めるようになる。