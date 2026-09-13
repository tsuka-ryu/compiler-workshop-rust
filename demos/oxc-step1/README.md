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

## 追加 (1.3): 未移植の tsc エラー回復 — 括弧なし関数型

`parse_union_type_or_intersection_type` (types.rs:262, 268) にコメントアウトで残っている
tsc 原文 `parseFunctionOrConstructorTypeToError` が何を担っていたかの実測。

| デモ | 入力 | 結果 |
|---|---|---|
| demo6 | `type A = string \| () => void;` | **エラー** `Unexpected token` (`)` を指す)。body は空 |
| demo7 | `type B = string \| (() => void);` | OK。TSUnionType になる (正しい書き方) |
| demo8 | `type C = string & new () => void;` | **エラー** `Expected a semicolon...` (`(` を指す)。body は空 |

括弧が必要な理由: **`=>` が右を全部飲む** (関数型は `|` より弱く結合する) ため、
`string | () => void` は「`string | ()` を受け取って `void` を返す関数」とも読めてしまう。
TS は括弧を強制して曖昧性を消している。

tsc は同じ入力を **関数型として最後まで読んだうえで**
`Function type notation must be parenthesized when used in a union type` と名指しで報告する。
oxc はその回復処理を移植していないので、`(` が括弧型の開始として読まれて中身が空でコケる
(demo6) / `new` が型名として読まれた挙句 ASI エラー (demo8) と、原因から遠い診断になる。

**受理/拒否の判定は一致、失われるのはメッセージの質だけ** — 「tsc は IDE のために回復へ投資、
oxc は正しいコードを速く処理することに全振り」の具体例。

## 追加 (1.3): 前置型演算子と infer — パーサーとチェッカーの境界

`parse_type_operator_or_higher` (types.rs:282) / `parse_type_operator` (295) /
`parse_constraint_of_infer_type` (335) の挙動確認。

| デモ | 入力 | 結果 |
|---|---|---|
| demo9 | `type A<T> = keyof keyof T;` | OK。TSTypeOperator の入れ子 (295-299 の再帰の実証) |
| demo10 | `type B<T> = T extends keyof infer U ? 1 : 0;` | OK。TSTypeOperator > TSInferType |
| demo11 | `type C<T> = keyof infer U;` | **OK (エラーなし)** — だが TypeScript としては不正 |
| demo12 | `type D<T> = T extends infer U extends string ? U : never;` | OK。TSInferType に string 制約が付き、`? U : never` は外側の conditional へ |

**demo11 が肝**: `infer` は conditional type の `extends` 節の中でしか書けない。
tsc なら `'infer' declarations are only permitted in the 'extends' clause of a conditional type`
が出る (未検証。Playground で確認のこと)。oxc が黙って通すのはバグではなく **役割分担** —
「`infer` がどこに置かれているか」は祖先ノードを遡らないと判定できず、木を作り終えてからの
仕事なので tsc でも **チェッカー** が報告している。

境界線は **ローカルな情報で判定できるか**:
- 1.2 の `new (this: number) => any` → 同じ関数内で `this_param` の有無を見れば済む → パーサーが報告
- `infer` の位置制約 → 木全体を見ないと分からない → パーサーの守備範囲外

demo12 は `parse_constraint_of_infer_type` の **checkpoint なし経路** (343-346) の実証。
conditional の extends 節は既に `DisallowConditionalTypes` が立っているので、後続の `?` で
再解釈される余地がなく、投機せずに制約を読める。

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
