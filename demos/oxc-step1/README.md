# oxc Session 1 デモ: conditional type の2つのガード

[reading-oxc-ts.md](../../docs/reading-oxc-ts.md) Session 1 / `parse_ts_type` (types.rs:15-44) の
宿題2問の実証。各 `demoN_*.ts` の隣の `.estree.txt` が AST ダンプ (エラー時は診断メッセージ)。

再実行:

```bash
cd ~/ghq/github.com/oxc-project/oxc
cargo run -q -p oxc_parser --example parser -- <このディレクトリ>/demo1_nested_extends.ts --estree
```

## Q1: なぜ extends 節では conditional をネスト禁止にするのか

| デモ | 入力 | 結果 |
|---|---|---|
| demo1 | `T extends U extends V ? X : Y` | **エラー**: `` Expected `?` but found `extends` `` |
| demo2 | `T extends (U extends V ? X : Y) ? 1 : 0` | OK (括弧なら合法) |
| demo3 | `T extends string ? 1 : T extends number ? 2 : 3` | OK (false 側のネストは常時合法) |

`DisallowConditionalTypes` フラグの正体 = **「extends 節に裸の conditional を書くことの禁止」**。
demo1 のエラーはフラグの動作そのもの: extends 節のパースが `U` で止まり (フラグが立って
いるので2つ目の `extends` を食べない)、外側の `expect(Kind::Question)` が `extends` に
ぶつかる。

禁止する理由は **`?` と `extends` の対応付けを決定的にするため**。ネストを許すと
`T extends U extends V ? X : Y` の `? X : Y` が内側・外側どちらの `extends` に属すか
先読みだけでは決められず、バックトラック必須になる。禁止すれば「各 `extends` は
直近の `?` と組む」が一発で決まり、書き手は括弧で意図を明示させられる (demo2)。
チェーンは demo3 のように false 側に書く (三項演算子と同じ右結合)。

## Q2: なぜ `extends` の前の改行で conditional を打ち切るのか (types.rs:22)

| デモ | 入力 | 結果 |
|---|---|---|
| demo4 | `type X<T> = T` ␤ `extends string ? 1 : 0;` | **エラー**: Unexpected token |
| demo5 | `interface I {` ␤ `a: string` ␤ `extends: number` ␤ `}` | OK: `extends` という名前のプロパティ |

TS では予約語もメンバー名に使える (`extends: number` は合法なプロパティ宣言)。
demo5 の `a: string` はセミコロンなしで行末を迎えるので、次行の `extends` が
「conditional の開始」か「次のメンバーの名前」か曖昧になる。
`!is_on_new_line()` ガード (types.rs:22) は **改行が来たらメンバー名側に倒す** という
ASI 的な裁定。代償として demo4 のように conditional の `extends` を行頭に置く書き方が
禁止される (tsc の `hasPrecedingLineBreak()` と同じ挙動。`T extends` は同じ行に
書かなければならない — prettier がそう整形するのはこのため)。
