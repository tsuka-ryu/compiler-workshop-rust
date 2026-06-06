# `src/traverse_binary_only.rs` の実装解説

## このドキュメントの位置づけ

- [docs/visit.md](./visit.md) で扱った Visit / VisitMut が前提。「walk と visit を分離する」基本パターンは既知とする。
- ここでは **Visit/VisitMut では届かない場面** と、そのために oxc が採用している **Traverse**（親アクセス付き visitor）の設計を解説する。
- `traverse_binary_only.rs` はその設計を **`Expression::Binary` 1 つだけ** に絞った最小版。全 variant 版は別途 [src/traverse.rs](../src/traverse.rs) にある。

## なぜ Traverse か

Visit / VisitMut は強力だが、**「今のノードの兄弟・親を見たい」が出てくると詰む**。たとえば:

- 「`Binary` の `left` を訪問中に、同じ `Binary` の `right` 側も覗いて両者の組合せで判定したい」
- 「識別子の使用箇所（`Expression::Identifier`）を訪問したとき、それが代入の LHS か RHS かを親から判定したい」
- 「関数呼び出しの引数を訪問中に、`callee` の名前で挙動を分岐したい」

VisitMut の `visit_expression(&mut self, expr: &mut Expression)` は **自分のノードしか引数で受け取らない**。親のスタックは外側から自前で `Vec` を持って push/pop する手もあるが、

1. 利用者が毎回スタック管理を書くのは面倒（VisitMut のデフォルト実装が `walk_*` を呼ぶだけなのと噛み合わない）
2. 親を `&Expression` で持ったまま `&mut self` で子を書き換えると **借用が衝突する** ことが起きる

の 2 点で行き詰まる。oxc の [`oxc_traverse`](https://github.com/oxc-project/oxc/tree/main/crates/oxc_traverse) はこの 2 点を **trait 側で吸収する** ことで、利用者が `enter_*` / `exit_*` のメソッド内で常に「親」「祖先」を読める API を提供している。

## スコープを Binary 1 つに絞った理由

`Expression` だけでも 10 variant あり、それぞれの子スロットごとに親アクセサ用の構造体が要る（後述）。**全部書くと 500 行を超える**。

最小公倍数として残るのは:

1. **Ancestor の表現** = 親 variant × 自分がいる子スロット
2. **「自分の枝は見せない」アクセサ** = `XxxWithoutY` という構造体
3. **walk が降りる前後で push/pop する**

この 3 つの構造が一目で見える形に絞ったのが `traverse_binary_only.rs`。**`Expression::Binary` だけサポートし、それ以外の variant は内部で「子なし」扱い**。実装の核心が 200 行で読み切れる。

## 設計の核 1: Ancestor enum

```rust
pub enum Ancestor<'a> {
    None,
    BinaryLeft(BinaryWithoutLeft<'a>),
    BinaryRight(BinaryWithoutRight<'a>),
}
```

[src/traverse_binary_only.rs:9-17](../src/traverse_binary_only.rs#L9-L17)

ポイントは **「親は Binary」だけでなく「Binary のどの枝にいるか」まで variant に焼き込んでいる** こと。

なぜか？ **「自分の枝は読ませない」という不変条件を型レベルで守る** ため。例えば `BinaryLeft` の人（＝ `left` を訪問中）に親から `right` だけ見せたい。そのために `BinaryLeft` には `right` と `op` と `span` だけを返すアクセサ（後述の `BinaryWithoutLeft`）を持たせる。`left` を返すアクセサは生やさない。

もし `Ancestor::Binary(Binary)` のように親を 1 つにまとめてしまうと、利用者は `match` で `left` 含む全フィールドを覗けてしまう。自分の枝を間接的に読んで処理すると **借用安全性も意味論も壊れる** ので、「枝ごとに別 variant」で物理的に塞ぐ。

## 設計の核 2: `BinaryWithoutLeft` / `BinaryWithoutRight`

```rust
pub struct BinaryWithoutLeft<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}
```

[src/traverse_binary_only.rs:37-40](../src/traverse_binary_only.rs#L37-L40)

「親の Binary 全体を指すが、left は見せない」アクセサ構造体。フィールドは 2 つしかない:

### `ptr: *const Expression` — なぜ生ポインタか

`&'a Expression` ではなく `*const Expression`（生ポインタ）を保持しているのが肝。借用チェッカに **借用と見なされない** からこそ、`walk` が同じノードに `&mut` で降りているのと両立できる。

VisitMut の素朴な拡張で、Vec に親の `&Expression` を積もうとすると:

```rust
let parent: &Expression = current; // &Binary を借用
walk(&mut *current);               // ← 同じノードに &mut で降りる → 借用エラー
```

になる。生ポインタなら借用としてカウントされないので、利用者が `parent()` 経由で参照を取りたいタイミングまで `&` 化を遅らせられる。**aliased XOR mutable** という Rust の鉄則を、unsafe で局所的に緩める設計。

### `_phantom: PhantomData<&'a Expression>` — なぜ必要か

- **PhantomData は実体が 0 バイトだが、型システム上は `T` を持っていると見なせる特殊型。** `PhantomData<&'a Expression>` と書くことで「この struct は `&'a Expression` を借りているのと同じ寿命制約を持つ」と表明する。
- **使わないと `parameter 'a is never used` エラーになる。** `*const Expression` には `'a` が出てこないので、`'a` をどのフィールドでも使っていないことになり、Rust が「不要なライフタイムパラメータ」を蹴る。`PhantomData` でその矛盾を黙らせるのが慣用句。
- `_` プレフィックスは「未使用変数だが意図的」のサイン。

### アクセサは「他の枝だけ」露出

```rust
impl<'a> BinaryWithoutLeft<'a> {
    pub fn op(&self) -> &'a BinaryOp { /* ... */ }
    pub fn right(&self) -> &'a Expression { /* ... */ }
    pub fn span(&self) -> Span { /* ... */ }
}
```

[src/traverse_binary_only.rs:42-72](../src/traverse_binary_only.rs#L42-L72)

`op` / `right` / `span` の **フィールド単位のアクセサだけ** を提供する。`binary() -> &Expression` のような「親全体」を返すメソッドは敢えて生やさない。それを生やすと利用者が `match` で `left` まで覗けてしまうから。

中身は `unsafe { match &*self.ptr { ... } }`。`*const Expression` を `&Expression` に戻して match している。`unreachable!()` 枝は型では塞ぎきれない部分（「この ptr が指すのは必ず Binary variant」という不変条件）を実行時で守る保険。

#### SAFETY コメントが約束していること

> `BinaryWithoutLeft` が作られるのは walk が `Expression::Binary` を訪問してる最中だけ。この間 ptr は有効な Binary を指していると保証される。

walk 関数のループ内で **push してすぐ降り、降りた walk が return したら pop する**。つまり `BinaryWithoutLeft` が stack に乗っている期間は、その親 Binary を保持している `&mut *ptr` の借用期間より短い（**入れ子になっている**）。生ポインタが指している先が「いつ消えるか」を walk の構造で人間が保証する。

## 設計の核 3: walk が push/pop で stack を回す

```rust
unsafe fn walk_expression<'a, T: Traverse<'a>>(
    t: &mut T,
    ptr: *mut Expression,
    ctx: &mut TraverseCtx<'a>,
) {
    unsafe { t.enter_expression(&mut *ptr, ctx) };          // 1. enter
    match unsafe { &mut *ptr } {
        Expression::Binary { left, right, .. } => {
            // left を訪問する間、親は「left を見せない Binary」
            ctx.stack.push(Ancestor::BinaryLeft(BinaryWithoutLeft {
                ptr,                                         //   親 (Binary) 自身を指す
                _phantom: PhantomData,
            }));
            unsafe { walk_expression(t, &mut **left as *mut _, ctx) };
            ctx.stack.pop();

            // right を訪問する間、親は「right を見せない Binary」
            ctx.stack.push(Ancestor::BinaryRight(BinaryWithoutRight { /* ... */ }));
            unsafe { walk_expression(t, &mut **right as *mut _, ctx) };
            ctx.stack.pop();
        }
        _ => {}                                              // 葉ノードは何もしない
    };
    unsafe { t.exit_expression(&mut *ptr, ctx) };            // 3. exit
}
```

[src/traverse_binary_only.rs:133-161](../src/traverse_binary_only.rs#L133-L161)

3 段階の順序 **enter → children → exit** は固定。これを入れ替えると pre/post-order の規約（`enter_*` が pre、`exit_*` が post）が壊れる。

children の左右順 `left → right` は **ソース上の登場順を保つ** 慣習。Linter / Formatter / Transformer が「左から右へ」処理する前提を共有できる。

### 「ptr を `*const` と `*mut` で使い分けている」点

- walk 内部は `*mut Expression`（書き換えたい）
- stack に積む `BinaryWithoutLeft.ptr` は `*const Expression`（読むだけ）

同じノードを指すポインタを mut/const 両方で持っている。これは **`walk` が `&mut *ptr` でノードを書き換えている瞬間に、利用者が `parent().right()` で `&right` を覗ける** ような芸当を可能にする。Rust の借用ルール下では普通できないので、`unsafe` で局所的にだけ実現する。

設計者と利用者の責任分界:

- **walk の作者** は「stack に乗っている `*const` は、それを `&` 化して読むときに有効か」を保証する責任を負う。これは push/pop が入れ子になっているという walk 構造の不変条件で保証している。
- **Traverse の利用者** は「`enter_*` / `exit_*` 内で受け取った `&mut Expression`（自分のノード）と、`ctx.parent()` 経由で得た `&` 系参照（親のフィールド）を同時に持っても OK」と信頼してよい。

## TraverseCtx と trait

```rust
pub struct TraverseCtx<'a> {
    stack: Vec<Ancestor<'a>>,
}

impl<'a> TraverseCtx<'a> {
    pub fn parent(&self) -> &Ancestor<'a> { /* stack.last() */ }
}

pub trait Traverse<'a> {
    fn enter_expression(&mut self, _expr: &mut Expression, _ctx: &mut TraverseCtx<'a>) {}
    fn exit_expression(&mut self, _expr: &mut Expression, _ctx: &mut TraverseCtx<'a>) {}
}
```

[src/traverse_binary_only.rs:114-127](../src/traverse_binary_only.rs#L114-L127)

VisitMut との差は 2 点だけ:

1. メソッドが `enter_*` / `exit_*` の **2 種類** に分かれた（pre-order と post-order）
2. メソッドが `ctx: &mut TraverseCtx<'a>` を **追加で受け取る** ようになった。これ越しに `ctx.parent()` で親を読める

エントリポイント `traverse_mut` は最初に `Ancestor::None` を 1 つ積んでから walk を始める ([src/traverse_binary_only.rs:171-178](../src/traverse_binary_only.rs#L171-L178))。これで利用者は `stack.last()` が常に `Some` であることを当てにできる（ルート訪問中は `Ancestor::None` が返る）。

## 使用例: `SiblingReader`

[src/traverse_binary_only.rs:187-208](../src/traverse_binary_only.rs#L187-L208) のテスト visitor は、`x + 1` をトラバースして「葉ノードを訪問しているときの兄弟」を記録する。

```rust
impl<'a> Traverse<'a> for SiblingReader {
    fn enter_expression(&mut self, expr: &mut Expression, ctx: &mut TraverseCtx<'a>) {
        let is_leaf = matches!(
            expr,
            Expression::Number { .. } | Expression::Identifier { .. }
        );
        if !is_leaf {
            return;
        }
        let sibling = match ctx.parent() {
            Ancestor::None => "no_parent".to_string(),
            Ancestor::BinaryLeft(p) => format!("right={}", label(p.right())),
            Ancestor::BinaryRight(p) => format!("left={}", label(p.left())),
        };
        self.siblings_seen.push(sibling);
    }
}
```

`x + 1` のトラバース順序と stack の状態:

```
walk_expression(Binary(x, +, 1))
  enter(Binary)             stack: [None]                      葉でない → 何もしない
  push(BinaryLeft(parent=Binary))
  walk_expression(x)        stack: [None, BinaryLeft]
    enter(x)                                                    葉 → parent=BinaryLeft → ★ "right=Number(1)" を記録
                                                                   (p.right() で 1 を覗く)
  pop
  push(BinaryRight(parent=Binary))
  walk_expression(1)        stack: [None, BinaryRight]
    enter(1)                                                    葉 → parent=BinaryRight → ★ "left=Ident(x)" を記録
                                                                   (p.left() で x を覗く)
  pop
  exit(Binary)              stack: [None]
```

期待結果は `["right=Number(1)", "left=Ident(x)"]`。**親に登っていない**（`ctx.stack` を自前で触ってない）のに、葉ノード位置で「同じ Binary の反対側の枝」が読めている。これが Traverse の旨味。

## binary_only から全 variant への拡張

`traverse_binary_only.rs` の延長で `Conditional` / `Call` / `Array` / `Member` / `ArrowFunction` / `Statement` / `TypeAnnotation` / `Parameter` も同じパターンで足せる。1 つの「親 variant × 子スロット」ごとに:

- `Ancestor` に variant を 1 つ追加
- `XxxWithoutY<'a>` 構造体と他フィールド用アクセサを生やす
- walk の対応 match arm に push/pop を仕込む

全部書いたのが [src/traverse.rs](../src/traverse.rs)。同じ設計で **構造体が 19 個、Ancestor variant が 21 個** に膨れる。oxc がここで [codegen (tasks/ast_tools)](https://github.com/oxc-project/oxc/tree/main/tasks/ast_tools) を採用しているのは、この **量的な辛さ** が動機。

### Vec 子の扱い

`Call.arguments: Vec<Expression>` のような Vec 子フィールドは、それぞれの要素を訪問する間 push/pop することになる。ただし `Ancestor::CallArguments(CallWithoutArguments)` が **自分のインデックス** を持たないなら「同じ Vec 内の他の要素」も区別できず、安全に晒すのは Vec 全体を隠すしかない。実用版は「自分の index」を ancestor に持たせ、`element(other_index) -> Option<&T>` のようなアクセサで「自分以外の兄弟」だけ覗かせる。`src/traverse.rs` は前者の単純版を採用している。

## まとめ

1. **Visit/VisitMut は親が見えない → Traverse は ancestor stack で親を見せる**
2. **「親 + 自分がいる子スロット」を 1 つの Ancestor variant に詰める** ことで、利用者が自分の枝を覗くのを型レベルで防ぐ
3. **生ポインタ + PhantomData で借用チェッカを回避** し、`walk` の `&mut *ptr` と利用者の `parent().xxx()` を共存させる
4. **stack の push/pop は walk が責任を持つ** ことで、利用者は `enter_*` / `exit_*` だけ書けば良い
5. **binary_only は核を 200 行で見るための最小例** であり、全 variant 版は [src/traverse.rs](../src/traverse.rs) に並置する（実用では codegen するくらい量が出る）

参考: [oxc_traverse](https://github.com/oxc-project/oxc/tree/main/crates/oxc_traverse) と [oxc_ast_visit](https://github.com/oxc-project/oxc/tree/main/crates/oxc_ast_visit) の対比を読むと、「Visit と Traverse をなぜ別 crate に分けるか」の判断が見える。