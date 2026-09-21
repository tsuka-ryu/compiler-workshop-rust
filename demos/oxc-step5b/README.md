# oxc Session 5.2 デモ: 後置の `!` (TSNonNullExpression) とオプショナルチェーンの木の形

[reading-oxc-ts.md](../../docs/reading-oxc-ts.md) の 5.2。`parse_member_expression_rest` (`js/expression.rs:859`) の
`Kind::Bang if self.is_ts && !self.cur_token().is_on_new_line()` の腕は、`!` を食べて `TSNonNullExpression` で包むだけの3行。
面白いのは腕そのものではなく、**`?.` (オプショナルチェーン) と組み合わさったときの木の形** (`map_to_chain_expression`、`js/expression.rs:800-815`)。

## 木の形の実測 (`shapes.js` と `estree_shapes.js`、出力は `results.txt` / `estree_results.txt`)

`Chain` = `ChainExpression`、`Member?` / `Call?` = optional なメンバー/呼び出し、`⛓` = tsc のノードに `NodeFlags.OptionalChain` が付いている
(tsc には `ChainExpression` が無い)。oxc は rev `1aa5ec11ce`、typescript-estree は 8.26.1、tsc は 6.0.3。

| デモ | 入力 | oxc (`--estree`) | typescript-estree | tsc |
|---|---|---|---|---|
| demo1 | `a!` | `NonNull(a)` | 同じ | `NonNull(a)` |
| demo2 | `a?.b!` | `Chain(NonNull(Member?(a, b)))` | 同じ | `NonNull(Prop?⛓(a, b))` |
| demo3 | `a?.b!.c` | `Chain(Member(NonNull(Member?(a, b)), c))` | 同じ | `Prop⛓(NonNull⛓(Prop?⛓(a, b)), c)` |
| demo4 | `(a?.b)!.c` | `Member(NonNull(Paren(Chain(Member?(a, b)))), c)` | `Member(NonNull(Chain(Member?(a, b))), c)` | `Prop(NonNull(Paren(Prop?⛓(a, b))), c)` |
| demo5 | `a?.b.c!` | `Chain(NonNull(Member(Member?(a, b), c)))` | 同じ | `NonNull(Prop⛓(Prop?⛓(a, b), c))` |
| demo6 | `a!.b?.c` | `Chain(Member?(Member(NonNull(a), b), c))` | 同じ | `Prop?⛓(Prop(NonNull(a), b), c)` |
| demo7 | `a?.[0]!` | `Chain(NonNull(Member?(a, [0])))` | 同じ | `NonNull(Elem?⛓(a, 0))` |
| demo8 | `a?.()!` | `Chain(NonNull(Call?(a)))` | 同じ | `NonNull(Call?⛓(a))` |
| demo9 | `a?.b!(x)` | `Chain(Call(NonNull(Member?(a, b))))` | 同じ | `Call⛓(NonNull⛓(Prop?⛓(a, b)))` |
| demo10 | `a` 改行 `!b` | `a ; UnaryExpression` (2文) | 同じ | `a ; PrefixUnaryExpression` (2文) |
| demo11 | `a!` (**`.js`**) | **エラー** (セミコロンが必要) | (`.ts` のみ実行) | `NonNull(a)` (tsc は JS でも木は作る) |

## 読み取れること

- **oxc の木は typescript-estree と10ケースすべてで一致** (demo4 の `Paren` だけは、oxc の例が括弧をノードとして残すため。ESTree の仕様に
  括弧のノードは無い)。`oxc_parser` の出力が「typescript-estree の形に合わせる」(`null` のときと同じ方針) の、`?.` と `!` の実例
- **tsc の木は違う作り**: tsc は `ChainExpression` のようなラッパーを持たず、チェーンの中のノードに **`OptionalChain` フラグ** (⛓) を付ける。
  `!` がチェーンの途中にあれば (demo3・9) `NonNull⛓` でチェーンの一部、末尾にあれば (demo2・5・7・8) `NonNull` はフラグ無しでチェーンの外側に載る。
  oxc / typescript-estree は、**`!` がチェーンの末尾にあっても、`?.` を含む式全体を `ChainExpression` で包み、`NonNull` はその中**に入れる
- **`map_to_chain_expression` の `TSNonNullExpression` の腕が、この橋渡し**: `a?.b!` は `parse_member_expression_rest` が `Member?` を作り、
  `!` の腕が `NonNull` で包み、最後に `parse_lhs_expression_or_higher_impl` が `in_optional_chain` の旗を見て、`map_to_chain_expression` で
  `Chain` に包む。この `match` に `TSNonNullExpression` の腕が無いと、`a?.b!` の `NonNull` が `Chain` の外に出てしまう
- **括弧がチェーンを閉じる** (demo4): `(a?.b)!.c` は `Chain` が `NonNull` の**中**に閉じ、外側の `.c` はチェーンに入らない。`a?.b!.c`
  (demo3) は全体が1つの `Chain` (`?.` が失敗すると `.c` も含めて全体が `undefined`、という意味の違い)
- **改行の `!`** (demo10): `!self.cur_token().is_on_new_line()` で、改行した `!` は後置と見ず、別の文 (`!b`) になる (ASI)。`as` の腕の改行と同じ裁定
- **`is_ts` フラグ** (demo11): `.js` では `!` の腕に入らず、`_ => return lhs` に落ちるので、次の文の読み込みが `!` を見て「セミコロンが必要」のエラーになる。
  `as` (`.js` でも読んでエラーだけ出す) と違って、`!` は `.js` では**後置として読まない**。tsc の JS モードは `NonNull` の木を作る (TS 専用の構文の
  エラーは別の段階で出るはず、と思われるが、確認していない)

## 再実行

```bash
cd ~/ghq/github.com/tsukaryu/compiler-workshop-rust/demos/oxc-step5b
TS_PATH=<typescript のパス> OXC_PARSER=~/ghq/github.com/oxc-project/oxc/target/debug/examples/parser node shapes.js
TE_PATH=<@typescript-eslint/typescript-estree の dist/index.js> node estree_shapes.js
```
