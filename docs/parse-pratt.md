# Pratt parser と `src/parse_pratt.rs` の実装解説

## Pratt parser の本質

> **二項演算子の優先順位と結合性を、「左 bp / 右 bp」という 2 つの数字で表現する**

これだけ。bp = binding power (束縛力)。`parse_expr_bp(min_bp)` という 1 つの関数に、優先順位の異なる全ての infix 演算子をまとめ込む。

## `parse.rs` (recursive descent) との対比

このリポジトリには 2 つの parser が並置されている:

| | `parse.rs` | `parse_pratt.rs` |
|---|---|---|
| 三項 (`?:`) の処理 | `parse_expression` で分岐 | `parse_expr_bp` に統合 |
| 二項 (`+`, `*`) の処理 | `parse_binary` で loop (優先順位なし) | `parse_expr_bp` で bp 表を見ながら loop |
| Plus と Multiply | 同じ階層 → `1 + 2 * 3` が `((1+2)*3)` | bp で区別 → `1 + 2 * 3` が `1 + (2*3)` |

### parse.rs の構造 (2 階層)

```
parse_expression  ─ 三項 (?:) を分岐
                   └→ parse_binary  ─ +/* の loop
                                     └→ parse_primary  ─ atom + postfix
```

### parse_pratt.rs の構造 (1 階層)

```
parse_expression  ─ parse_expr_bp(0) を呼ぶだけ
                   └→ parse_expr_bp  ─ +/*/三項を bp 表で統一 loop
                                       └→ parse_atom  ─ atom + postfix
```

`parse_atom` の中身 (リテラル / 識別子 / 括弧 / 配列 / アロー関数 / 後置 `(` `[`) は `parse.rs` の `parse_primary` と同じ。Pratt で書き換えたのは **infix の部分だけ**。

## binding power

二項演算子は「左の項を引きつける力 (左 bp)」と「右の項を引きつける力 (右 bp)」の 2 つを持つ。`+` の力より `*` の力のほうが強いから `*` の方が優先順位が高い、というのを数字で表現する。

### このリポジトリでの bp 表 ([src/parse_pratt.rs:128-135](../src/parse_pratt.rs#L128-L135))

| 演算子 | (lbp, rbp) | 結合性 |
|---|---|---|
| `?:` (Ternary) | `(2, 1)` | 右結合 (lbp > rbp) |
| `+` (Plus) | `(3, 4)` | 左結合 (lbp < rbp) |
| `*` (Multiply) | `(5, 6)` | 左結合 (lbp < rbp) |

### 結合性のルール

- **左結合**: 左 bp < 右 bp → `a + b + c` が `(a+b)+c`
- **右結合**: 左 bp > 右 bp → `a ? b : c ? d : e` が `a ? b : (c ? d : e)`

### 優先順位のルール

- **強い演算子ほど大きい数字**にする
- 厳密には「強い演算子の左 bp > 弱い演算子の右 bp」を満たす
- 三項 (2,1) と Plus (3,4) の場合: Plus の右 bp=4 > Ternary の左 bp=2 → `a + b ? c : d` で Plus が先に閉じる
- Plus (3,4) と Multiply (5,6) の場合: Multiply の左 bp=5 > Plus の右 bp=4 → `a + b * c` で Multiply が先に結合

数字の絶対値に意味はない。相対的な大小関係だけで決まる。

## `parse_expr_bp` の解説

[src/parse_pratt.rs:137-186](../src/parse_pratt.rs#L137-L186)

### シグネチャ

```rust
fn parse_expr_bp(&mut self, min_bp: u8) -> Expression
```

`min_bp` は **「この呼び出しは、左 bp が `min_bp` 以上の infix しか食わない」というしきい値**。再帰呼びのときに「ここから先は強い演算子だけ拾ってきて」と子に伝える役割。

### 全体の流れ

```
1. lhs = atom を 1 個取る
2. loop:
   a. peek した infix の bp を取る (None なら break)
   b. lbp < min_bp なら break (= 親に返す)
   c. op を消費して、Ternary か Binary かで分岐して lhs を作り直す
3. lhs を返す
```

### 核心: `rhs = parse_expr_bp(rbp)`

二項を組むときの右辺は、`rbp` を min_bp として渡して再帰呼びする。これが優先順位と結合性を生む仕組み。

```rust
let rhs = self.parse_expr_bp(rbp);
lhs = Expression::Binary {
    left: Box::new(lhs),   // ← 前の lhs を左に押し込む
    op,
    right: Box::new(rhs),
};
```

「前の lhs を左の子に押し込む」のがループのたびに繰り返されることで、**左結合の木**が自然に伸びていく。

## トレース

bp 表 = Plus(3,4) / Multiply(5,6) で `a + b + c` と `a + b * c` を追う。

### 例1: `a + b + c` (左結合)

トークン列: `[a, +, b, +, c, EoF]`

**`parse_expr_bp(0)` (最外殻)**
| 状態 | 動作 |
|---|---|
| `lhs = atom()` | `lhs = a` |
| loop 1: peek=`+` (lbp=3, rbp=4) | `3 >= 0` で進む |
| `+` 消費、`rhs = parse_expr_bp(4)` | ↓ |

**　└ `parse_expr_bp(4)`** (1個目の `+` の右辺取得中)
| 状態 | 動作 |
|---|---|
| `lhs = atom()` | `lhs = b` |
| loop 1: peek=`+` (lbp=3, rbp=4) | **`3 < 4` で break** ⛔ |
| return `b` | |

**`parse_expr_bp(0)` に戻る**
| 状態 | 動作 |
|---|---|
| `rhs = b` | `lhs = Binary(+, a, b)` |
| loop 2: peek=`+` | `3 >= 0` で進む |
| `+` 消費、`rhs = parse_expr_bp(4)` | ↓ |

**　└ `parse_expr_bp(4)`** (2個目の `+` の右辺取得中)
| 状態 | 動作 |
|---|---|
| `lhs = atom()` | `lhs = c` |
| loop 1: peek=EoF → None | break |
| return `c` | |

**`parse_expr_bp(0)`**
| 状態 | 動作 |
|---|---|
| `rhs = c` | `lhs = Binary(+, Binary(+, a, b), c)` |
| loop 3: peek=EoF | break |
| return | `(a + b) + c` ← 左結合 ✓ |

木で見ると:

```
        Binary(+)
       /         \
  Binary(+)       c
   /    \
  a      b
```

ループのたびに **「これまでの結果」を新しい Binary の左の子に押し込んで上に伸ばしていく** ので、左に深い木 = 左結合の構造になる。

### 例2: `a + b * c` (優先順位)

トークン列: `[a, +, b, *, c, EoF]`

**`parse_expr_bp(0)` (最外殻)**
| 状態 | 動作 |
|---|---|
| `lhs = atom()` | `lhs = a` |
| loop 1: peek=`+` (lbp=3, rbp=4) | `3 >= 0` で進む |
| `+` 消費、`rhs = parse_expr_bp(4)` | ↓ |

**　└ `parse_expr_bp(4)`** (`+` の右辺取得中)
| 状態 | 動作 |
|---|---|
| `lhs = atom()` | `lhs = b` |
| loop 1: peek=`*` (lbp=5, rbp=6) | **`5 >= 4` で進む** ✓ |
| `*` 消費、`rhs = parse_expr_bp(6)` | ↓↓ |

**　　└ `parse_expr_bp(6)`** (`*` の右辺取得中)
| 状態 | 動作 |
|---|---|
| `lhs = atom()` | `lhs = c` |
| return `c` | |

**　└ `parse_expr_bp(4)` に戻る**
| 状態 | 動作 |
|---|---|
| `rhs = c` | `lhs = Binary(*, b, c)` |
| loop 2: peek=EoF | break |
| return `Binary(*, b, c)` | |

**`parse_expr_bp(0)`**
| 状態 | 動作 |
|---|---|
| `rhs = Binary(*, b, c)` | `lhs = Binary(+, a, Binary(*, b, c))` |
| return | `a + (b * c)` ← `*` が先に結合 ✓ |

木で見ると:

```
   Binary(+)
   /       \
  a    Binary(*)
        /    \
       b      c
```

`rhs = parse_expr_bp(rbp)` の **再帰呼びの戻り値が Binary になって返ってくる** から、右の子に Binary がぶら下がる。これが優先順位の効き方。

### まとめ

| 入力 | 子側の判定 | 結果 |
|---|---|---|
| `a + b + c` | `+` lbp=3 vs min_bp=4 → 3 < 4 で **break** | `(a+b)+c` 左結合 |
| `a + b * c` | `*` lbp=5 vs min_bp=4 → 5 >= 4 で **進む** | `a+(b*c)` 優先順位 |

同じ `parse_expr_bp(4)` の中でも、**次のトークンの lbp と親から渡された min_bp の比較だけで分かれる**。これが Pratt の核。

## 三項演算子の特殊形

[src/parse_pratt.rs:155-168](../src/parse_pratt.rs#L155-L168)

三項 `?:` は「`?` の後に consequent → `:` → alternate」の 3 引数を取るので、普通の二項と形が違う。

```rust
if matches!(op_tok, Token::Ternary) {
    let consequent = self.parse_expr_bp(0);  // ':' まで全部食う
    match self.advance() {
        Token::Colon => {}
        other => panic!("Expected ':' in ternary, got {other:?}"),
    }
    let alternate = self.parse_expr_bp(rbp); // rbp=1 で右結合
    Expression::Conditional { test, consequent, alternate }
}
```

ポイント:

- **consequent は `parse_expr_bp(0)`** — `?` と `:` の間は何でも入る (式を全部食ってよい)
- **alternate は `parse_expr_bp(rbp)` (= rbp=1)** — ここで再帰呼びの中で 2 個目の `?` (lbp=2) を見ると `2 >= 1` で取り込める → 右結合になる

bp 表の Ternary `(2, 1)` で「2 > 1」と書いた効果がここで出る。

## 設計判断と注意点

### parse_atom と postfix

このリポジトリでは postfix (`f()` の `(` や `arr[i]` の `[`) を **`parse_atom` の内部 loop で食う** 方式に倒している ([src/parse_pratt.rs:208-228](../src/parse_pratt.rs#L208-L228))。`parse.rs` の `parse_primary` と同じ方針。

matklad の記事や [tiny-js-parser](https://github.com/tsukaryu/tiny-js-parser) では postfix も Pratt の infix と同じ枠組みで扱っているが、本リポジトリではコードを簡潔にするため別扱いに倒した。bp で並べる必要がない (postfix は実質的に最強なので) のと、`parse.rs` からのコピーで済む利点を取っている。

### AST 型は parse.rs と共有

ロードマップ ([docs/next-steps.md](./next-steps.md) の 6 節「同じ AST を別方式で構築」) に従い、`Statement` / `Expression` / `BinaryOp` 等の AST 型は `parse.rs` のものを再利用する。Pratt 版で別 AST 型を用意する理由はない。

### bp の数字を後から増やしたくなったら

`+` を `+=` `==` `<` `>` `&&` `||` 等の追加で拡張するときは、bp 表に追加するだけ。例えば JavaScript 風の優先順位なら:

```rust
fn infix_bp(&self, tok: &Token) -> Option<(u8, u8)> {
    match tok {
        Token::Or       => Some((1, 2)),      // 左結合、最弱
        Token::And      => Some((3, 4)),
        Token::Eq       => Some((5, 6)),
        Token::LessThan => Some((7, 8)),
        Token::Plus     => Some((9, 10)),
        Token::Multiply => Some((11, 12)),
        Token::Ternary  => Some((/* 右結合の数字 */)),
        _ => None,
    }
}
```

`parse_expr_bp` 本体は触らずに済むのが Pratt の旨味。

## 読書 TODO

- [matklad / Simple but Powerful Pratt Parsing](https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html) — 元ネタ。prefix / postfix / グルーピングまで含めた完全版
- [tsukaryu/tiny-js-parser](https://github.com/tsukaryu/tiny-js-parser) — 上記の TypeScript 移植 (自作)
- [oxc_parser](https://github.com/oxc-project/oxc/tree/main/crates/oxc_parser) — 実用 parser での precedence climbing 採用例