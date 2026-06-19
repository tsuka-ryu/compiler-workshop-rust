# Resilient parsing と `src/parse_resilient.rs` の実装解説

## ねらい

parse エラーで `panic!` して止まるのではなく、**エラーを集めながら何かしらの結果を返し続ける** parser にする。実用 parser (oxc / rust-analyzer / Roslyn) は壊れたコードを常に食わされる (エディタでタイプ中のコードは大半が文法的に壊れている) ので、この性質が必須になる。

このリポジトリでは **oxc 忠実** の方式で実装した。matklad の [Resilient LL Parsing Tutorial](https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html) は lossless syntax tree + event 方式だが、oxc は **typed AST を直接組みながら 2 層のエラーで回復する** 方式で、設計がかなり違う (後述)。

`parse_span.rs` のコピーから出発し、`panic!` を 2 種類のエラー処理に置き換えた。AST は `Error` ノードを足した `ast_resilient.rs` を別途用意 (並置スタイル)。

## 核心: 2 層のエラー

oxc のエラーは **recoverable / fatal** の 2 層に分かれる ([oxc error_handler.rs](https://github.com/oxc-project/oxc/blob/main/crates/oxc_parser/src/error_handler.rs))。本リポジトリも同じ構造。

| | recoverable | fatal |
|---|---|---|
| メソッド | `error()` ([parse_resilient.rs:88](../src/parse_resilient.rs#L88)) | `set_fatal_error()` / `fatal_error<T>()` ([parse_resilient.rs:97](../src/parse_resilient.rs#L97)) |
| 動作 | `errors` に積むだけ。**パース続行** | フラグを立て、**lexer を EoF へ飛ばして全ループ巻き戻し** |
| 木への影響 | 残る (穴は空かない) | 最終的に**木ごと捨てる** (`statements = vec![]`) |
| 例 | セミコロン欠落 | 式の途中で想定外トークン |

```
                         エラー発生
                        /          \
              recoverable          fatal
              error() で           set_fatal_error() で
              errors に push        fatal_error フラグ + advance_to_end()
                    |                       |
              そのまま続行           has_fatal_error() で全ループ break
                    |                       |
              木は残る            スタック unwind → 各所が Dummy 返す
                    |                       |
              panicked=false        最上位で木を捨て panicked=true
```

## `Dummy` trait — `?` を使わずに型を満たす

fatal が起きたとき、Rust 的には「`Expression` を返す関数なのに値が作れない」状況になる。oxc は `Result` / `?` を**使わず**、代わりに `Dummy::dummy()` で「型を満たすためだけの捨て値」を返してスタックを巻き戻す ([oxc は `oxc_allocator::Dummy`](https://github.com/oxc-project/oxc/blob/main/crates/oxc_allocator/src/dummy.rs))。

本リポジトリでは `ast_resilient.rs` に `Dummy` trait を定義し、専用の `Error` variant を返す ([ast_resilient.rs:105](../src/ast_resilient.rs#L105)):

```rust
pub trait Dummy {
    fn dummy(span: Span) -> Self;
}
impl Dummy for Expression {
    fn dummy(span: Span) -> Self { Expression::Error { span } }
}
impl Dummy for Statement {
    fn dummy(span: Span) -> Self { Statement::Error { span } }
}
impl Dummy for TypeAnnotation {
    // 型注釈には Error variant を足さず、空 Named で代用
    fn dummy(span: Span) -> Self { TypeAnnotation::Named { name: String::new(), span } }
}
```

これにより `fatal_error<T: Dummy>()` が**任意のノード型に対してジェネリックに**ダミーを返せる ([parse_resilient.rs:108](../src/parse_resilient.rs#L108))。oxc の `fn fatal_error<T: Dummy<'a>>(&mut self, error) -> T` とそっくりな形。

> fatal 時は最終的に木ごと捨てられるので、`Error` ノードの中身が出力に現れることはない。あくまで「型を満たすための throwaway」。

## fatal が全ループを巻き戻す仕組み

`set_fatal_error()` は 2 つのことをする ([parse_resilient.rs:97](../src/parse_resilient.rs#L97)):

```rust
fn set_fatal_error(&mut self, error: ParseError) {
    if self.fatal_error.is_none() {       // 最初の fatal だけ記録
        self.advance_to_end();            // pos を EoF トークンへ
        self.fatal_error = Some(FatalError { error, errors_len: self.errors.len() });
    }
}
```

`advance_to_end()` が `pos` を末尾 (EoF) に飛ばす ([parse_resilient.rs:68](../src/parse_resilient.rs#L68))。これは oxc の `lexer.advance_to_end()` 相当。以降 `peek()` は常に EoF を返す ([parse_resilient.rs:25](../src/parse_resilient.rs#L25) で `pos` をクランプ)。

`has_fatal_error()` は「EoF に来た or fatal フラグが立った」で true を返す ([parse_resilient.rs:129](../src/parse_resilient.rs#L129)):

```rust
fn has_fatal_error(&self) -> bool {
    matches!(self.peek().kind, TokenKind::EoF) || self.fatal_error.is_some()
}
```

これでループが止まる。例えば arrow 本体の文リスト ([parse_resilient.rs:409](../src/parse_resilient.rs#L409)) は oxc の `while ... && !self.has_fatal_error()` と同じ形:

```rust
while !matches!(self.peek().kind, TokenKind::RCurly) && !self.has_fatal_error() {
    body.push(self.parse_statement());
}
```

> 他の `loop { ...; if comma { advance } else break }` 系 (call 引数 / array 要素 / arrow params) は、fatal 後に `peek` が EoF になり comma 判定が外れて**自然に break** するので明示ガードは不要。`while != RCurly` 系だけは EoF ≠ RCurly で無限ループになるので明示ガードが要る。

## oxc 語彙の cursor ヘルパ

`parse_span.rs` の `peek` / `advance` に加えて、oxc の [cursor.rs](https://github.com/oxc-project/oxc/blob/main/crates/oxc_parser/src/cursor.rs) と同じ語彙を用意した:

| メソッド | 意味 | 行 |
|---|---|---|
| `at(kind)` | 現在トークンが kind か (消費しない) | [42](../src/parse_resilient.rs#L42) |
| `eat(kind) -> bool` | 合致したら食って true | [47](../src/parse_resilient.rs#L47) |
| `expect(kind)` | 要求。無ければ **fatal** | [56](../src/parse_resilient.rs#L56) |
| `expect_ident() -> String` | 識別子を要求して名前を返す。無ければ fatal + 空文字 | [74](../src/parse_resilient.rs#L74) |

`expect` が「閉じ記号が無ければ fatal」を一手に引き受けるので、`parse_span.rs` に散らばっていた `match rparen.kind { RParen => {}, other => panic! }` 系がすべて `self.expect(&TokenKind::RParen)` の 1 行になった。

> `TokenKind` はデータ持ち variant (`Ident(String)` など) があるので、`at` での `==` 比較は `Const` / `LParen` のような単純 variant 専用。識別子や数値の取り出しは従来どおり `match self.advance().kind { ... }`。

## エントリポイントと最上位の fatal 処理

`parse_resilient()` は `ParserReturn { statements, errors, panicked }` を返す ([parse_resilient.rs:556](../src/parse_resilient.rs#L556))。oxc の `ParserReturn` ([oxc lib.rs](https://github.com/oxc-project/oxc/blob/main/crates/oxc_parser/src/lib.rs)) を写経:

```rust
let mut panicked = false;
if let Some(fatal) = parser.fatal_error.take() {
    panicked = true;
    parser.errors.truncate(fatal.errors_len); // 巻き戻し中の recoverable を捨てる
    parser.errors.push(fatal.error);          // fatal を 1 件だけ報告
    statements = Vec::new();                   // fatal 時は木を捨てる (oxc は Program::dummy)
}
```

ポイント:

- **`truncate(errors_len)`**: fatal が立った後、スタックを巻き戻す途中でさらに recoverable error が積まれることがある。それを fatal 時点の長さまで切り詰め、最後に fatal 本体を 1 件足す。「panic したら recoverable は無かったことにして fatal だけ報告」という oxc の挙動。
- **木を捨てる**: これが oxc 忠実版の肝。fatal では部分木を残さない。

## 挙動の確認 (snapshot)

[tests/parse_snapshots.rs](../tests/parse_snapshots.rs) の t07〜t09 が 3 パターンを固定している。

### recoverable: 木が残る (`const x = 5` — セミコロン欠落)

```
ParserReturn {
    statements: [ ConstDeclaration { name: "x", init: Number { 5 }, .. } ],  // ← 木は残る
    errors: [ ParseError { message: "expected ';'", .. } ],
    panicked: false,
}
```

### fatal: 木を捨てる (`const x = ;` — 式が壊れている)

```
ParserReturn {
    statements: [],                                                          // ← 空
    errors: [ ParseError { message: "unexpected token in expression: Semicolon", .. } ],
    panicked: true,
}
```

## matklad 方式との設計差

| | matklad (Resilient LL Tutorial) | oxc (本リポジトリ) |
|---|---|---|
| 木の種類 | lossless syntax tree (green tree) | typed AST |
| エラー表現 | tree 中に `ERROR` ノードを常に埋める | recoverable は `errors` vec、fatal はダミー |
| 回復 | sync set まで skip して `ERROR` で包み**必ず部分木を残す** | fatal は**木ごと捨てる**。回復は recoverable 層が担う |
| `?` / Result | 使わない (event 列を組む) | 使わない (`Dummy` で巻き戻す) |

「壊れたコードでも木を作る」という見出しに最も忠実なのは matklad 方式 (常に部分木が残る)。oxc は fatal で木を捨てるので、resilient さの主役は **recoverable 層** と、`?` 地獄を避ける **Dummy 機構** にある。本リポジトリは比較学習のため oxc 側を忠実になぞった。

## 読書 TODO

- [matklad / Resilient LL Parsing Tutorial](https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html) — sync set 方式のエラー回復。設計対比に
- [oxc_parser error_handler.rs](https://github.com/oxc-project/oxc/blob/main/crates/oxc_parser/src/error_handler.rs) — recoverable / fatal の 2 層
- [oxc_parser cursor.rs](https://github.com/oxc-project/oxc/blob/main/crates/oxc_parser/src/cursor.rs) — `at` / `eat` / `bump` / `expect` / `checkpoint` / `rewind`
- [oxc_allocator Dummy](https://github.com/oxc-project/oxc/blob/main/crates/oxc_allocator/src/dummy.rs) — ダミーノードの trait
