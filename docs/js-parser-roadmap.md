# JS パーサー学習メモ (javascript-parser-in-rust) ✅完了

> **【完了】2026-06-28: チュートリアル本文 (overview〜typescript) を全章読了、**
> **JS subset パーサー (lexer / ast / parser / errors核 / semantic) を実装済み。**
> typescript 章は概念読了のみ (発展なので実装はしない方針どおり未着手)。
> miette / arena / interning は方針どおり本体には組み込まず (下記「注意点」参照)。
> 次の学習: oxc の Architecture / ECMAScript / Performance / 本体ソース
> → 読書計画は [oxc-reading-plan.md](./oxc-reading-plan.md)。

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

| 元チュートリアル章 | 作る/育てるファイル | 状態 | メモ |
|---|---|---|---|
| overview | — | ✅読了 | 概念のみ (読むだけ) |
| lexer | `js/lexer.rs` | ✅実装 | `Chars` + offset / peek / keyword / `TokenValue`。interning は方針どおり未組込 |
| ast | `js/ast.rs` | ✅実装 | `Node` / estree / `Box` / enum サイズ。serde / arena は方針どおり未組込 |
| parser | `js/parser.rs` | ✅実装 | 再帰下降 + Pratt (式) |
| errors | `js/parser.rs` | ✅核実装 | `Result` / `expect` / `SyntaxError` (手書き Error)。miette は未組込 |
| semantic_analysis | `js/semantic.rs` | ✅実装 | scope tree + visitor。解説: [semantic-analysis.md](./semantic-analysis.md) (toy 版含む) |
| typescript | (未作成) | 📖読了のみ | lookahead / arrow backtracking。発展なので実装は見送り |
| references | — | ✅読了 | 読書のみ |

## 注意点 (チュートリアルそのままで OK な箇所)

- **arena / string interning は本体に組み込まない**。ast 章の arena (`bumpalo`) と lexer 章の interning
  (`string-cache`) は「こういう最適化もある」という紹介スニペットのみ。チュートリアル本体は Box 版・
  `String` 版で進む (ast 章に「次の章のコードはメモリアリーナを示していません」の注記あり)。
  → やりたければ後で別途。toy リポの `ast_arena.rs` / `atom.rs` に実装済みなのでそちらを参照。
- typescript 章は難所。概念を浴びるだけで十分。
