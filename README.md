# compiler-workshop-rust

[Richard Feldman](https://github.com/rtfeldman) 氏の Frontend Masters コース [Building a Static Type-Inferring Compiler](https://frontendmasters.com/courses/type-compiler/) ([rtfeldman/compiler-workshop-v1](https://github.com/rtfeldman/compiler-workshop-v1)) を Rust で再実装した個人学習用リポジトリです。

オリジナルは TypeScript / Node.js で書かれていますが、Rust の練習を兼ねて同じ題材を Rust で書き直しています。

## 構成

- [src/tokenize.rs](src/tokenize.rs) — 字句解析 (lexer)
- [src/parse.rs](src/parse.rs) — 構文解析 (parser)
- [src/naming.rs](src/naming.rs) — 名前解決 (scope/未宣言・重複宣言チェック)
- [src/typecheck.rs](src/typecheck.rs) — Hindley-Milner 型推論 (Algorithm W 流の Let-多相つき)
- [src/lib.rs](src/lib.rs) — `compile` / `compile_full` エントリポイント
- [src/main.rs](src/main.rs) — テスト用エントリ

## カバー範囲

オリジナル ([solutions/7](https://github.com/rtfeldman/compiler-workshop-v1/tree/main/solutions/7)) の WebAssembly 生成以外をカバー：

- ✅ tokenize
- ✅ parse (アロー関数、型注釈、配列、三項演算子、メンバアクセスなど)
- ✅ naming (レキシカルスコープ、自由変数キャプチャ)
- ✅ typecheck (関数型、Algorithm W 流の generalize / instantiate、環境を考慮した Let-多相)
- ❌ wasm 生成

### Let-多相の例

```js
const id = (x) => { return x; };
const a = id(5);       // Number
const b = id("hi");    // String  ← 同じ id を異なる型で使える
```

オリジナルの JS 版にはない、Algorithm W 流の generalize / instantiate を実装しているため、こうしたパターンが正しく型推論できます。

## 実行

```sh
cargo test
```

## 参考

- 元リポジトリ: <https://github.com/rtfeldman/compiler-workshop-v1>
- コース: <https://frontendmasters.com/courses/type-compiler/>