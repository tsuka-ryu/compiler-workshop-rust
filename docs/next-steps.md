# 今後の予定

このリポジトリでさらに学習を進めるためのロードマップ。

- Pratt parser
- Resilient parsing
- Span / 位置情報
- Result ベースのエラー処理
- codespan-reporting でエラー整形
- Linter
- Formatter
- LSP 実装
- Arena allocator (読書のみ)

## Pratt parser

新規ファイル: `src/parse_pratt.rs`

ねらい: 既存の recursive descent との対比。同じ AST を別の方法で構築する。

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

### 読書 TODO

- [ ] [matklad / Simple but Powerful Pratt Parsing](https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html)
- [ ] [Eli Bendersky / TDOP](https://eli.thegreenplace.net/2010/01/02/top-down-operator-precedence-parsing)

## Resilient parsing

新規ファイル: `src/parse_resilient.rs`

ねらい: parse エラーで止まらず、Error ノードを混ぜた AST を作り続ける parser。後段 (lint / format) で「壊れたコードでも何か返す」基盤になる。

### 前提

- Span / Result (なくても実装可能。あれば Error ノードに位置情報やメッセージを乗せやすい)

### 最初のステップ

1. AST に Error ノードを追加 (`Expression::Error`、`Statement::Error` など)
2. 既存 parser をベースに `parse_resilient.rs` を作る
3. **sync set** を決める (`}`, `;`, `const`, `return` など、文の境界として明らかに認識できるもの)
4. エラー時は Err 返却ではなく sync set まで skip して Error ノードを挿入
5. エラー情報は `Vec<ParseError>` を別途返す

### 学習ポイント

- 「壊れたコードでも木を作る」設計
- sync set の選び方 (狭すぎるとエラーがカスケード、広すぎるとエラー範囲が広い)
- Error ノードを後段でどう扱うか (基本はスキップ)

### 読書 TODO

- [ ] [matklad / Resilient LL Parsing Tutorial](https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html) — エラー回復の実装手引き
  - sync set を持って、エラーが来たらそこまで skip
  - AST に Error ノードを混ぜて結果を返す
  - rust-analyzer などの実用 parser はだいたいこの方式

## Span / 位置情報

変更箇所: `src/tokenize.rs`、`src/parse.rs`、各 AST ノード

ねらい: ソース上の位置情報を AST に付ける。エラーメッセージ、linter、formatter、LSP の基盤。

### 最初のステップ

1. `Span` 型を定義
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub struct Span {
       pub start: usize,
       pub end: usize,
   }
   ```
2. `Token` を `(TokenKind, Span)` ペアまたは `Token { kind: TokenKind, span: Span }` に変更
3. tokenize で位置を追跡 (`char_indices` がすでに位置を返している)
4. parser でトークンの span を集約して AST ノードの span を作る
5. 最後に AST 各ノードに `span: Span` フィールドを追加

### 注意

`Token` 型をいじると tokenize / parse / 全 AST ノードに影響が及ぶ。

### 学習ポイント

- 実用 parser の基本である位置情報の取り扱い
- AST ノードを value 型のままにするか `Box<NodeWithSpan>` にするかなどの設計判断

## Result ベースのエラー処理

変更箇所: 各 parser ファイル

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

## codespan-reporting でエラー整形

新規依存: `codespan-reporting`。エラー出力に Diagnostic 整形を組み込む。

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
2. `ParseError` / `TypeError` / `NamingError` をまとめた `Diagnostic` 変換層を用意
3. `Files` impl にソース文字列を渡して、`emit` で stderr に整形出力

### 学習ポイント

- 1 つのエラーに複数の span (`primary` + `secondary`) を付ける設計
- `Severity` (error / warning / note) を使い分ける
- 「機械可読 (LSP 等) と人間可読 (CLI) を同じデータから作る」発想

### 読書 TODO

- [ ] [codespan-reporting README](https://github.com/brendanzab/codespan) — 基本 API
- [ ] [ariadne](https://github.com/zesterer/ariadne) — もう一つの選択肢、見栄えが派手。設計の対比に
- [ ] rustc のエラー (`rustc --explain E0308` など) を見て、どんな情報をどう並べてるか観察

## Linter

新規ファイル: `src/lint.rs`

ねらい: AST visitor pattern の練習。

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
   - 構文木を 2 回 walk
     - 1 回目: 全ての `Identifier` 参照を `HashSet<String>` に集める
     - 2 回目: 全ての `ConstDeclaration` で、参照されてない名前を warning に
4. ルールを増やす:
   - shadowing
   - 定数三項 (`true ? a : b` → 常に a)
   - 到達不能コード (`return` の後)

### 学習ポイント

- visitor pattern の典型実装
- 「集めるパス」と「報告するパス」を分ける考え方

## Formatter

新規ファイル: `src/format.rs`

ねらい: AST → ソースコード変換。pretty-printer の設計を学ぶ。

### 最初のステップ

1. `pub fn format(stmts: &[Statement]) -> String` のエントリポイント
2. 数値 / 文字列 / 真偽値などのリテラルから始める
3. 二項演算は **優先順位を見て括弧を補う** ロジック
4. ブロック内のインデント (2 スペース推奨)
5. アロー関数の整形 (短ければ 1 行、長ければ複数行)

### ハマりどころ

- 括弧の要否: `(1 + 2) * 3` は括弧を残し、`1 + (2 * 3)` は括弧を消す
- 改行ルール: JS 版の `solutions/1/formatter.js` を参考にできる
- AST ベースなのでコメント / 空白は失われる (ここでは許容)

### 学習ポイント

- pretty-printer の基本
- AST walker
- 優先順位と括弧補完のロジック

## LSP 実装

新規ファイル: `src/lsp.rs`

ねらい: stdin/stdout に JSON-RPC を流す LSP サーバを作って、エディタから AST/naming/typecheck の結果を引き出す。

### 前提

- ✅ Span (位置を返すのに必須)
- ✅ Result エラー (panic だと LSP プロセスが落ちる)

**parse が通った時だけ動けばよい** スタンスでいく。タイピング中の壊れたコードへの対応 (resilient parsing) は要求しない。

### 段階

| Phase | 機能 | 必要なもの |
|---|---|---|
| 1 | **Go-to-definition** | naming.rs の定義位置を保持 |
| 2 | **References / Rename** | naming の逆引き (使用箇所一覧) |
| 3 | **Completion** | スコープ内の名前一覧を span 位置から計算 |

Phase 1 の前段で LSP プロトコル / 初期化 / 通信周りを一式組む必要があるので、立ち上げが一番重い。Phase 2 以降は AST 索引を整備すれば軽い。

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

### 読書 TODO

- [ ] [LSP 仕様](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
- [ ] [matklad / Why LSP?](https://matklad.github.io/2022/04/25/why-lsp.html) — LSP の設計思想
- [ ] [tower-lsp examples](https://github.com/ebkalderon/tower-lsp/tree/master/examples)

## Arena allocator (bumpalo) [読書のみ]

ねらい: Oxc の `oxc_allocator` の元ネタを理解する。AST のような「同時に大量に作って一気に捨てる」データ構造での所有権設計を知る。

### なぜ arena か

- 個別 `Box` の `malloc/free` overhead を消せる
- AST 用途に最適 (1 ファイル単位で「作る → 使う → 捨てる」をまとめて処理)
- Oxc / SWC など実用パーサの定番

### 読書 TODO

- [ ] [bumpalo README](https://github.com/fitzgen/bumpalo)
- [ ] [oxc_allocator のソース](https://github.com/web-infra-dev/oxc/tree/main/crates/oxc_allocator) — bumpalo を AST 向けにラップした実例

### 発展: 実験

このリポジトリで実際に試す場合: `Expression` / `Statement` 内の `Box` を `&'a Expression` に置き換えてみる。ライフタイム引数が parser 全体に伝染するので影響大。やるなら experiments/ ブランチで。

## 想定スケジュール (目安)

| タスク | 所要 |
|---|---|
| Pratt parser | 半日〜1 日 |
| Resilient parsing | 1 日〜数日 |
| Span | 半日〜1 日 (リファクタ量しだい) |
| Result | 半日 |
| codespan-reporting | 半日 |
| Linter | 半日〜1 日 (ルール数しだい) |
| Formatter | 1 日〜数日 (整形の細かさしだい) |
| LSP | 数日〜1 週間 (機能数しだい) |

全体で 2 週間〜1 ヶ月程度。