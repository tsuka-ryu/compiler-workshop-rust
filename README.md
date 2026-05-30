# compiler-workshop-rust

[Richard Feldman](https://github.com/rtfeldman) 氏の Frontend Masters コース [Building a Static Type-Inferring Compiler](https://frontendmasters.com/courses/type-compiler/) ([rtfeldman/compiler-workshop-v1](https://github.com/rtfeldman/compiler-workshop-v1)) を Rust で再実装した個人学習用リポジトリです。

オリジナルは TypeScript / Node.js で書かれていますが、Rust の練習を兼ねて同じ題材を Rust で書き直しています。

## 構成

- [src/tokenize.rs](src/tokenize.rs) — 字句解析 (lexer)
- [src/parse.rs](src/parse.rs) — 構文解析 (parser)
- [src/lib.rs](src/lib.rs) — `compile` エントリポイント
- [src/main.rs](src/main.rs) — テスト用エントリ

## 実行

```sh
cargo test
```

## 参考

- 元リポジトリ: <https://github.com/rtfeldman/compiler-workshop-v1>
- コース: <https://frontendmasters.com/courses/type-compiler/>