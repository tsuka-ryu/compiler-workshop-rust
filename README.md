# compiler-workshop-rust

[Richard Feldman](https://github.com/rtfeldman) 氏の Frontend Masters コース [Building a Static Type-Inferring Compiler](https://frontendmasters.com/courses/type-compiler/) ([rtfeldman/compiler-workshop-v1](https://github.com/rtfeldman/compiler-workshop-v1)) を Rust で再実装した個人学習用リポジトリです。

オリジナルは TypeScript / Node.js で書かれていますが、Rust の練習を兼ねて同じ題材を Rust で書き直しています。

## 構成

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

## カバー範囲

オリジナル ([solutions/7](https://github.com/rtfeldman/compiler-workshop-v1/tree/main/solutions/7)) と同等の機能 + さらに JS 版にはない多相対応を実装:

- ✅ tokenize
- ✅ parse (アロー関数、型注釈、配列、三項演算子、メンバアクセスなど)
- ✅ naming (レキシカルスコープ、自由変数キャプチャ)
- ✅ typecheck (関数型、Algorithm W 流の generalize / instantiate、環境を考慮した Let-多相)
- ✅ wasm 生成 (LEB128 エンコード、Memory / Data セクション、関数呼び出し、IF / ELSE など)
- ✅ monomorphization (JS 版にない: 多相関数を使用された型ごとに具体版へ展開)

### Let-多相の例

```js
const id = (x) => { return x; };
const a = id(5);       // Number
const b = id("hi");    // String  ← 同じ id を異なる型で使える
```

Algorithm W 流の generalize / instantiate を実装しているため、こうしたパターンが正しく型推論できます。

### Monomorphization の例

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

## ドキュメント

- [docs/wasm-plan.md](docs/wasm-plan.md) — wasm 生成と monomorphization の設計方針
- [docs/next-steps.md](docs/next-steps.md) — 今後の学習ロードマップ (Pratt parser、Resilient parsing、Span、Result、codespan-reporting、Linter、Formatter、LSP)

## 参考

- 元リポジトリ: <https://github.com/rtfeldman/compiler-workshop-v1>
- コース: <https://frontendmasters.com/courses/type-compiler/>
- Algorithm W (参考実装): [drgon8 の Haskell 版](https://github.com/tsukaryu/algorighm-w-copy) など