# compiler-workshop-rust

コンパイラまわりの個人学習リポジトリです。コードもメモも雑多に置いています。

もともとは [Richard Feldman](https://github.com/rtfeldman) 氏の Frontend Masters コース [Building a Static Type-Inferring Compiler](https://frontendmasters.com/courses/type-compiler/) ([rtfeldman/compiler-workshop-v1](https://github.com/rtfeldman/compiler-workshop-v1)) を Rust で再実装するリポジトリでした。いまはそこから広がり、次のものが同居しています。

| 何があるか | 場所 |
| --- | --- |
| ワークショップの Rust 再実装 (tokenize → parse → 型推論 → wasm) | [src/](src/) と [下のセクション](#ワークショップの再実装) |
| その先の toy 実装 (Pratt / resilient parsing / arena / visitor / linter など) | [src/](src/) と [docs/](docs/) |
| JS パーサーの実装 | [src/js/](src/js/) |
| oxc / tsc のコードリーディングのメモ | [docs/](docs/) と [demos/](demos/) |

## ワークショップの再実装

### 構成

```
Source code (JS-like)
   ↓ tokenize
Tokens
   ↓ parse
AST
   ↓ naming + typecheck + monomorphize
全部単相な AST
   ↓ wasm code generation
WebAssembly バイナリ
```

### ファイル

- [src/tokenize.rs](src/tokenize.rs) — 字句解析 (lexer)
- [src/parse.rs](src/parse.rs) — 構文解析 (parser)
- [src/naming.rs](src/naming.rs) — 名前解決 (scope / 未宣言・重複宣言チェック)
- [src/typecheck.rs](src/typecheck.rs) — Hindley-Milner 型推論 (Algorithm W 流の Let-多相つき)
- [src/typecheck_mono.rs](src/typecheck_mono.rs) — typecheck.rs を拡張し、wasm 向けに resolve 済み型情報を取り出せるようにしたもの (比較学習用に別ファイル)
- [src/monomorphize.rs](src/monomorphize.rs) — 多相関数の単相化 (specialization 収集 + AST 書き換え + エイリアス解決)
- [src/wasm.rs](src/wasm.rs) — WebAssembly コード生成
- [src/lib.rs](src/lib.rs) — `compile` / `compile_full` / `compile_to_wasm_full` エントリポイント

### カバー範囲

オリジナル ([solutions/7](https://github.com/rtfeldman/compiler-workshop-v1/tree/main/solutions/7)) と同等の機能 + さらに JS 版にはない多相対応を実装:

- ✅ tokenize
- ✅ parse (アロー関数、型注釈、配列、三項演算子、メンバアクセスなど)
- ✅ naming (レキシカルスコープ、自由変数キャプチャ)
- ✅ typecheck (関数型、Algorithm W 流の generalize / instantiate、環境を考慮した Let-多相)
- ✅ wasm 生成 (LEB128 エンコード、Memory / Data セクション、関数呼び出し、IF / ELSE など)
- ✅ monomorphization (JS 版にない: 多相関数を使用された型ごとに具体版へ展開)

#### Let-多相の例

```js
const id = (x) => { return x; };
const a = id(5);       // Number
const b = id("hi");    // String  ← 同じ id を異なる型で使える
```

Algorithm W 流の generalize / instantiate を実装しているため、こうしたパターンが正しく型推論できます。

#### Monomorphization の例

WebAssembly は多相型を直接表現できないため、上記の `id` を wasm に落とす際には:

```text
const id_Number = (x: number): number => { return x; };
const id_String = (x: string): string => { return x; };
const a = id_Number(5);
const b = id_String("hi");
```

のように、使用された型ごとに別関数として展開します。エイリアス (`const f2 = id;`) も解決されて呼び出しが書き換えられます。

## 実行

```sh
cargo test
```

## Snapshot testing (insta)

parser 系のテストの一部は [insta](https://insta.rs/) で snapshot 化している ([tests/parse_snapshots.rs](tests/parse_snapshots.rs))。

### 初回セットアップ

```sh
cargo install cargo-insta
```

### 普段のワークフロー

```sh
cargo test --test parse_snapshots   # snapshot と差分があれば fail する
cargo insta review                  # TUI で diff を確認: a で承認 / r で却下 / s でスキップ
```

承認すると `tests/snapshots/*.snap` が更新される (git にコミットする)。

### 新しい snapshot テストを追加するとき

```rust
insta::assert_debug_snapshot!(value);
```

を書いて `cargo test` → 初回は `.snap.new` が生成されて fail → `cargo insta review` で承認 → `.snap` 確定。

### AST の構造を意図的に変えたとき

テストが fail する → `cargo insta review` で新しい AST 出力を確認 → 期待通りなら `a` で承認 → snapshot 更新。逆に意図しない変化なら `r` で却下してコードを直す。

## ワークショップの先

ワークショップ後に足した toy 実装は [src/](src/) に `parse_pratt.rs` や `parse_resilient.rs` のような単体ファイルで置いてあり、解説を [docs/](docs/) に書いています。

- [docs/next-steps.md](docs/next-steps.md): toy 言語の技術ツアー(節 11 まで実装して打ち切り)
  - [parse-pratt.md](docs/parse-pratt.md): Pratt parser
  - [parse-resilient.md](docs/parse-resilient.md): Resilient parsing
  - [arena-allocator.md](docs/arena-allocator.md): arena allocator
  - [string-interning.md](docs/string-interning.md): String interning
  - [index-types.md](docs/index-types.md): Index 型
  - [linter.md](docs/linter.md): Linter
  - [visit.md](docs/visit.md) / [traverse-binary-only.md](docs/traverse-binary-only.md): Visitor / Traverse
  - [semantic-analysis.md](docs/semantic-analysis.md): 意味解析
- [docs/wasm-plan.md](docs/wasm-plan.md): wasm 生成と monomorphization の設計方針
- [docs/js-parser-roadmap.md](docs/js-parser-roadmap.md): JS パーサー学習(完了)。実装は [src/js/](src/js/)

## 読書メモ

### oxc

- [reading-oxc.md](docs/reading-oxc.md): 読む前の前提知識
- [oxc-reading-plan.md](docs/oxc-reading-plan.md): 読書計画
- [reading-oxc-ts.md](docs/reading-oxc-ts.md): TypeScript パーサーの深読みガイド
- [reading-oxc-ts-notes.md](docs/reading-oxc-ts-notes.md): 上の読書メモ。[demos/](demos/) に実証用のデモがある
- [lt-oxc-ts-parser.md](docs/lt-oxc-ts-parser.md): LT 用のメモ
- [oxc-issue-as-exponent.md](docs/oxc-issue-as-exponent.md): `2 ** 3 as number ** 2` の issue メモ

### tsc の健全性

- [reading-tsc-soundness-plan.md](docs/reading-tsc-soundness-plan.md): 読書計画
- [reading-tsc-soundness-notes.md](docs/reading-tsc-soundness-notes.md): 読書メモ

## 参考

- 元リポジトリ: <https://github.com/rtfeldman/compiler-workshop-v1>
- コース: <https://frontendmasters.com/courses/type-compiler/>
- Algorithm W (参考実装): [drgon8 の Haskell 版](https://github.com/tsukaryu/algorighm-w-copy) など