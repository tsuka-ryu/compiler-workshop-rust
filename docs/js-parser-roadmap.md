# JS/TS パーサー学習ロードマップ (javascript-parser-in-rust 版)

[oxc-project/javascript-parser-in-rust](https://github.com/oxc-project/javascript-parser-in-rust)
(日本語版あり) を **読みながら実装していく** ためのロードマップ。
旧 [next-steps.md](./next-steps.md) (toy 言語の技術ツアー) は放棄し、こちらに乗り換える。

元チュートリアルは「**oxc 流の本物の JavaScript/TypeScript パーサー**を作る」ための技術ツアー。
完全な ECMAScript パーサーを作るのが目的ではなく、`Chars` レキサー / estree AST / 再帰下降 + Pratt /
bumpalo アリーナ / scope tree / `miette` / string interning といった**実用パーサーの定番テクニック**を
JS という題材で一通り浴びるのが目的。本ロードマップも同じスタンスで、**実装可能な小さな JS subset**
(後述) に絞って各テクニックを実装する。

## これは toy compiler の「2 周目」である

このリポジトリの既存実装 (toy 言語: `const`/`return`/typecheck/monomorphize/wasm) で、
すでに以下の技術を一度通っている。本ロードマップは**同じ技術を、今度は本物の JS subset の上で、
oxc 流の語彙で**やり直す 2 周目。1 周目との対比が最大の学びになるので、各節に「toy 版との対比」を置く。

| 技術 | toy 版 (1 周目) | 本ロードマップ (2 周目・JS) |
|---|---|---|
| 位置情報 (Span) | `tokenize_span.rs` / `ast_span.rs` | `Node { start, end }` を最初から AST に埋める |
| Result エラー | `parse_result.rs` | `Result<T>` + `SyntaxError` + `thiserror` + `miette` |
| Pratt parser | `parse_pratt.rs` | 式パースを最初から Pratt で書く |
| Arena AST | `ast_arena.rs` / `parse_arena.rs` | `bumpalo::boxed::Box<'a>` / `collections::Vec<'a>` |
| String interning | `atom.rs` / `ast_arena_atom.rs` | `TokenValue::String(Atom)` (string_cache or 自前) |
| Index 型 / scope | `naming_indexed.rs` | `ScopeId` + scope tree (indextree 風) |
| Visit パターン | `visit.rs` / `traverse.rs` | scope ビルダを別パスの visitor で組む |

> 1 周目で `'a` の伝染やインターン化に散々苦しんだはずなので、2 周目は「本物の JS でも同じ構造が出る」
> ことを確認するのが主眼。**手が覚えている分、JS 文法そのものの難しさに集中できる。**

## 方針: 並置スタイル (旧 next-steps と同じ)

学習用なので、**同じ機能の別実装はファイルをコピーして並置する**。既存ファイルを書き換えて一本化しない。
ただし「並置するもの」と「同じファイルを育てるもの」を区別する:

- **並置する** (= 別実装が共存する価値があるもの):
  `lexer.rs` ↔ `lexer_interned.rs`、`ast.rs` (Box) ↔ `ast_arena.rs`、`parser.rs` ↔ `parser_arena.rs`
- **同じファイルを育てる** (= 線形に積み上がり、前の版に戻る価値が薄いもの):
  lexer に keyword / TokenValue を足す、AST に enum サイズ最適化を施す、parser に文を足していく

> toy 版では `parse_span.rs` → `parse_result.rs` のように "エラー処理" すら並置していたが、本ロードマップでは
> エラー処理は `parser.rs` を **`parser_result.rs` にコピーしてから** Result 化する (1 箇所だけ並置を残す)。
> 理由: panic 版と Result 版の diff を後から見返せると、`?` 伝播の入り方が分かりやすいため。

## ディレクトリ構成

既存 `src/` はフラット (toy 言語の 28 ファイルが平置き)。JS パーサーは**別サブプロジェクト**なので、
混ざらないよう `src/js/` サブツリーに隔離する。

```
src/
  lib.rs              ← `pub mod js;` を 1 行足すだけ (既存 toy モジュールは無改造)
  ...(既存 toy ファイル群はそのまま)...
  js/
    mod.rs            ← js サブツリーの公開エントリ。各 mod を登録
    lexer.rs          ← 節1〜2 (Token / Kind / Lexer / TokenValue)
    lexer_interned.rs ← 節10 (並置: String → Atom)
    ast.rs            ← 節3〜4 (Node / Program / Statement / Expression, Box)
    ast_arena.rs      ← 節11 (並置: bumpalo Box/Vec, 'a 伝染)
    parser.rs         ← 節5〜6 (再帰下降 + Pratt, panic 版)
    parser_result.rs  ← 節7〜8 (並置: Result + SyntaxError + miette)
    parser_arena.rs   ← 節11 (並置: arena AST を産出)
    error.rs          ← 節7 (SyntaxError, Result<T> エイリアス)
    semantic.rs       ← 節9 (Scope / ScopeBuilder / visitor)
    serialize.rs      ← 節12 (serde で estree JSON)
    ts.rs             ← 節13 (発展: lookahead / arrow backtracking)
tests/
  js_snapshots.rs     ← insta スナップショット (toy の tests/parse_snapshots.rs と同じ流儀)
```

> 代替案: 既存の流儀どおり `src/js_lexer.rs` のようにフラット + `js_` プレフィクスでも良い。ただし
> JS パーサーだけで 12+ ファイルになり既存 28 と混ざるため、サブツリー隔離を推奨。
> どちらでも以降の節の手順は (パスを読み替えれば) そのまま使える。

## 対象 subset (これだけ作る)

完全な JS は無理なので、**全テクニックを一度ずつ踏める最小の文法**に絞る。
チュートリアルの例文 (`debugger`, `var a`, `1 + 2 * 3`) を全部カバーする範囲:

```
Program        := Statement*
Statement      := DebuggerStatement       // debugger ;
                | VariableDeclaration      // (var|let|const) Ident (= Expression)? ;
                | BlockStatement           // { Statement* }
                | ExpressionStatement      // Expression ;
Expression     := NumberLiteral           // 1.23
                | StringLiteral            // "foo"
                | BooleanLiteral           // true / false
                | Identifier               // foo
                | BinaryExpression         // + - * / **
                | ParenExpression          // ( Expression )
                | ConditionalExpression    // test ? a : b   (発展)
```

`while` / 関数 / アロー / オブジェクトリテラルは subset 外 (必要になった節でだけ最小限足す)。

---

## 全体ロードマップ

元チュートリアルの章立て (overview → lexer → ast → parser → semantic → errors → typescript) に沿う。
各章の「Rust 最適化」節が、本ロードマップでは**並置バリアント**になる。

| 節 | 内容 | 主なファイル | 元チュートリアル章 |
|---|---|---|---|
| 0 | セットアップ (mod 登録 / 依存) | `js/mod.rs`, `lib.rs`, `Cargo.toml` | overview |
| 1 | Lexer 基礎 (`Chars` / offset / peek) | `js/lexer.rs` | lexer |
| 2 | Lexer JS 化 (keyword / TokenValue) | `js/lexer.rs` | lexer |
| 3 | AST 基礎 (Node / estree / Box) | `js/ast.rs` | ast |
| 4 | AST enum サイズ最適化 | `js/ast.rs` + test | ast |
| 5 | Parser 基礎: 文 (再帰下降) | `js/parser.rs` | parser |
| 6 | Parser: 式 = Pratt | `js/parser.rs` | parser |
| 7 | エラー処理 (Result / SyntaxError) | `js/parser_result.rs`, `js/error.rs` | errors |
| 8 | エラー整形 (miette) | `js/parser_result.rs` | errors |
| 9 | 意味解析: scope tree + visitor | `js/semantic.rs` | semantic_analysis |
| 10 | 並置: 文字列インターン化 | `js/lexer_interned.rs` | lexer |
| 11 | 並置: アリーナ AST | `js/ast_arena.rs`, `js/parser_arena.rs` | ast |
| 12 | estree JSON 出力 (serde) | `js/serialize.rs` | ast |
| 13 | TypeScript (発展) | `js/ts.rs` | typescript |
| 14 | References / 読書まとめ | — | references |

---

## 0. セットアップ

ねらい: `src/js/` サブツリーを作り、`lib.rs` に 1 行だけ足してビルドが通る土台を用意する。

### ステップ

1. `src/js/mod.rs` を作る (中身は空でよい。節が進むごとに `pub mod lexer;` 等を足す)
2. `src/lib.rs` に `pub mod js;` を **1 行追加** (既存 toy モジュールは触らない)
3. `Cargo.toml` の依存はこの段階では何も足さない (節ごとに必要になったら足す)
   - 先に全体像だけ: `unicode-id-start` (節2) / `thiserror` (節7) / `miette` (節8) /
     `bumpalo` (節11, 既存) / `string_cache` か自前 Atom (節10) / `serde` + `serde_json` (節12) /
     `bitflags` + `indextree` (節9, 任意)

### 学習ポイント

- 既存 crate に新サブシステムを「混ぜずに」足す感覚 (mod のサブツリー化)
- toy 版の `atom.rs` / `ast_arena.rs` が手元にあるので、節10/11 はそれを **読み返しながら** JS 版に移植できる

---

## 1. Lexer 基礎

新規ファイル: `src/js/lexer.rs`

ねらい: ソース文字列を `Chars` イテレータで舐めてトークン列にする最小レキサー。
まず `+` 1 個だけ。toy 版 (`tokenize.rs`) は `Vec<char>` + index 方式だったので、
**`Chars` + `offset()` 方式**との違いを体感する。

### 最初のステップ

1. `Token { kind: Kind, start: usize, end: usize }` と `Kind { Eof, Plus }`
2. `Lexer<'a> { source: &'a str, chars: Chars<'a> }` と `new`
3. `read_next_kind` (`while let Some(c) = self.chars.next()` で match)
4. `offset()` = `self.source.len() - self.chars.as_str().len()` で O(1) 位置取得
5. `read_next_token` で `start` / `kind` / `end` を組んで `Token` を返す
6. `peek(&self)` = `self.chars.clone().next()` で 1 文字先読み → `++` / `+=` をトークン化

### 学習ポイント

- `Chars` イテレータと `as_str().len()` が O(1) で offset を出す仕組み (チュートリアル lexer 章の核)
- `peek` を「`chars` をクローンして 1 歩進める」で実装する発想 (clone は index コピーだけで安い)
- **toy 版との対比**: `Vec<char>` index 方式 vs `Chars` 方式。後者は UTF-8 を安全に扱える

### 読書 TODO

- [ ] 元チュートリアル **lexer 章** (ja: `lexer.md`) の「トークン」「peek」
- [ ] [oxc_parser/src/lexer](https://github.com/oxc-project/oxc/tree/main/crates/oxc_parser/src/lexer) — 本物のレキサー

---

## 2. Lexer: JavaScript 化

編集ファイル: `src/js/lexer.rs` (育てる)

ねらい: `+` だけのレキサーを JS subset まで広げる。空白 / コメントのスキップ、識別子とキーワード、
数値・文字列リテラルの値抽出。

### 最初のステップ

1. 空白 (` \t\n`) と コメント (`//`, `/* */`) を読み飛ばす
2. 識別子: 先頭が `ID_Start`、続きが `ID_Continue`。crate [`unicode-id-start`](https://crates.io/crates/unicode-id-start) を使う
   (`is_id_start` / `is_id_continue`)。`var ಠ_ಠ` は OK / `var 🦀` は NG を確認
3. キーワード: `match_keyword(ident)` で長さ境界 (`1 <= len <= 10`) チェック後に match
   (`var`/`let`/`const`/`debugger`/`true`/`false`)。`undefined` は**キーワードではない**点に注意
4. `Kind` に `Identifier` / `Number` / `String` / `Plus`/`Minus`/`Star`/`Slash`/`StarStar` /
   `LParen`/`RParen`/`LCurly`/`RCurly`/`Semicolon`/`Eq`/`Question`/`Colon` などを足す
5. `TokenValue { None, Number(f64), String(String) }` を `Token` に足し、
   数値は `self.source[start..end].parse::<f64>()`、文字列はクォート内を `to_string()` で抽出

### 学習ポイント

- Unicode 識別子 (`ID_Start` / `ID_Continue`) という仕様の現実
- キーワードを enum バリアントにして parser 側の文字列比較を消す発想
- `Kind` を 1 バイト enum に保ち、値は別フィールド `TokenValue` に逃がす理由
  (enum サイズは最大バリアントで決まる ← toy の `atom.rs` で学んだのと同じ話)

### 読書 TODO

- [ ] 元チュートリアル lexer 章の「JavaScript」「トークンの値」「Rust の最適化 / より小さいトークン」
- [ ] [ECMAScript 仕様: Names and Keywords](https://tc39.es/ecma262/#sec-names-and-keywords)

---

## 3. AST 基礎

新規ファイル: `src/js/ast.rs`

ねらい: estree 互換の形で JS subset の AST を Rust の struct/enum で定義する。
[ASTExplorer](https://astexplorer.net/) で `acorn` の出力を見ながら形を合わせる。

### 最初のステップ

1. `Node { start: usize, end: usize }` (estree の位置情報。toy の `Span` 相当)
2. `Program { node: Node, body: Vec<Statement> }`
3. `Statement` enum: `DebuggerStatement` / `VariableDeclaration` / `BlockStatement` / `ExpressionStatement`
4. `VariableDeclaration { node, kind: VariableKind, declarations: Vec<VariableDeclarator> }`
   `VariableDeclarator { node, id: BindingIdentifier, init: Option<Expression> }`
5. `Expression` enum: `NumberLiteral` / `StringLiteral` / `BooleanLiteral` / `Identifier` /
   `BinaryExpression` / `ParenExpression` / `ConditionalExpression`
6. 自己再帰するフィールド (`BinaryExpression.left/right`) は `Box<Expression>` で包む

### 学習ポイント

- 「継承の代わりのコンポジション」= 各 struct に `Node` を持たせる estree 流
- Rust で自己参照 enum を作るには `Box` が要る (toy の `ast.rs` で既習)
- **toy 版との対比**: toy AST は独自言語、こちらは estree という**標準仕様**に形を合わせる制約

### 読書 TODO

- [ ] 元チュートリアル **ast 章** の「estree」「AST に慣れる」
- [ ] [estree es5.md](https://github.com/estree/estree/blob/master/es5.md) — ノード定義の正典
- [ ] [oxc_ast](https://github.com/oxc-project/oxc/tree/main/crates/oxc_ast)

---

## 4. AST enum サイズ最適化

編集ファイル: `src/js/ast.rs` (育てる) + テスト

ねらい: `Statement` / `Expression` enum を 16 バイトに保つ。
「enum サイズは最大バリアントの和」を実測で体感する。

### 最初のステップ

1. 大きいバリアントを `Box` で包む (`Expression::BinaryExpression(Box<BinaryExpression>)` の形)
2. 確認テスト:
   ```rust
   #[test]
   fn no_bloat_enum_sizes() {
       use std::mem::size_of;
       assert_eq!(size_of::<Statement>(), 16);
       assert_eq!(size_of::<Expression>(), 16);
   }
   ```
3. `RUSTFLAGS=-Zprint-type-sizes cargo +nightly build` で各型のサイズ内訳を眺める (任意)

### 学習ポイント

- enum を `match` で持ち回るとき、サイズが効く理由 (キャッシュ局所性)
- `no_bloat_enum_sizes` テストが rustc 本体にも存在する話
- **toy 版との対比**: toy では `atom.rs` で「enum サイズは縮まない」を学んだ。ここでは逆に「バリアントを
  Box 化して縮める」側を実装する

### 読書 TODO

- [ ] 元チュートリアル ast 章「Rust の最適化 / 列挙型のサイズ」
- [ ] [enum size のブログ](https://adeschamps.github.io/enum-size)

---

## 5. Parser 基礎: 文 (再帰下降)

新規ファイル: `src/js/parser.rs`

ねらい: トークン列を AST にする再帰下降パーサーの骨格。まず最も簡単な `debugger;` を通す。
toy の `parse.rs` と作りはほぼ同じだが、**oxc 流のカーソル語彙** (`at`/`bump`/`eat`/`expect`) で書く。

### 最初のステップ

1. `Parser<'a> { source: &'a str, lexer: Lexer<'a>, cur_token: Token, prev_token_end: usize }`
2. カーソルヘルパ: `cur_kind` / `at(kind)` / `advance` / `bump(kind)` / `bump_any` / `eat(kind)`
3. ノードヘルパ: `start_node()` (現在トークン start で `Node`) / `finish_node(node)` (`prev_token_end` で閉じる)
4. `parse_debugger_statement` → `Statement::DebuggerStatement`
5. `parse_program` で `body` を文のループで埋める。EOF まで `parse_statement` を回す
6. `parse_statement` で `cur_kind` を見て `debugger` / `var|let|const` / `{` / 式文 に分岐
7. `parse_variable_declaration` (`kind` を読む → ident → `=` Expression? → `;`)

### 学習ポイント

- `start_node` / `finish_node` で span を「開始トークン start 〜 直前トークン end」で組む定石
- `at` / `eat` / `bump` / `expect` という oxc/rust-analyzer 共通のカーソル語彙
- **toy 版との対比**: `parse_resilient.rs` で既にこの語彙を使った。ここで「素の」版を本物の JS で書き直す

### 読書 TODO

- [ ] 元チュートリアル **parser 章** の「ヘルパー関数」「parse 関数」
- [ ] [oxc_parser/src/cursor.rs](https://github.com/oxc-project/oxc/blob/main/crates/oxc_parser/src/cursor.rs)

---

## 6. Parser: 式 = Pratt

編集ファイル: `src/js/parser.rs` (育てる)

ねらい: 式パースを Pratt (binding power) で実装する。toy の `parse_pratt.rs` の知識をそのまま使う。
チュートリアルも「式は深く再帰するので Pratt を使え」と言っている。

### 最初のステップ

1. `parse_expression` → `parse_expr_bp(0)`
2. `parse_atom`: 数値 / 文字列 / 真偽値 / 識別子 / `( Expression )`
3. `infix_bp(kind) -> Option<(u8, u8)>`: `**`(右結合) > `* /` > `+ -` > `?:`(右結合)
4. `parse_expr_bp(min_bp)` ループで `lhs` を `BinaryExpression` / `ConditionalExpression` に畳む
5. `parse_expression_statement` (式 + `;`) を `parse_statement` に接続

### 実装の核 (toy の再掲)

```rust
fn parse_expr_bp(&mut self, min_bp: u8) -> Expression {
    let mut lhs = self.parse_atom();
    while let Some((lbp, rbp)) = self.infix_bp(self.cur_kind()) {
        if lbp < min_bp { break; }
        let op = self.advance_op();
        let rhs = self.parse_expr_bp(rbp);
        lhs = Expression::BinaryExpression(Box::new(/* left=lhs, op, right=rhs */));
    }
    lhs
}
```

### 学習ポイント

- 右結合 (`**`, `?:`) は `left_bp > right_bp`、左結合は逆
- スタックオーバーフローを避けるための Pratt の動機 (深い式)
- **toy 版との対比**: 完全に同じテクニック。JS の演算子優先順位表に置き換えるだけ

### 読書 TODO

- [ ] 元チュートリアル parser 章「式のパース」
- [ ] [matklad / Simple but Powerful Pratt Parsing](https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html) (再読)

---

## 7. エラー処理: Result / SyntaxError

新規ファイル: `src/js/parser_result.rs` (`parser.rs` をコピー)、`src/js/error.rs`

ねらい: panic 版パーサーを `Result` 化する。**コピーして並置**し、panic 版との diff を残す。
新依存: `thiserror`。

### 最初のステップ

1. `parser.rs` を `parser_result.rs` にコピー
2. `error.rs` に:
   ```rust
   pub type Result<T> = std::result::Result<T, SyntaxError>;

   #[derive(Debug, thiserror::Error)]
   pub enum SyntaxError {
       #[error("Unexpected token")]
       UnexpectedToken,
       #[error("Expected a semicolon")]
       AutoSemicolonInsertion,
       #[error("Unterminated string")]
       UnterminatedString,
   }
   ```
3. `expect(kind) -> Result<()>`: `at(kind)` でなければ `Err(SyntaxError::UnexpectedToken)`
4. 各 `parse_*` を `Result<...>` 化し、`expect(...)?` / 子パースに `?` を伝播
5. テストは当面 `.unwrap()` でしのぐ (toy 版と同じ流儀)

### 学習ポイント

- `Result<T>` 型エイリアスでエラー型を一点管理する定石
- `?` 伝播で parser コードが panic 版とほぼ同じ見た目に保たれること
- recoverable / panic の概念整理 (本格的な resilient は toy `parse_resilient.rs` で既習なので深追いしない)
- **toy 版との対比**: `parse_span.rs` → `parse_result.rs` でやったのと同じ写経。今回は JS subset で

### 読書 TODO

- [ ] 元チュートリアル **errors 章** の「Result」「Error トレイト」
- [ ] [oxc_parser error 周り](https://github.com/oxc-project/oxc/tree/main/crates/oxc_parser/src) — `diagnostics.rs` / `error_handler.rs`

---

## 8. エラー整形: miette

編集ファイル: `src/js/parser_result.rs` (育てる) / `src/main.rs` か例

ねらい: `miette` でソース該当箇所をハイライトした派手なエラー表示にする。
toy では `codespan-reporting` を使ったので、**miette との設計対比**が学び。

### 最初のステップ

1. `Cargo.toml`: `miette = { version = "7", features = ["fancy"] }`
2. parser の `Result` 型はそのままに、最上位で `miette::Error::new(err).with_source_code(NamedSource::new(path, src))` でラップ
3. (発展) `SyntaxError` に `#[derive(miette::Diagnostic)]` を足し、`#[label]` で span を指す

### 学習ポイント

- 「parser の Result 型を変えずに表示層だけ差し替える」設計 (関心の分離)
- **toy 版との対比**: `codespan-reporting` (`diagnostics.rs`) vs `miette`。API 思想はほぼ同じ、
  miette は `Diagnostic` derive マクロで宣言的

### 読書 TODO

- [ ] 元チュートリアル errors 章「Fancy Error Report」
- [ ] [miette](https://docs.rs/miette/latest/miette) / toy の `docs/` codespan メモと対比

---

## 9. 意味解析: scope tree + visitor

新規ファイル: `src/js/semantic.rs`

ねらい: スコープツリーを構築する。`var` / `let` の宣言名を scope ごとに集め、親をたどって解決する。
toy の `naming_indexed.rs` (Symbol/Scope/Reference + SemanticBuilder) を JS subset に移植する形。

### 最初のステップ

1. `ScopeId(u32)` と `Scope { parent: Option<ScopeId>, flags: ScopeFlags, bindings: ... }`
   (チュートリアルは `indextree` + `bitflags` を使うが、toy `naming_indexed.rs` の `Vec<Scope>` + `u32` 方式でも可)
2. `ScopeFlags`: `TOP` / `FUNCTION` / `ARROW` などを `bitflags!` で (subset では `TOP` / `BLOCK` 程度でよい)
3. `ScopeBuilder { scopes, current_scope_id }` に `enter_scope(flags)` / `leave_scope()`
4. **visitor パス**で AST を pre-order に巡回し、`BlockStatement` で enter/leave、宣言で binding 登録
5. Early Error の一例: 同一 scope での重複宣言 (`let a; let a;`) をエラーにする

### 学習ポイント

- scope tree (親リンク) を `Option<ScopeId>` で持つ設計 (toy `naming_indexed.rs` と同型)
- パース中に作るか別パス (visitor) で作るかのトレードオフ
- Early Error (ECMAScript 仕様の "Static Semantics: Early Errors") の存在
- **toy 版との対比**: `naming_indexed.rs` の `enter_scope`/`leave_scope`/`resolve` がそのまま効く

### 読書 TODO

- [ ] 元チュートリアル **semantic_analysis 章** 全体
- [ ] [oxc_semantic](https://github.com/oxc-project/oxc/tree/main/crates/oxc_semantic) (再読)

---

## 10. 並置: 文字列インターン化

新規ファイル: `src/js/lexer_interned.rs` (`lexer.rs` をコピー)

ねらい: `TokenValue::String(String)` を `TokenValue::String(Atom)` に置き換え、識別子の重複 alloc を消す。
toy の `atom.rs` / `ast_arena_atom.rs` をほぼそのまま使える。

### 最初のステップ

1. `lexer.rs` を `lexer_interned.rs` にコピー
2. `Atom` を用意 (toy `atom.rs` の自前 `Atom<'a>(&'a str)` + `Interner`、もしくは
   [`string_cache`](https://crates.io/crates/string_cache) の `atom!` マクロ)
3. `TokenValue::String(Atom)` に変更し、トークン化時に intern
4. 同じ識別子が同一ポインタを共有することをテストで実証 (`"a + a"` で left/right が同一 atom)

### 学習ポイント

- `==` が文字列比較からポインタ比較に化ける条件 (interning が効くとき)
- **toy 版との対比**: `atom.rs` / `ast_arena_atom.rs` の知見を JS レキサーに移植するだけ

### 読書 TODO

- [ ] 元チュートリアル lexer 章「文字列のインターン化」
- [ ] [oxc_span::Atom](https://github.com/oxc-project/oxc/blob/main/crates/oxc_span/src/atom.rs)

---

## 11. 並置: アリーナ AST

新規ファイル: `src/js/ast_arena.rs` (`ast.rs` をコピー)、`src/js/parser_arena.rs` (`parser.rs` をコピー)

ねらい: `Box<Expression>` を `bumpalo::boxed::Box<'a, Expression<'a>>` に、`Vec` を
`bumpalo::collections::Vec<'a, _>` に置き換え、`'a` が AST と parser 全体に伝染するのを (再) 体感する。

### 最初のステップ

1. `ast.rs` → `ast_arena.rs`: 子を `bumpalo::boxed::Box<'a, _>`、リストを `collections::Vec<'a, _>`、
   各型に `<'a>` を付与
2. `parser.rs` → `parser_arena.rs`: `&'a Bump` を受け取り、`bump.alloc` / `bumpalo::vec!` で AST を産出
3. AST ノードサイズが `Box` 版と同じ 16 バイトに保たれるか確認

### 学習ポイント

- `'a` が型・関数・trait 全体に伝染する様子 (toy `ast_arena.rs` で既習。JS でも同じと確認)
- bump 確保がポインタ加算だけで速い理由 / 個別 free できないトレードオフ
- **toy 版との対比**: `ast_arena.rs` / `parse_arena.rs` の写経を JS subset で再演

### 読書 TODO

- [ ] 元チュートリアル ast 章「メモリアリーナ」
- [ ] [oxc_allocator](https://github.com/oxc-project/oxc/tree/main/crates/oxc_allocator) (再読)

---

## 12. estree JSON 出力 (serde)

新規ファイル: `src/js/serialize.rs` (もしくは `ast.rs` に `#[derive(Serialize)]`)

ねらい: AST を estree 互換 JSON にシリアライズし、[ASTExplorer](https://astexplorer.net/) の `acorn` 出力と
見比べる。「自分の AST が本物と同じ形か」を客観確認できる。

### 最初のステップ

1. `Cargo.toml`: `serde = { version = "1", features = ["derive"] }` / `serde_json`
2. `#[serde(tag = "type")]` で struct 名を `type` フィールドに
3. `Node` を `#[serde(flatten)]` で start/end を親に展開
4. `#[serde(rename = "...")]` で estree のノード名に合わせる (例: `BindingIdentifier` → `Identifier`)
5. enum は `#[serde(untagged)]` で余計なラッパを作らない
6. `serde_json::to_string_pretty(&program)` で出力し、ASTExplorer と diff

### 学習ポイント

- serde の `tag` / `flatten` / `rename` / `untagged` の使い分け
- 「機械可読な estree」という相互運用フォーマットの存在意義

### 読書 TODO

- [ ] 元チュートリアル ast 章「JSON シリアライゼーション」
- [ ] [serde 公式](https://serde.rs/)

---

## 13. TypeScript (発展)

新規ファイル: `src/js/ts.rs`

ねらい: ここからは難所。**概念を浴びるだけ**でよい。先読みバッファとバックトラッキングで TS 特有の
曖昧文法 (アロー関数 / 型アサーション) を捌く発想を知る。subset としては「型注釈付き変数宣言」
(`const x: number = 1`) を 1 つ通せれば十分。

### 最初のステップ (止めどころを決めて深追いしない)

1. レキサーに**複数トークンの先読みバッファ**を足す動機を理解 (`TSIndexSignature` の例)
2. 型注釈の最小実装: `VariableDeclarator` に `type_annotation: Option<TSType>` を足し、
   `: number` / `: string` だけパース
3. (読むだけ) アロー関数の `lookAhead` + `tryParse` バックトラッキング (TS 本体 `parser.ts`)
4. (読むだけ) JSX vs TSX の `<string>` 曖昧性

### 学習ポイント

- 先読み (高速パス) + バックトラッキング (低速パス) の二段構え
- TS に仕様書がなく「実装が仕様」である現実
- **toy 版との対比**: toy にも型 (`typecheck.rs`) はあったが、ここは**構文の曖昧性**の話で別軸

### 読書 TODO

- [ ] 元チュートリアル **typescript 章** 全体
- [ ] [TypeScript parser.ts](https://github.com/microsoft/TypeScript/blob/main/src/compiler/parser.ts) (該当箇所だけ)

---

## 14. References / 読書まとめ

元チュートリアル references 章のリンク集 + 本ロードマップで参照した oxc crate を俯瞰する回。
実装はなし。toy の `docs/reading-oxc.md` (oxc の自動生成・独自 Box/Vec などの前提知識) を
読み返しておくと、節11 の arena や oxc ソース読みが楽になる。

### 読書 TODO

- [ ] 元チュートリアル **references 章**
- [ ] toy の [docs/reading-oxc.md](./reading-oxc.md) を再読

---

## 想定スケジュール (目安)

toy 版の実績ペースを前提に、2 周目で手が覚えている分を割り引いた見積り。

| 節 | 所要 | 備考 |
|---|---|---|
| 0. セットアップ | 30 分 | mod 登録だけ |
| 1. Lexer 基礎 | 1〜2 時間 | `Chars` 方式が toy と違う |
| 2. Lexer JS 化 | 2〜3 時間 | unicode-id-start / keyword / TokenValue |
| 3. AST 基礎 | 1〜2 時間 | estree に形を合わせる |
| 4. AST enum サイズ | 30 分 | Box 化 + size テスト |
| 5. Parser 文 | 2〜3 時間 | カーソル語彙 |
| 6. Parser 式 (Pratt) | 1〜2 時間 | toy の再演 |
| 7. エラー処理 | 1〜2 時間 | コピー → Result 化 |
| 8. miette | 1 時間 | 表示層だけ |
| 9. 意味解析 | 3〜4 時間 | naming_indexed 移植 |
| 10. インターン化 | 1〜2 時間 | atom.rs 移植 |
| 11. アリーナ AST | 半日〜1 日 | `'a` 伝染の再演 |
| 12. estree JSON | 1〜2 時間 | serde 属性 |
| 13. TypeScript | 半日 (深追いしない) | 概念中心 |
| 14. References | — | 読書のみ |

合計はおよそ **4〜6 日分** (週末ベースで 2〜3 週間)。2 周目なので 1 周目より速いはず。
