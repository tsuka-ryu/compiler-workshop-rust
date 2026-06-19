# String interning (`Atom<'a>`)

[next-steps.md](./next-steps.md) 節 9 の実装メモ。
AST 中の識別子・文字列リテラルを `String`(heap allocation) ではなく
**arena 上の `&'a str` を包んだ `Atom<'a>`** に置き換える。
oxc / swc の AST に `Atom<'a>` がそこら中に出てくる理由を体感するのがねらい。

前提: 節 8 の arena 版が動いていること([arena-allocator.md](./arena-allocator.md))。

対応ファイル:
- [src/atom.rs](../src/atom.rs) — `Atom<'a>` と `Interner<'a>`
- [src/ast_arena_atom.rs](../src/ast_arena_atom.rs) — `String` → `Atom<'a>` 化した AST
- [src/parse_arena_atom.rs](../src/parse_arena_atom.rs) — `Interner` を通す parser

節 8 の [ast_arena.rs](../src/ast_arena.rs) / [parse_arena.rs](../src/parse_arena.rs) からの派生(並置スタイル、元は触らない)。

---

## 1. "intern" とは

`intern` は「**内部のテーブルに取り込んで一本化する**」という動詞(語源はラテン語 `internus` = 内部の)。

> string interning = 同じ文字列を **intern table に1個だけ確保し、同じ値は全員でそれを共有する**こと。

歴史的には Lisp の `intern`(シンボルを obarray に登録)が出どころで、
Java の `String.intern()`、Rust の oxc/swc の `Atom` も同じ系譜。

例: ソースに識別子 `count` が100回出てくるとき

- **interning なし**: `count` を見るたびに別 alloc → arena 上に "count" が100個並ぶ
- **interning あり**: 最初の1個だけ確保し、以降は同じポインタを返す → "count" は1個だけ

---

## 2. `Atom<'a>` 型

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Atom<'a>(&'a str);
```

`String`(24 byte: ptr+len+cap、所有、`Copy` 不可) と違い、
`Atom<'a>` は arena 上の文字列を**指すだけ**の 16 byte(ptr+len)で **`Copy` 可能**。

### 御利益その1: `Copy`(本命)

parser / visitor / typechecker は識別子を何度もコピーして回す。
`String` だと毎回 clone か move だが、`Atom` はポインタのコピーで済む。
**arena parser で識別子型が `Atom<'a>` なのは主にこれ**。

### 御利益その2: AST サイズ削減(ただし限定的)

`String`(24) → `Atom`(16) で 8 byte 縮む…が、**enum のサイズは最大バリアントで決まる**。
`Expression` の最大バリアントは `ArrowFunction`(`Vec`×2 + `Option`)で、そこに文字列はいない。
よって **`Expression` 全体のサイズは String 版と Atom 版で変わらない**([atom.rs](../src/atom.rs) のテストで確認)。
縮むのは `Parameter` のように「最大バリアントが文字列に支配される型」だけ(8 byte 減)。

> 教訓: 「`String`→`Atom` で AST が軽くなる」は雑な理解。enum では最大バリアント次第。

---

## 3. `Interner<'a>` — interning の本体

`Atom` は型を作っただけで、**dedup する仕組みは別物**。それが `Interner`。

```rust
pub struct Interner<'a> {
    bump: &'a Bump,
    map: HashMap<&'a str, Atom<'a>>, // これが intern table
}

impl<'a> Interner<'a> {
    pub fn intern(&mut self, s: &str) -> Atom<'a> {
        if let Some(&atom) = self.map.get(s) {
            return atom; // 既出: 新規 alloc せず使い回す
        }
        let allocated: &'a str = self.bump.alloc_str(s);
        let atom = Atom(allocated);
        self.map.insert(allocated, atom);
        atom
    }
}
```

### interning すると `==` がポインタ比較になる(最重要)

同じ `Interner` から作った `Atom` 同士は「**内容が同じ ⟺ ポインタが同じ**」が保証される。
だから `==` を「文字列を1文字ずつ比較」ではなく「ポインタ(16 byte)の一致」で代用できる。
typechecker / スコープ解決は識別子の `==` を大量にやるので、これが速い。

- `Atom::new_in`(dedup なし): `new_in("count")` 2回 → 内容は `==` だが `ptr_eq` は false
- `Interner::intern`(dedup あり): `intern("count")` 2回 → `ptr_eq` が true

(両方 [atom.rs](../src/atom.rs) のテストで実証)

---

## 4. parser への統合と借用パズル

`Interner::intern` は `&mut self`、ノード確保の `bump.alloc` は `&` 借用。
両者が**同じ式に同居すると** `self` の `&` と `&mut` が衝突して詰む。

今回は衝突しなかった。理由:

1. **`&'a Bump` は `Copy`** なので、`Parser` が `bump`(ノード用)と `interner`(文字列用)を
   両方持っても問題ない(同じ参照のコピーを2つ持つだけ)。
2. **文字列を作る文とノードを alloc する文が分かれている**。
   `let name = self.interner.intern(&s);` と `self.bump.alloc(expr)` が別々の文なので借用が重ならない。

```rust
struct Parser<'a> {
    bump: &'a Bump,         // ノード確保用 (&)
    interner: Interner<'a>, // 文字列用 (&mut)
    tokens: Vec<Token>,
    pos: usize,
}
```

もし `Expression::Binary { left: self.bump.alloc(x), name: self.interner.intern(s) }` の
ように1式で両方やる設計だったら衝突していた。AST 構造のおかげで回避できた、というのが学び。

### 実 AST で interning が効いている証明

`"const a = a + a;"` をパースすると、右辺 `a + a` の2つの `Identifier` の
`name`(`Atom`)が **同じポインタを共有**する([parse_arena_atom.rs](../src/parse_arena_atom.rs) のテスト
`repeated_identifier_is_interned_to_same_pointer`)。

---

## 5. 節 8 との繋がり

- **節 8 (arena)**: ノードを1枚の土地にまとめて置く = **確保の一本化**
- **節 9 (interning)**: 同じ文字列を1点にまとめる = **文字列の一本化**

どちらも「バラバラに持つのをやめて集約する」という同じ哲学の別レイヤー。
oxc が速いのはこの集約を AST 全体に徹底しているから。

## 6. oxc との対応

| この実装 | oxc |
|---|---|
| `Atom<'a>(&'a str)` | `oxc_span::Atom<'a>`(基本同じ。`&'a str` ラッパー) |
| `Interner<'a>` 自前 | oxc は arena への直接 alloc 中心。global interning は別途 |
| `Interner::intern` の dedup | swc は global string interning(`swc_atoms`) — 設計対比が面白い |

### 読書 TODO

- [ ] [oxc_span::Atom のソース](https://github.com/oxc-project/oxc/blob/main/crates/oxc_span/src/atom.rs)
- [ ] [swc_atoms](https://github.com/swc-project/swc/tree/main/crates/swc_atoms) — global interning との設計対比