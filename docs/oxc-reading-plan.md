# oxc 読書計画

方針: website の Learn / guide ドキュメントは**通読しない**(重いので)。**コードを直接読む**。
詰まったら下の「参考ドキュメント」を該当箇所だけピンポイントで見る。

## 本体コードを読む — front-end 優先

`transformer` / `linter` / `formatter` / `minifier` / `codegen` は **AST 消費側なので後回し**。
まず front-end (lexer → ast → parser → semantic) を、依存の下層から読む。**TS / JSX も含める。**

クレート順 (下層 → 上層):

- [ ] `oxc_span` — Span / Atom (基盤、軽い)
- [ ] `oxc_allocator` — arena。読むのは `boxed.rs` / `vec.rs` / `allocator.rs` 中心。`allocator_api2` / `vec2` / `pool` は配管なので飛ばす
- [ ] `oxc_parser` — 本命
  - [ ] `lexer/` — source / kind / byte_handlers / identifier / whitespace / comment
  - [ ] `cursor.rs` — at/eat/bump/expect/lookahead/checkpoint
  - [ ] `js/statement.rs` / `js/declaration.rs` / `js/expression.rs` / `js/operator.rs` — 再帰下降 + Pratt
  - [ ] `error_handler.rs` / `diagnostics.rs` — recoverable/fatal の 2 層
  - [ ] `js/grammar.rs` / `js/arrow.rs` — Cover Grammar / アロー曖昧性 (typescript 章の概念の実物)
  - [ ] `ts/types.rs` / `ts/statement.rs` / `lexer/typescript.rs` — TS (型パースが本丸・難所)
  - [ ] `jsx/mod.rs` / `lexer/jsx.rs` — JSX (レキサーが文脈で挙動を変える例)
- [ ] `oxc_ast` — AST 定義。`generated/` (42k) は自動生成なので飛ばし、`ast/` の元定義と builder の要点だけ
- [ ] `oxc_semantic` — scope / symbol / reference

### 規模の目安 (front-end フル: JS + TS + JSX)

| 範囲 | 実読量 | 目安 |
|---|---|---|
| span + allocator(主要) | ~5k | 3-5h |
| parser JS コア | ~10-12k | 10-15h |
| parser TS / JSX / 曖昧性 | ~3.3k | 6-10h (行数の割に重い) |
| ast (手書き要点) | ~3k | 2-3h |
| semantic | ~8k | 6-10h |
| **計** | **~31k** | **~27-40h (週末ペースで 5-7 週)** |

追い風: ① 自作 `src/js/` のミニ版を書いた後なので「本気版」を読む形で速い。
② 依存が DAG なので下層から積めば未知の型に詰まらない。

> 隣接: `js/module.rs` (import/export = ES Modules) は TS ではないが front-end の一部。読むなら +2-3h。

## Learn ドキュメント (任意・通読しない)

コード直読みが主。Learn は記録として残すが、**読むなら高価値の上 3 つだけ**ピンポイントで。
コードで迷ったとき該当箇所を見る使い方でよい。

高価値 (コードの地図になる):

- [ ] `architecture/parser.md` (177 行) — パーサー設計の地図
- [ ] `ecmascript/grammar.md` — Cover Grammar / ASI などの概念
- [ ] `performance.md` — arena / interning / enum サイズの動機
- [ ] このリポ [docs/reading-oxc.md](./reading-oxc.md) — oxc 固有の前提 (コード生成・独自 Box/Vec)

その他 (周辺・必要になれば):

- `architecture/linter.md` / `test.md` / `ast-tools.md` — 消費側 / テスト / コード生成
- `ecmascript/spec.md` — 仕様書の読み方
- `terminology.md` / `references.md` — 用語集 / リンク集
- `parser_in_rust/` — 読了済み
