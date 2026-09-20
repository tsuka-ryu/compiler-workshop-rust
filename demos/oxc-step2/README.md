# oxc Session 2 デモ: タプル型の要素の並び順検査

[reading-oxc-ts.md](../../docs/reading-oxc-ts.md) Session 2.2 / `parse_tuple_type` (types.rs:967) の
実証。各 `demoN_*.ts` の隣の `.estree.txt` が AST ダンプ (エラー時は診断メッセージ)。

再実行:

```bash
cd ~/ghq/github.com/oxc-project/oxc
cargo run -q -p oxc_parser --example parser -- <このディレクトリ>/demo1_rest_after_rest.ts --estree
```

## `seen_rest_span` / `seen_optional_span` が出す3つのエラー

`parse_tuple_type` は要素を1つ読むたびに、これまでに見た rest 要素 (`...T[]`) と optional 要素
(`a?: T` / `T?`) の位置を `Option<Span>` で覚え、並び順のルール違反を検出する。
覚えた位置は診断の「先に見た側」のラベルにそのまま使われる。

| デモ | 入力 | 結果 |
|---|---|---|
| demo1 | `[...string[], ...number[]]` | **TS1265** rest の後に rest はだめ |
| demo2 | `[...Array<string>, ...Array<number>]` | **TS1265** (`Array<...>` 形式も配列扱い) |
| demo3 | `[...a: string[], ...b: number[]]` | **TS1265** (名前付き rest も中身を見て配列扱い) |
| demo16 | `[...a: Array<string>, ...b: Array<number>]` | **TS1265** (名前付き + `Array<>`。ラベルを外して中身を判定) |
| demo4 | `[a?: string, b: number]` | **TS1257** optional の後に必須はだめ (名前付き) |
| demo5 | `[string?, number]` | **TS1257** (`T?` 形式の optional) |
| demo6 | `[...string[], a?: number]` | **TS1266** rest の後に optional はだめ |

エラー時の診断は「先に見た側 (`seen_*_span`)」と「今読んだ側」の両方にラベルが付く
(例: demo1 は `First seen here` / `Second rest element here`)。

## エラーにならないもの

| デモ | 入力 | 理由 |
|---|---|---|
| demo7 | `type A<T extends unknown[]> = [...string[], ...T]` | oxc: `...T` の `T` は配列型でも `Array<...>` でもないので `seen_rest_span` に記録されない。tsc: `...T` は `Variadic` 要素で、解決した型が配列のときだけ `Rest` に昇格する (型パラメータのままでは昇格しない) |
| demo8 | `[string, number?, ...boolean[]]` | 必須 → optional → rest の正しい並び |
| demo9 | `[...string[], number]` | rest の後に必須が来るのは許される (tsc 5.9 で `tsc_check.js` により確認済み) |
| demo10 | `[a?: string, ...boolean[]]` | optional の後の rest はよい。rest は「必須ではない要素」として扱われる |

## oxc(構文の近似)と tsc(型の解決)で結果が分かれるもの

| デモ | 入力 | oxc | tsc 5.9 |
|---|---|---|---|
| demo11 | `type S = string[]; type A = [...S, ...S];` | **エラーなし** | **TS1265** |
| demo12 | `[...string[], ...readonly number[]]` | **エラーなし** | **TS1265** |
| demo13 | `[...string[], ...ReadonlyArray<number>]` | **エラーなし** | **TS1265** |
| demo14 | `declare namespace ns { type Array<T> = T[] }` + `[...string[], ...ns.Array<number>]` | **エラーなし** | **TS1265** |

oxc が rest と数えるのは、`...` の後ろが「`X[]` という構文」か「名前が `Array` の型参照」のときだけ。
`...` の後ろの書き方ごとの比較 (前に `...string[]` がある状態で試した結果):

| `...` の後ろ | oxc | tsc 5.9 |
|---|---|---|
| `number[]` | エラー | エラー |
| `Array<number>` | エラー | エラー |
| `readonly number[]` (demo12) | OK | エラー |
| `ReadonlyArray<number>` (demo13) | OK | エラー |
| `T` (型パラメータ、demo7) | OK | OK |
| `[number, string]` (タプル、demo15) | OK | OK |
| `S` (`S = string[]` のエイリアス、demo11) | OK | エラー |
| `ns.Array<number>` (修飾名、demo14) | OK | エラー |

`parse_tuple_type` の判定 (`match rest_type`) の枝と、上の入力の対応:

| `match rest_type` の枝 | 該当する入力 | デモ |
|---|---|---|
| `TSArrayType(_) => true` | `X[]` | demo1 |
| `TSTypeReference` + `IdentifierReference` + 名前が `"Array"` => true | `Array<X>` | demo2 |
| 同上、名前が `"Array"` 以外 => false | `T` / `S` / `ReadonlyArray<X>` | demo7, demo11, demo13 |
| `TSTypeReference` + `QualifiedName` など (`_ => false`) | `ns.Array<X>` | demo14 |
| その他の型 (`_ => false`) | `readonly X[]` (`TSTypeOperator`) / `[a, b]` (タプル) | demo12, demo15 |
| (② で名前付きのラベルを外してから同じ判定) | `...a: X[]` / `...a: Array<X>` | demo3, demo16 |

oxc は「書かれた見た目が配列っぽいか」、tsc は「解決した結果が配列型か」で判定する。`T` が通るのは
両者で一致するが、それは `T` が実際に何に展開されるか (`[]` / `[a, b]` / `number[]`) がその場では
決まらないから。`readonly` / `ReadonlyArray` / エイリアスは見た目が配列らしくないだけで中身は配列なので、
oxc は見逃す。

demo11 の `S` は型エイリアスなので名前を見ただけでは配列と分からず、記録されない。tsc は `S` を解決して
配列型だと分かるので TS1265 を出す。

**エラーを出す場所が違う**: tsc ではこの3つのエラーは型の解決が要る **チェッカー**
(`checker.ts` の `checkTupleType`。`getTypeFromTypeNode` で解決した型が配列かを見る) が出している。
oxc はパーサーが構文の見た目で近似して出しているので、エイリアス越しの配列は見逃す。
Session 1.3 の「ローカルな情報で決まる検査はパーサー、木全体が要る検査はチェッカー」の境界線で言うと、
「タプル内の前の要素だけで決まる」ように見える検査でも、**判定に型の解決が要るなら本来はチェッカーの仕事**
で、oxc は構文の範囲で近似している、という例 (近似にした理由は確認していない)。

tsc 側の確認 (再実行):

```bash
TS_PATH=<typescript の built/local/typescript.js> node tsc_check.js demo11_rest_after_rest_via_alias.ts
```

## なぜ rest は1個まで? — 型パラメータが展開されて rest が2個になる場合 (demo17)

`demo17_rest_normalization.ts` を tsc に解決させた結果 (`tsc_type_of.js`、出力は `.tsc-types.txt`):

| 入力 | 解決後の型 |
|---|---|
| `A<number[]>` (`A<T> = [...string[], ...T]`) | `(string \| number)[]` — 2つの rest が **1つの union の rest に畳まれる** |
| `A<[boolean, null]>` | `[...string[], boolean, null]` — タプルは展開される |
| `A<[]>` | `string[]` |

書かれたコードで rest が2個ある形は拒否する (TS1265) が、`...T` が展開されて2個になる分は
tsc が黙って union に畳んで受け入れる。理由は `createNormalizedTupleType`
(`checker.ts:17267`) のコメントにある通り、タプル型は次の2つの形しか取れないため:

1. 必須要素 → optional 要素 → rest 0〜1個
2. 必須要素 → rest 1個 → 必須要素

`[...string[], ...number[]]` の `["a", "b", 1, 2]` のように、どこまでが最初の rest でどこからが
次の rest かが型から決まらないので、位置 (`t[3]`)・`length`・代入可否が定まらない。
