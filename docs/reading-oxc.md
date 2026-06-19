# oxc のソースを読むときの前提知識

[next-steps.md](./next-steps.md) は「概念」と「どの crate が何に対応するか」を扱う。
こちらは別軸で、**oxc の実ソースを開いたときに面食らう、コードベース固有の事情**をまとめる。
ロードマップの各節の「読書 TODO」を実際にやる前に、ここを通しておくと迷子になりにくい。

優先度をつけるなら **1(自動生成)と 2(独自 Box/Vec)だけは読み始める前に必ず**頭に入れる。
この 2 つを知らないと `generated/visit.rs` を「手書きの謎コード」として読み込んで時間を溶かす。

---

## 1. コードの大部分は「自動生成」されている(最重要)

`crates/*/src/generated/` 配下は手書きではなく [`tasks/ast_tools`](https://github.com/oxc-project/oxc/tree/main/tasks/ast_tools) が生成している。

- `crates/oxc_ast/src/generated/` … `ast_builder.rs` / `ast_kind.rs` / `derive_clone_in.rs` / `derive_get_span.rs` など
- `crates/oxc_ast_visit/src/generated/visit.rs` … **Visit / VisitMut 本体は生成物**

next-steps.md 節 5 で手書きした `walk_statement` 相当を、oxc では**マクロが AST 定義から吐いている**。
読むべき「真実のソース」は 2 つに分かれる:

- AST 定義: `crates/oxc_ast/src/ast/js.rs` の `#[ast(visit)]` 付き型定義
- 生成ロジック: `tasks/ast_tools`

`generated/` を直接読んで「なぜこう書いてあるのか」を探しても答えはない。生成器側を見る。

## 2. `Box` / `Vec` / `String` は std ではない

`crates/oxc_allocator/src/lib.rs` が独自型を再エクスポートしている。AST 定義の冒頭はこうなる:

```rust
use oxc_allocator::{Box, CloneIn, Dummy, GetAddress, TakeIn, Vec};
```

- `Box<'a, T>` … arena 上の box(`std::boxed::Box` ではない、ライフタイム付き)
- `Vec<'a, T>` … arena 上の Vec(`Vec<'a, Directive<'a>>` のように `'a` が前に来る)
- `String` / `HashMap` も arena 版

next-steps.md 節 8・9 で予習する `&'a T` / `Atom<'a>` の話が、実コードでは**全コレクション型に及んでいる**。
`use` を見落とすと std と勘違いする。

## 3. `#[ast]` マクロと derive 群の語彙

AST 型には大量の derive がぶら下がる。意味を知らないと型定義が読めない:

- `CloneIn` … arena をまたぐ clone
- `GetSpan` / `GetSpanMut` … 節 1 Span の取得 trait(生成)
- `ContentEq` … span を無視した構造比較
- `Dummy` / `TakeIn` … 節 7 で予習した `Dummy` の本物。arena 版
- `GetAddress` … ポインタ同一性(節 10 の index 型とは別系統の id)
- `ESTree` … JSON シリアライズ(`derive_estree.rs`)

## 4. Lexer は手書き・バイト単位・SIMD

next-steps.md は `char_indices` 前提のモデルだが、`crates/oxc_parser` の lexer は
**`&[u8]` ベースの手書き**で、一部 SIMD。
「tokenize は char を回す」という workshop のメンタルモデルのままだと読めない。

## 5. 入口になるドキュメントが repo 内にある

crate 一覧の前に、oxc 自身の読書ガイドを見る:

- `ARCHITECTURE.md`
- `CLAUDE.md` / `AGENTS.md`

## 6. `tasks/` は crate ではないが本質的

`tasks/` には以下が入る。crate 対応表だけ見ているとテストとコード生成の在処が分からない:

- `ast_tools` … コード生成(1 の実体)
- `coverage` … test262 等。節 4 で触れた「corpus pass/fail を snapshot で固定する」方式の実体
- `benchmark`
- `*_conformance`(prettier / transform など)