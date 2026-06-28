# JS パーサー学習メモ (javascript-parser-in-rust)

[oxc-project/javascript-parser-in-rust](https://github.com/oxc-project/javascript-parser-in-rust)
(日本語版あり) を**そのまま上から読んで実装する**。これは進捗ダッシュボードではなく、
置き場所と章↔ファイルの対応を決めただけのメモ。実装の手順はチュートリアル本文に従う。

旧 [next-steps.md](./next-steps.md) (toy 言語ツアー) は節11 まで実装した時点で放棄。

## 置き場所

JS パーサーは別サブプロジェクトなので `src/js/` に隔離する。`lib.rs` に `pub mod js;` を 1 行足すだけ
(既存 toy モジュールは無改造)。各章は**1 ファイルを育てる**方針で、最適化 (arena / interning / miette 等) も
新ファイルではなく既存ファイルへの編集として入れる ← チュートリアル自身がそうしているため。

```
src/js/
  mod.rs        ← 各 mod を登録
  lexer.rs      ← lexer 章
  ast.rs        ← ast 章
  parser.rs     ← parser 章 + errors 章
  semantic.rs   ← semantic_analysis 章
  ts.rs         ← typescript 章 (発展。やらないなら無し)
```

## 章 ↔ ファイル対応

| 元チュートリアル章 | 作る/育てるファイル | メモ |
|---|---|---|
| overview | — | 概念のみ (読むだけ) |
| lexer | `js/lexer.rs` | `Chars` + offset / peek / keyword / `TokenValue`。interning はここに編集で |
| ast | `js/ast.rs` | `Node` / estree / `Box` / enum サイズ / serde はここに編集で |
| parser | `js/parser.rs` | 再帰下降 + Pratt (式) |
| errors | `js/parser.rs` | `Result` / `expect` / `SyntaxError` / miette をパーサーに編集で |
| semantic_analysis | `js/semantic.rs` | scope tree + visitor。解説: [semantic-analysis.md](./semantic-analysis.md) (toy 版含む) |
| typescript | `js/ts.rs` | lookahead / arrow backtracking (発展、深追いしない) |
| references | — | 読書のみ |

## 注意点 (チュートリアルそのままで OK な箇所)

- **arena / string interning は本体に組み込まない**。ast 章の arena (`bumpalo`) と lexer 章の interning
  (`string-cache`) は「こういう最適化もある」という紹介スニペットのみ。チュートリアル本体は Box 版・
  `String` 版で進む (ast 章に「次の章のコードはメモリアリーナを示していません」の注記あり)。
  → やりたければ後で別途。toy リポの `ast_arena.rs` / `atom.rs` に実装済みなのでそちらを参照。
- typescript 章は難所。概念を浴びるだけで十分。
