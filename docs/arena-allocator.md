# Arena allocator (`&'a T` ベースの AST)

[next-steps.md](./next-steps.md) 節 8 の実装メモ。
`Box<T>` ベースの AST を `&'a T` ベースに書き換え、**ライフタイム引数 `'a` が型・関数・構造体の全体に伝染する**様子を体得するのがねらい。

対応する実装ファイル: [src/ast_arena.rs](../src/ast_arena.rs)、[src/parse_arena.rs](../src/parse_arena.rs)。
ベースにしたのは span 版の [src/ast_span.rs](../src/ast_span.rs)、[src/parse_span.rs](../src/parse_span.rs)（並置スタイルなので元は触っていない）。

---

## 1. なぜ arena か

AST は「式の木」なので、ノードが別ノードを指す。`a + b` ならこう:

```
   Binary(+)
   /      \
  a        b
```

`Binary` は中に `Expression` を持つが、`Expression` のサイズはコンパイル時に決まらない
(中にさらに `Binary` が入れ子になり得るので、直接持つと型が無限サイズになる)。
だから**実体はどこか別の場所に置き、そこを指す「矢印」で持つ**必要がある。この矢印の作り方が 2 通りある。

### 方法 A: `Box`（元の実装）

```rust
left: Box::new(left)
```

`Box::new` は OS に「メモリちょうだい」と**ノード 1 個ずつ**お願いする (malloc)。
式が 1000 個あれば 1000 回 malloc し、捨てるときも 1 個ずつ free する。この個別の確保・解放がオーバーヘッドになる。

### 方法 B: arena（この実装）

最初に**でかい 1 枚の土地 (`bumpalo::Bump`)** を確保しておき、ノードは端から順に詰めていく。

```rust
left: self.bump.alloc(left)
```

`bump.alloc(left)` =「土地の空きに `left` を置いて、その場所を指す矢印を返す」。
ポインタを進めるだけなので速い (bump = 「ぐいっと押す」)。
捨てるときも個別 free はせず、**土地ごと一括で破棄**する。

AST 用途にこれが噛み合う理由:

- 1 ファイル分の AST は「まとめて作る → まとめて使う → まとめて捨てる」という寿命が揃っている
- 途中で個別のノードだけ捨てることはまずない
- だから「個別 free できない」という arena の弱点が問題にならず、速度だけ得られる

oxc / SWC など実用パーサが arena を使うのはこのため。

---

## 2. `Box::new` → `bump.alloc` 置換

やることの本体はこれだけ。意味は同じ (矢印を作る) で、置き場所が「個別 malloc」から「共有の土地」に変わる。

```rust
// 元
left: Box::new(left),
// arena 版
left: self.bump.alloc(left),
```

`self.bump.alloc(left)` は `left` を arena 上にムーブし、`&'a mut Expression<'a>` を返す。
これが `&'a Expression<'a>` に自動で型強制される。

### コンパイラの修正提案 `&Box::new(x)` は罠

`Box::new(left)` のまま放置すると、コンパイラは
「`&Expression` が欲しいのに `Box<Expression>` が来てる、`&` を付けたら?」と
`&Box::new(left)` を提案してくる。**これには従わない。**

`Box::new(left)` はその行で作った一時的な箱で、文が終わると drop される。
drop される物への参照は無効になり、後で必ずライフタイムエラーになる。正解は `self.bump.alloc(left)`。

---

## 3. `'a` の伝染（この節の主目的）

`&'a T` を 1 箇所に導入すると、`'a` が芋づる式に全体へ広がる。連鎖はこう:

```
arena (Bump) を呼び出し側で作る
  → 入口関数 parse_span<'a>(bump: &'a Bump, ...)        ← 'a 発生
    → struct Parser<'a> { bump: &'a Bump, ... }         ← 構造体に 'a
      → impl<'a> Parser<'a>                              ← impl ヘッダに 'a
        → 各メソッド fn parse_expression -> Expression<'a> ← 返り値に 'a
          → enum Expression<'a> { Binary { left: &'a Expression<'a> } } ← AST 型に 'a
```

1 箇所足すと、それを使う全部に `'a` を書く羽目になる。
これが oxc の AST を開いて「なぜ `'a` だらけなんだ」と感じる正体。読書だけでは身につかないので手で写経する価値がある。

### 特にハマる 3 点

1. **`&'a Expression<'a>` の `<'a>` が 2 回**
   外側の `'a` は「参照自身がどれだけ生きるか」、内側の `<'a>` は「中身の `Expression` が持つ参照の寿命」。
   今回は arena が全部を覆うので両方同じ `'a` で押し切れる。

2. **返り値の `'a` を省略すると `&mut self` に紐づく**
   `fn parse_expression(&mut self) -> Expression` のように `<'a>` を省くと、
   Rust は「省略されたライフタイム = `&mut self` の寿命」と推測する。すると
   「返り値は self から借りている → 返り値が生きてる間 self はロック」となり、
   `let test = self.parse_binary();` の直後に `self.advance()` を呼んだだけで
   **二重借用エラー (E0499/E0502)** が出る。
   直し方は `-> Expression<'a>` と明示すること。「返り値が借りてるのは self ではなく arena (`'a`) だ」と伝えると self のロックが消える。

3. **引数にも `'a` が要る場合がある**
   `fn parse_call(&mut self, callee: Expression<'a>) -> Expression<'a>` のように、
   `Expression` を**受け取る**引数にも `<'a>` が必要。返り値だけ直して見落としやすい。

### 入口で arena を「持てない」理由

`parse_span` 関数自身は arena を所有できない。
関数内で `Bump::new()` して返り値の AST を返すと、関数を抜けた瞬間に土地ごと drop され、
中の `&'a` 参照が全部ダングリングになる。
だから**呼び出し側で arena を作り、引数 `bump: &'a Bump` で借りて渡す**。
arena parser のシグネチャに `'a` が顔を出すのはこの構造のため。

```rust
let bump = Bump::new();                       // 呼び出し側が土地を所有
let stmts = parse_span(&bump, tokens);        // 借りて渡す
// stmts は bump が生きてる間だけ有効
```

---

## 4. この実装の到達点と、あえて止めた所

到達点 (式と const 宣言が arena 上で動く):

- AST のノード間参照は全て `&'a Expression<'a>` / `&'a TypeAnnotation<'a>`
- パーサは `bumpalo::Bump` を借りて `bump.alloc` でノードを確保
- 既存テスト (span 検証) が arena 版でもそのまま通る

あえて std のまま残した所 (学習スコープを絞るため):

- `Vec<Expression<'a>>` は **std の `Vec`**。bumpalo の `bumpalo::collections::Vec` には載せていない
- `String` は **std の `String`**。`&'a str` 化は節 9 (string interning) の担当

つまり「ノード本体は arena、可変長コレクションと文字列はまだヒープ」という中間状態。
ここを詰めるのが次の発展。

---

## 5. oxc との対応

| この実装 | oxc |
|---|---|
| `bumpalo::Bump` を直接利用 | `oxc_allocator::Allocator` が bumpalo をラップ |
| `&'a Expression<'a>` | `oxc_allocator::Box<'a, T>`（std の Box ではない、ライフタイム付き） |
| `Vec<Expression<'a>>`（std のまま） | `oxc_allocator::Vec<'a, T>`（arena 上、`'a` が前に来る） |
| `String`（std のまま） | `oxc_span::Atom<'a>`（節 9） |

oxc では `Box` / `Vec` / `String` が全部 arena 版に差し替わっている点に注意
（詳細は [reading-oxc.md](./reading-oxc.md) 節 2）。
この実装の「ノードだけ arena」を「全コレクション arena」に広げたものが oxc の AST、と捉えると読みやすい。

### 発展 TODO

- [ ] `Vec<Expression<'a>>` を `bumpalo::collections::Vec` に置き換える（Vec も土地に載せる）
- [ ] `String` を `&'a str` 化（節 9 string interning と合流）
- [ ] [oxc_allocator のソース](https://github.com/oxc-project/oxc/tree/main/crates/oxc_allocator) で `Box<'a, T>` の実装を読む
