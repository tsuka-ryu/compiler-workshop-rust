# oxc Session 5.5 デモ: 三項演算子の `:` と、アロー関数の戻り値型の `:` がぶつかる形

[reading-oxc-ts.md](../../docs/reading-oxc-ts.md) の 5.5 / `parse_possible_parenthesized_arrow_function_expression`
(`js/arrow.rs:356`) の、長いコメントの実演。TS では `:` が **三項演算子 (`a ? b : c`)** と **アロー関数の戻り値型 (`(x): T => ...`)**
の 2 つの意味を持ち、条件式の真側でぶつかる。

## コメントが言っていること

```
x ? y => ({ y }) : z => ({ z })
```

`y => ...` の本体を読むとき、途中の `({ y }) : z => ({ z })` は「引数 `({ y })`、戻り値型 `z`、本体 `({ z })`」の有効なアロー関数にも読める。
もしそう読むと、**三項演算子の `:` を戻り値型の `:` と取り違える**。だから **条件式の真側 (`allow_return_type_in_arrow_function` が偽) では、曖昧な
(`Tristate::Maybe` の) アロー関数に戻り値型を許さない**。ただし、

- 読めたアロー関数の**後ろにもう 1 つ `:` が続く**なら許す (`a ? (x): string => x : null`。最初の `:` が戻り値型、2 つ目が三項の区切り)
- **`Tristate::True` で確定する形** (`(b: number, c?: string): void => ...`) は、投機の関数を通らないので許す

```rust
if !allow_return_type_in_arrow_function && has_return_type {
    if !self.at(Kind::Colon) {               // 後ろにもう1つ `:` が続かない
        self.state.not_parenthesized_arrow.insert(pos);
        self.rewind(checkpoint);             // 戻り値型を許されない場所で読んでしまった → 巻き戻す
        return None;
    }
}
```

tsc の対応 (`parseParenthesizedArrowFunctionExpression`、`parser.ts:5428` 以降) はコメントまで同じ文言 (`hasReturnColon`)。

## 木の形 (`shapes.js`、出力は `results.txt`)

`Cond(test, then, else)` = 三項演算子、`Arrow([引数], ret=戻り値型) => 本体`。oxc (rev `1aa5ec11ce`) と tsc 6.0.3 は **8 ケースすべて同じ木**。

| デモ | 入力 | 木 | `Tristate` | 投機の結果 (トレース) |
|---|---|---|---|---|
| demo1 | `x ? y => ({ y }) : z => ({ z })` | `Cond(x, Arrow([y]) => ({..}), Arrow([z]) => ({..}))` | `({ y })` が `Maybe` | **投機して巻き戻す** (`({ y }) : z => ({ z })` を戻り値型付きで全部読んだが、真側で許されず `[rewind] from 31 back to 9`) |
| demo2 | `a ? (x): string => x : null` | `Cond(a, Arrow([x], ret=string) => x, null)` | `Maybe` | **投機が成功** (後ろに `:` が続くので戻り値型を許す。`[rewind]` なし) |
| demo3 | `a() ? (b: number, c?: string): void => d() : e` | `Cond(Call(a), Arrow([b,c], ret=void) => Call(d), e)` | **`True`** (`(b:` で確定) | 投機の関数を**通らない** (`parse_possible_...` も `checkpoint(error recovery)` も出ない) |
| demo4 | `a ? (b) : c` | `Cond(a, Paren(b), c)` | `Maybe` | **投機して巻き戻す** (`(b) : c` を戻り値型付きの頭として読むが `=>` が無く `[rewind] from 13 back to 4`) |
| demo5 | `a ? b : (c) => d` | `Cond(a, b, Arrow([c]) => d)` | `Maybe` | **投機が成功** (偽側は戻り値型の制限なし。`[rewind]` なし) |
| demo6 | `a ? (b) : c => d` | `Cond(a, Paren(b), Arrow([c]) => d)` | `Maybe` | **投機して巻き戻す** (`(b) : c => d` を戻り値型 `c` 付きアローと読んだが、真側で許されず `[rewind] from 16 back to 4`。そのあと `:` は三項の区切りに、`c => d` は偽側のアローに) |
| demo7 | `(x): string => x` | `Arrow([x], ret=string) => x` | `Maybe` | **投機が成功** (最上位は戻り値型を許す) |
| demo8 | `a ? ({ y }) : z` | `Cond(a, Paren({..}), z)` | `Maybe` | **投機して巻き戻す** (`({ y }) : z` を頭として読むが `=>` が無く `[rewind] from 17 back to 4`) |

## `.flow.txt` は注釈付き

各 `demoN_*.flow.txt` には、重要な行の右に **`← ...` の注釈を直接書き込んである** (先読み / `Tristate` の結果 / 投機の開始 / 2 種類の `rewind` と、
外れた理由)。注釈は `tools/annotate.py` が付けたもので、何度実行しても同じ結果になる。`tools/regen.py` でトレースを再採取すると注釈は消えるので、
その後で `python3 tools/annotate.py` をもう一度実行する。

## トレースの読み方 (2 種類の `rewind` を区別する)

`.flow.txt` に `[rewind]` が出るが、**2 種類ある**:

```
is_parenthesized_arrow_function_expression  [LParen @4]
  ** [rewind] from 6 back to 4            ← ① 先読み (Tristate の判定)。毎回出る。`checkpoint` → `_worker` → `rewind`
parse_possible_parenthesized_arrow_function_expression  [LParen @4]
  ** [checkpoint(error recovery)] at 4     ← ② ここからが「投機」(`Maybe` のときだけ)
  ...
  ** [rewind] from 13 back to 4            ← ② 投機が外れた場合だけ。demo4・6・8・1
```

- `parse_possible_...` も `checkpoint(error recovery)` も出ない → `True` で確定した (demo3)
- `checkpoint(error recovery)` の後に `[rewind]` が**無い** → 投機成功 (demo2・5・7)
- `checkpoint(error recovery)` の後に `[rewind]` が**ある** → 投機が外れて括弧式 (と三項) として読み直した (demo1・4・6・8)

`Maybe` になる入力 (`(b)` `({ y })` `(x)`) は、**アロー関数でも括弧式でもあり得る**ので、位置 (三項の真側か、最上位か、偽側か) と、その後ろ (`=>` か `:`) を見て決まる。
demo4 と demo7 は、括弧の中身が同じ (`(b)` と `(x)`) で、`(b) : c` は括弧式 + 三項、`(x): string => x` はアロー関数、と**後ろの `=>` の有無**で分かれる。

## 再実行

```bash
cd ~/ghq/github.com/tsukaryu/compiler-workshop-rust/demos/oxc-step5c
TS_PATH=<typescript のパス> OXC_PARSER=~/ghq/github.com/oxc-project/oxc/target/debug/examples/parser node shapes.js   # 木の形
# トレース: oxc に仕込みを入れて採取し、元に戻す (demos/oxc-step3/tools/instrument.py を使う)
python3 ../oxc-step3/tools/instrument.py apply && (cd ~/ghq/github.com/oxc-project/oxc && cargo build -p oxc_parser --example parser) \
  && python3 tools/regen.py && python3 ../oxc-step3/tools/instrument.py revert
```
