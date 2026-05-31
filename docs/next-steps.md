# 今後の予定

このリポジトリでさらに学習を進めるためのロードマップ。実用 parser / compiler の定番テクニックを一通り触る。各節の最後に、対応する [oxc](https://github.com/oxc-project/oxc) の crate を参考実装として挙げている。

## 方針: 並置スタイル

学習用リポジトリなので、**同じ機能の別実装はファイルをコピーして並置する**。既存ファイルを書き換えて一本化しない。

- 既に `typecheck.rs` と `typecheck_mono.rs` が並んでいるのと同じ思想
- recursive descent 版の `parse.rs` の隣に Pratt 版 `parse_pratt.rs`、resilient 版 `parse_resilient.rs`、arena 版 `parse_arena.rs` を並べる
- `naming.rs` の隣に index 型ベースの `naming_indexed.rs` を並べる
- `lib.rs` には全てのモジュールを並列で登録する
- 比較学習が目的なので、ファイル数が増えても良い

## 全体ロードマップ

依存関係に沿って並べた。Span / Result / diagnostics は他のすべての前提なので最優先。

1. **Span / 位置情報** — 全ての後段の前提
2. **Result ベースのエラー処理** — panic を排除
3. **codespan-reporting でエラー整形** — 1〜3 で診断基盤が揃う
4. **Visit / Traverse パターン** — Linter / Formatter / Transformer の前提
5. **Pratt parser** — 同じ AST を別方式で構築
6. **Resilient parsing** — 壊れたコードでも木を作る
7. **Arena allocator (実装ブランチ)** — `Box<T>` → `&'a T` への変換を体験
8. **Index 型 (NodeId / SymbolId / ScopeId)** — naming を index ベースに refactor
9. **Linter** — visit パターンの実応用
10. **Formatter (浅め)** — pretty-printer の基本だけ
11. **LSP (Phase 1 まで)** — JSON-RPC の往復が分かれば十分

### 参考: oxc の主要 crate との対応

| oxc crate | このリポジトリでの対応 |
|---|---|
| `oxc_span` | (1) Span |
| `oxc_diagnostics` (miette) | (2)(3) Result + codespan-reporting |
| `oxc_ast_visit` / `oxc_traverse` | (4) Visit/Traverse |
| `oxc_parser` (expression は precedence climbing) | (5) Pratt |
| `oxc_parser` (壊れたコード対応) | (6) Resilient |
| `oxc_allocator` (bumpalo) + `&'a` AST | (7) Arena |
| `oxc_semantic` (scope / symbol / reference の id 管理) | (8) Index 型 |
| `oxc_linter` | (9) Linter |
| `oxc_formatter` | (10) Formatter |
| `oxc_language_server` | (11) LSP |

---

## 1. Span / 位置情報

新規ファイル: `src/tokenize_span.rs`、`src/parse_span.rs`、`src/ast_span.rs`

ねらい: ソース上の位置情報を AST に付ける。エラーメッセージ、linter、formatter、LSP の基盤。**並置スタイル**で、既存 tokenize/parse をいじらず、`*_span.rs` を新規追加する。

### 最初のステップ

1. `Span` 型を定義
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub struct Span {
       pub start: usize,
       pub end: usize,
   }
   ```
2. `Token { kind: TokenKind, span: Span }` に変更 (これは `tokenize_span.rs` 内の独自 Token 型でよい)
3. tokenize で位置を追跡 (`char_indices` がすでに位置を返している)
4. parser でトークンの span を集約して AST ノードの span を作る
5. AST 各ノードに `span: Span` フィールドを追加 (これも `ast_span.rs` で独立した型として定義する)

### 学習ポイント

- 実用 parser の基本である位置情報の取り扱い
- AST ノードを value 型のままにするか `Box<NodeWithSpan>` にするかなどの設計判断
- oxc の `Span` (`oxc_span` crate) と同じ構造

---

## 2. Result ベースのエラー処理

新規ファイル: `src/parse_result.rs` (Span 版をベースにコピー)

ねらい: `panic!` を `Result` に置き換える。Rust の `?` 演算子の典型練習。

### 最初のステップ

1. `ParseError` 型を定義
   ```rust
   #[derive(Debug, PartialEq)]
   pub struct ParseError {
       pub message: String,
       pub span: Span,
   }
   ```
2. `parse` の戻り値を `Result<Vec<Statement>, ParseError>` に
3. **下から上に panic を削っていく** (`parse_primary` → `parse_binary` → `parse_statement`)
4. `?` で伝播
5. テストは `parse(...).unwrap()` で当面しのぐ

### 学習ポイント

- `Result` の `?` チェイン
- `From<TokenError> for ParseError` 等の `impl From` パターン
- パニック前提のコードをエラー伝播型に書き換える経験

---

## 3. codespan-reporting でエラー整形

新規依存: `codespan-reporting`。新規ファイル: `src/diagnostics.rs`

ねらい: Span + Result が揃った後の見栄え改善。ソース該当箇所をハイライトしたエラー表示にする。

### Before / After

Before:
```
ParseError: expected ';' at position 42
```

After:
```
error: expected ';'
  ┌─ input.txt:3:15
  │
3 │   const x = 1
  │              ^ expected ';' here
```

### 最初のステップ

1. `Cargo.toml` に `codespan-reporting` を追加
2. `ParseError` / `TypeError` / `NamingError` をまとめた `Diagnostic` 変換層を `src/diagnostics.rs` に用意
3. `Files` impl にソース文字列を渡して、`emit` で stderr に整形出力

### 学習ポイント

- 1 つのエラーに複数の span (`primary` + `secondary`) を付ける設計
- `Severity` (error / warning / note) を使い分ける
- 「機械可読 (LSP 等) と人間可読 (CLI) を同じデータから作る」発想
- oxc は miette を使うが API の発想はほぼ同じ

### 読書 TODO

- [ ] [codespan-reporting README](https://github.com/brendanzab/codespan) — 基本 API
- [ ] [ariadne](https://github.com/zesterer/ariadne) — もう一つの選択肢、見栄えが派手。設計の対比に
- [ ] [miette](https://github.com/zkat/miette) — oxc の採用しているもの。codespan との設計対比
- [ ] rustc のエラー (`rustc --explain E0308` など) を見て、どんな情報をどう並べてるか観察

---

## 4. Visit / Traverse パターン

新規ファイル: `src/visit.rs`、`src/visit_mut.rs`

ねらい: AST を走査する抽象を独立 trait として切り出す。Linter / Formatter / Transformer の共通基盤。oxc では [`oxc_ast_visit`](https://github.com/oxc-project/oxc/tree/main/crates/oxc_ast_visit) と [`oxc_traverse`](https://github.com/oxc-project/oxc/tree/main/crates/oxc_traverse) が独立 crate になっている。

### 最初のステップ

1. immutable visitor を `src/visit.rs` に
   ```rust
   pub trait Visit {
       fn visit_statement(&mut self, stmt: &Statement) { walk_statement(self, stmt); }
       fn visit_expression(&mut self, expr: &Expression) { walk_expression(self, expr); }
       // ... 各ノード種別
   }
   pub fn walk_statement<V: Visit + ?Sized>(v: &mut V, stmt: &Statement) {
       match stmt {
           Statement::Const { value, .. } => v.visit_expression(value),
           // ...
       }
   }
   ```
2. mutable visitor を `src/visit_mut.rs` に (`&mut Statement` 版)
3. 試しに「全 Identifier 名を集める」visitor を 1 つ書く
4. (発展) `oxc_traverse` 風に **parent stack** を持つ trait を試す

### 学習ポイント

- visitor pattern と walk 関数を分離する古典的設計
- immutable / mutable の trait を別物にする理由
- parent stack を持つ mutable traverse (oxc 特有) の重さ
- Linter / Formatter / Transformer がこの上に乗る発想

### 読書 TODO

- [ ] [oxc_ast_visit](https://github.com/oxc-project/oxc/tree/main/crates/oxc_ast_visit) — immutable visitor
- [ ] [oxc_traverse](https://github.com/oxc-project/oxc/tree/main/crates/oxc_traverse) — mutable + parent stack

---

## 5. Pratt parser

新規ファイル: `src/parse_pratt.rs`

ねらい: 既存 recursive descent との対比。同じ AST を別の方法で構築する。**recursive descent 版の `parse.rs` は残したまま**、`parse_pratt.rs` を並置する。

### 最初のステップ

1. `pub fn parse_pratt(tokens: Vec<Token>) -> Vec<Statement>` のスケルトン作成
2. `Statement` のパースは既存 parse.rs と同じ recursive descent で書く (`const` 宣言、`return` 文など)
3. **`parse_expression` だけ Pratt にする** (ここが本題)
4. binding power 表 (`infix_bp(&Token) -> Option<(left_bp, right_bp)>`) を用意
5. 既存テストの一部を `parse_pratt` 用に流用し、同じ結果になることを確認

### 実装の核

```rust
fn parse_expr_bp(&mut self, min_bp: u8) -> Expression {
    let mut lhs = self.parse_atom();  // 数値 / 識別子 / (...) など
    loop {
        let (lbp, rbp) = match self.peek_infix_bp() {
            Some(bp) => bp,
            None => break,
        };
        if lbp < min_bp { break; }
        let op = self.advance_op();
        let rhs = self.parse_expr_bp(rbp);
        lhs = Expression::Binary {
            left: Box::new(lhs),
            op,
            right: Box::new(rhs),
        };
    }
    lhs
}
```

これだけで `1 + 2 * 3` が正しい優先順位で組める。

### 学習ポイント

- 二項演算子の優先度を `(left_bp, right_bp)` で表現するパターン
- 右結合 (`a ? b : c`) は left_bp > right_bp、左結合は逆
- 後置演算子 (`f()`, `a[i]`, member access) を **infix として** Pratt で扱う発想
- oxc の expression parser も precedence climbing 系

### 読書 TODO

- [ ] [matklad / Simple but Powerful Pratt Parsing](https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html)
- [ ] [Eli Bendersky / TDOP](https://eli.thegreenplace.net/2010/01/02/top-down-operator-precedence-parsing)

---

## 6. Resilient parsing

新規ファイル: `src/parse_resilient.rs`、`src/ast_resilient.rs` (Error ノードを足した AST を別型として置く)

ねらい: parse エラーで止まらず、Error ノードを混ぜた AST を作り続ける parser。後段 (lint / format) で「壊れたコードでも何か返す」基盤になる。

### 前提

- ✅ Span (Error ノードに位置情報を載せるため)
- ✅ Result (sync set との組み合わせで使う)

### 最初のステップ

1. AST に Error ノードを追加 (`Expression::Error`、`Statement::Error` など)。AST を侵襲しないために `ast_resilient.rs` に別型として定義
2. `parse_resilient.rs` を `parse.rs` のコピーから出発して書き換え
3. **sync set** を決める (`}`, `;`, `const`, `return` など、文の境界として明らかに認識できるもの)
4. エラー時は Err 返却ではなく sync set まで skip して Error ノードを挿入
5. エラー情報は `Vec<ParseError>` を別途返す

### 学習ポイント

- 「壊れたコードでも木を作る」設計
- sync set の選び方 (狭すぎるとエラーがカスケード、広すぎるとエラー範囲が広い)
- Error ノードを後段でどう扱うか (基本はスキップ)
- oxc / rust-analyzer / Roslyn など実用 parser はだいたいこの方式

### 読書 TODO

- [ ] [matklad / Resilient LL Parsing Tutorial](https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html) — エラー回復の実装手引き

---

## 7. Arena allocator (実装ブランチ)

新規ファイル: `src/ast_arena.rs`、`src/parse_arena.rs`

ねらい: AST を `Box<T>` ではなく `&'a T` で持つ実装を書き、ライフタイム引数が parser 全体に伝染する感覚を体得する。`'a` の偏在が arena 系 parser を読むときの最大の障壁になるので、読書だけで済ませない。

### なぜ arena か

- 個別 `Box` の `malloc/free` overhead を消せる
- AST 用途に最適 (1 ファイル単位で「作る → 使う → 捨てる」をまとめて処理)
- Oxc / SWC など実用パーサの定番

### 最初のステップ

1. `Cargo.toml` に `bumpalo` を追加
2. `src/ast_arena.rs` に AST を再定義
   ```rust
   pub enum Expression<'a> {
       Number(f64),
       Binary {
           left: &'a Expression<'a>,
           op: BinOp,
           right: &'a Expression<'a>,
       },
       // ...
   }
   ```
3. `src/parse_arena.rs` で `bumpalo::Bump` を受け取る parser に書き換え
   ```rust
   pub fn parse<'a>(bump: &'a Bump, tokens: Vec<Token>) -> Vec<Statement<'a>> { ... }
   ```
4. **最小スコープで止める**: tokenize / wasm まで通す必要はない。式と const 宣言だけ arena 版で動けば学習目的は達成
5. 既存 `parse.rs` / `ast.rs` 相当は触らない (並置)

### 学習ポイント

- `&'a T` ベースの AST 設計
- ライフタイム引数が型・関数・trait 全体に伝染する様子
- `bumpalo::Bump::alloc` の使い方
- arena ベース parser のシグネチャに `'a` が至る所に出てくる理由

### 読書 TODO

- [ ] [bumpalo README](https://github.com/fitzgen/bumpalo)
- [ ] [oxc_allocator のソース](https://github.com/oxc-project/oxc/tree/main/crates/oxc_allocator) — bumpalo を AST 向けにラップした実例
- [ ] [oxc_ast のソース](https://github.com/oxc-project/oxc/tree/main/crates/oxc_ast) — `'a` がどう散らばっているか観察

---

## 8. Index 型 (NodeId / SymbolId / ScopeId)

新規ファイル: `src/naming_indexed.rs`

ねらい: 既存 `naming.rs` は `HashMap<String, _>` ベースだが、oxc の `oxc_semantic` は **`Vec<Symbol>` + `SymbolId(u32)`** ベースで symbol を管理する。これを真似て書き直す。

### 最初のステップ

1. `SymbolId(u32)` / `ScopeId(u32)` を newtype で定義
2. `Symbol { name, scope, span }` を持つ `Vec<Symbol>` を `SemanticBuilder` 側に保持
3. AST 上の `Identifier` は `String` ではなく `SymbolId` 参照を持つ (別 AST 型 `ast_indexed.rs` を作るのが綺麗だが、外部 `HashMap<NodeId, SymbolId>` でも可)
4. References (使用箇所) も `Vec<Reference>` + `ReferenceId` で管理
5. scope の親子関係を `Vec<Scope>` + `parent: Option<ScopeId>` で表現

### 学習ポイント

- index 型による間接参照のメリット (cache 局所性、Copy、シリアライズ容易)
- `Vec<Symbol>` の中に親子関係を `Option<SymbolId>` で持つ設計
- 実用 semantic API (oxc など) のシグネチャがなぜ index 型を返す形になっているかの理解
- LSP の go-to-definition / find-references が index 型で楽になる流れ

### 読書 TODO

- [ ] [oxc_semantic](https://github.com/oxc-project/oxc/tree/main/crates/oxc_semantic) — Symbol / Scope / Reference の管理
- [ ] [oxc_index](https://github.com/oxc-project/oxc/tree/main/crates/oxc_index) — index 型の基盤

---

## 9. Linter

新規ファイル: `src/lint.rs`

ねらい: visit パターン (節 4) と index 型 (節 8) の応用。

### 最初のステップ

1. `LintWarning` 型
   ```rust
   pub struct LintWarning {
       pub rule_name: String,
       pub message: String,
       pub span: Span,
   }
   ```
2. `pub fn lint(stmts: &[Statement]) -> Vec<LintWarning>` のエントリポイント
3. ルール 1 個目: **未使用変数の検出**
   - 節 4 の `Visit` trait を実装した struct を作る
   - 節 8 の symbol table を使うと「定義されたが参照されていない symbol」が直接取れる
4. ルールを増やす:
   - shadowing
   - 定数三項 (`true ? a : b` → 常に a)
   - 到達不能コード (`return` の後)

### 学習ポイント

- visit パターンの典型実装
- 「集めるパス」と「報告するパス」を分ける考え方
- ルール毎に visitor を作るスタイル (oxc の linter rule もこの形)

---

## 10. Formatter (浅め)

新規ファイル: `src/format.rs`

ねらい: AST → ソースコード変換。pretty-printer の **基本概念だけ** 押さえる。prettier 互換を目指すと沼なので、最小限に止める。

### 最初のステップ (ここまでで止める)

1. `pub fn format(stmts: &[Statement]) -> String` のエントリポイント
2. 数値 / 文字列 / 真偽値などのリテラル
3. 二項演算は **優先順位を見て括弧を補う** ロジック
4. ブロック内のインデント (2 スペース推奨)
5. アロー関数は常に複数行 (短い形は実装しない)

### 深追いしないもの

- 改行戦略 (Wadler / Prettier のような group / fill 系)
- コメント保持 (AST ベースなので諦める)
- 設定可能なオプション

### 学習ポイント

- pretty-printer の基本骨格
- 優先順位と括弧補完のロジック
- (読書のみ) Wadler の "A Prettier Printer" がどんな抽象を導入したか

### 読書 TODO

- [ ] [Wadler / A Prettier Printer (PDF)](https://homepages.inf.ed.ac.uk/wadler/papers/prettier/prettier.pdf) — 概念だけでよい
- [ ] [oxc_formatter](https://github.com/oxc-project/oxc/tree/main/crates/oxc_formatter) — どう group を作っているか観察

---

## 11. LSP (Phase 1 まで)

新規ファイル: `src/lsp.rs`

ねらい: stdin/stdout に JSON-RPC を流す LSP サーバを作って、**go-to-definition** だけ動かす。Completion / References などの深追いはしない (プロトコルの輪郭がつかめれば十分)。

### 前提

- ✅ Span (位置を返すのに必須)
- ✅ Result エラー (panic だと LSP プロセスが落ちる)
- ✅ Index 型 (節 8 の symbol table が go-to-definition に直結)

**parse が通った時だけ動けばよい** スタンスでいく。タイピング中の壊れたコードへの対応 (resilient parsing) は要求しない。

### スコープ

| Phase | 機能 | やる? |
|---|---|---|
| 1 | **Go-to-definition** | ✅ ここまで |
| 2 | References / Rename | ❌ 浅追いしない |
| 3 | Completion | ❌ 浅追いしない |

Phase 1 の前段で LSP プロトコル / 初期化 / 通信周りを一式組む必要があるので、立ち上げが一番重い。そこを越えたら次に進む。

### ライブラリ

- **[tower-lsp](https://github.com/ebkalderon/tower-lsp)** — async (tokio) ベース、サンプル豊富 (推奨)
- **[lsp-server](https://github.com/rust-lang/rust-analyzer/tree/master/lib/lsp-server)** — rust-analyzer 内製、同期ベース、シンプル
- **[lsp-types](https://crates.io/crates/lsp-types)** — 型定義だけ (上記両方が内部で使用)

### 動作確認

stdin/stdout に JSON-RPC を流すだけ。エディタ統合はやらない。

- 手動: `echo '{...}' | ./target/debug/lsp` で初期化リクエストなどを投げる
- `cargo test`: stdin/stdout に JSON-RPC を投げて end-to-end テスト

### 学習ポイント

- JSON-RPC ベースのプロトコル設計
- async/await の実用例 (tokio + tower)
- 「ソース ↔ AST ノード」の双方向索引の設計
- LSP 実装が semantic と密結合になりがちな理由の理解

### 読書 TODO

- [ ] [LSP 仕様](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
- [ ] [matklad / Why LSP?](https://matklad.github.io/2022/04/25/why-lsp.html) — LSP の設計思想
- [ ] [tower-lsp examples](https://github.com/ebkalderon/tower-lsp/tree/master/examples)
- [ ] [oxc_language_server](https://github.com/oxc-project/oxc/tree/main/crates/oxc_language_server)

---

## 想定スケジュール (目安)

| タスク | 所要 |
|---|---|
| 1. Span | 半日〜1 日 |
| 2. Result | 半日 |
| 3. codespan-reporting | 半日 |
| 4. Visit/Traverse | 半日 |
| 5. Pratt parser | 半日〜1 日 |
| 6. Resilient parsing | 1 日〜数日 |
| 7. Arena allocator (実装) | 1 日〜数日 (ライフタイム格闘) |
| 8. Index 型 | 半日〜1 日 |
| 9. Linter | 半日〜1 日 (ルール数しだい) |
| 10. Formatter (浅め) | 半日〜1 日 |
| 11. LSP Phase 1 | 数日 (立ち上げ重め) |

全体で 2 週間〜1 ヶ月程度。