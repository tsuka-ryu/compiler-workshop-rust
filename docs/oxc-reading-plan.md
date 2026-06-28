# oxc 読書計画 (Architecture / ECMAScript / Performance / 本体)

oxc の学習ドキュメントと本体ソースを読むための計画。

## 方針

- oxc は巨大なので**通読しない**。**自作 `src/js/` に対応する所を軸に**拾い読みする。
- 素材はほぼ `oxc-project/website/src/docs/learn/` に揃っている (`parser_in_rust/` は読了済み)。
- パスは 3 リポジトリにまたがる:
  - 解説サイト: `oxc-project/website/src/docs/learn/`
  - 本体ソース: `oxc-project/oxc/`
  - 自作 + 前提: このリポ ([docs/reading-oxc.md](./reading-oxc.md))

読む順は **概念 (Phase 1-2) → 設計 (Phase 3) → 実装 (Phase 4-5)** が自然。

---

## Phase 0 — 前提

- [ ] [docs/reading-oxc.md](./reading-oxc.md) — oxc 固有の事情 (コード生成・独自 Box/Vec)
- [ ] `website/.../learn/terminology.md` — 用語集 (読み進める前の足場)
- [ ] 心構え: `oxc/crates/*/src/generated/` は**自動生成**。手で追わない

---

## Phase 1 — ECMAScript (概念の土台)

仕様まわり。なぜパーサーが面倒なのかの根っこ。

- [ ] `website/.../learn/ecmascript/grammar.md` — 文法チュートリアル
  - 見どころ: **Cover Grammar** / **ASI (自動セミコロン挿入)** / `let` の文脈依存 / `[Yield][Await]`
  - 対応: 自作 parser.rs の `eat(Kind::Semicolon)` (セミコロン省略許容) の背景、typescript 章の cover grammar
  - 注: javascript-parser-in-rust の `blog/grammar.md` と同内容。どちらで読んでも可
- [ ] `website/.../learn/ecmascript/spec.md` — ECMAScript 仕様書の読み方ガイド

---

## Phase 2 — Performance

oxc が速い理由の哲学。自分が toy/JS で測った最適化の動機がここに集約。

- [ ] `website/.../learn/performance.md`
  - 見どころ: 「**メモリ割当を減らす / CPU サイクルを減らす**」の 2 原則
  - 対応: toy `ast_arena.rs` (arena) / `atom.rs` (interning) / ast.rs の `no_bloat_enum_sizes` (enum サイズ)

---

## Phase 3 — Architecture 全体

設計ドキュメント 4 本 + プロジェクト全体。

- [ ] `website/.../learn/architecture/parser.md` (177 行) — パーサー設計
  - 見どころ: `BindingIdentifier` vs `IdentifierReference` / `Arena` / `String Interning` /
    `Two-Phase Design` / `Error Handling Philosophy`
  - 対応: 自作 ast.rs の別型分け、parser.rs の二段構え、`SyntaxError`
- [ ] `website/.../learn/architecture/ast-tools.md` (50 行) — AST のコード生成
  - → `generated/` の正体。なぜ手書きしないか
- [ ] `website/.../learn/architecture/linter.md` (102 行) — リンター設計 (visitor の実応用)
  - 対応: toy `lint.rs` / visit.rs
- [ ] `website/.../learn/architecture/test.md` (158 行) — テスト戦略 (conformance / snapshot)
  - 対応: toy の `insta` snapshot、blog `conformance.md` (test262)
- [ ] `oxc/ARCHITECTURE.md` (394 行) — プロジェクト全体
  - parser 関連章: `System Overview` / `Zero-Copy Architecture` / `Visitor Pattern` / `Foundation Layer`

---

## Phase 4 — 実ソース (parser 本体, 自作と対応づけ)

`oxc/crates/oxc_parser/src/` を、自作の所に絞って読む。ここが本番。

### 4a. Lexer ↔ `src/js/lexer.rs`
- [ ] `lexer/mod.rs` / `lexer/token.rs` / `lexer/source.rs` — 本体・`Token`/`Kind`・位置管理 (`offset`/`peek`)
- [ ] `lexer/identifier.rs` / `lexer/unicode.rs` — 識別子・Unicode (`is_id_start`)
- [ ] `lexer/whitespace.rs` / `lexer/comment.rs` — trivia (`skip_trivia`)
- [ ] (任意) `lexer/byte_handlers.rs` — バイト先頭分岐の最適化
- 見どころ: 複数トークンのバッファ / lookahead (typescript 章の話の実物)

### 4b. Cursor ↔ parser.rs のヘルパ
- [ ] `cursor.rs` — `at`/`eat`/`bump`/`expect`/lookahead/checkpoint・rewind
- [ ] `context.rs` / `state.rs` — `[Yield]`/`[Await]` 文法コンテキスト (semantic 章で省いた所)

### 4c. 再帰下降 + 式 ↔ parser.rs
- [ ] `js/statement.rs` — 文 (`parse_statement`/`parse_block_statement`)
- [ ] `js/declaration.rs` — 変数宣言 (`parse_variable_declaration`)
- [ ] `js/expression.rs` + `js/operator.rs` — 式・優先順位 (Pratt `binary_binding_power`)

### 4d. エラー処理 ↔ `SyntaxError`
- [ ] `error_handler.rs` — recoverable / fatal の 2 層 (toy `parse_resilient.rs` で写経)
- [ ] `diagnostics.rs` — 診断の作り方

### 4e. Cover Grammar / アロー (発展) ↔ Phase 1 grammar + typescript 章
- [ ] `js/grammar.rs` — 式 → 束縛パターン変換
- [ ] `js/arrow.rs` — アローの曖昧性解決 (lookahead + backtracking, Tristate)

### 4f. TypeScript (発展) ↔ typescript 章
- [ ] `lexer/typescript.rs` / `ts/types.rs` / `ts/statement.rs`

---

## Phase 5 — AST / アロケータ (任意) ↔ toy `ast_arena.rs` / `atom.rs`

- [ ] `oxc/crates/oxc_allocator/src/boxed.rs` / `vec.rs` / `allocator.rs` — 独自 `Box<'a>`/`Vec<'a>`
- [ ] `oxc/crates/oxc_ast/` — AST 定義 (`ast/` の元定義。`generated/` は自動生成)

---

## 読み終えたら

- 自作 `src/js/` の各ファイル冒頭に「oxc 本体の対応ファイル」を 1 行足すと往復しやすい。
- 関連: [js-parser-roadmap.md](./js-parser-roadmap.md) / [semantic-analysis.md](./semantic-analysis.md)
