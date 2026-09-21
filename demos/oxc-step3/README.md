# oxc Session 3.3 見直しデモ: 再帰下降パーサーの呼び出し順

`<` まわり (型引数 / ジェネリックなアロー関数 / 型アサーション) で、oxc の再帰下降パーサーが
**どの関数をどの順に呼ぶか**を、実際に走らせて採ったトレース。3.3 の見直し用
([reading-oxc-ts.md](../../docs/reading-oxc-ts.md) の 3.3、`parse_type_arguments_in_expression` など)。

## トレースの読み方

各 `demoN_*.ts` の隣に 2 つのファイルがある:

- `demoN_*.trace.txt` — 全部のトレース
- `demoN_*.flow.txt` — 読みやすくしたもの。ノイズの関数 (識別子を読むだけの `parse_identifier_*` や、
  式の優先順位のはしごの通過点など) を除いて字下げを詰めた

```
parse_member_expression_rest  [LAngle @2]      ← 関数名  [その関数に入った時点の現在トークン @ソースのオフセット]
  parse_type_arguments_in_expression  [LAngle @2]
    ** [checkpoint] at 2                        ← `**` は cursor.rs のイベント (checkpoint / rewind / re-lex)
    ** [re_lex L] at 2
    parse_ts_type  [Ident @4]                   ← 字下げが深い = その関数の中から呼ばれた
    ** [rewind] from 8 back to 2                ← 8 まで読んだところから 2 へ巻き戻した
```

**採り方**: oxc の `crates/oxc_parser/src` の `parse_*` / `try_parse_*` / `is_start_of*` などの関数 297 個の入口に一時的に出力を仕込み、採取後に revert
した (`git checkout`)。仕込みは [tools/instrument.py](tools/instrument.py)、採取は [tools/regen.py](tools/regen.py)。
再現手順: `python3 tools/instrument.py apply` → `cargo build -p oxc_parser --example parser` →
`python3 tools/regen.py` → `python3 tools/instrument.py revert`。oxc は rev `1aa5ec11ce`。
step0 のトレースと違い、仕込みは自動 (関数の入口だけ)。`re_lex_right_angle` などは仕込んでいない。

## 全体の流れ (どの入力にも共通)

```
parse_directives_and_statements
  └ parse_statement_list_item
      ├ (TS の宣言なら) at_start_of_ts_declaration → parse_ts_declaration_statement → parse_declaration → ...
      └ (式の文なら) parse_expression_or_labeled_statement
          └ parse_assignment_expression_or_higher
              ├ try_parse_parenthesized_arrow_function_expression   ← アロー関数を先に試す (`(` か `<` で始まるとき)
              ├ try_parse_async_simple_arrow_function_expression
              └ parse_binary_expression_or_higher                    ← 式の優先順位のはしご
                  └ (unary → update →) parse_lhs_expression_or_higher
                      ├ parse_primary_expression                     ← `f` などの読み始め
                      └ parse_member_expression_rest                 ← `.` `[` `<` `(` の後置。ここで `<` を見る
                          └ parse_type_arguments_in_expression       ← 型引数を試す (投機)
```

**型のほう** (`parse_ts_type` から先) は、メモの「型パーサー全体地図」のはしごそのまま:
`parse_ts_type` → `parse_union_type_or_higher` → `parse_intersection_type_or_higher` →
`parse_type_operator_or_higher` → `parse_postfix_type_or_higher` → `parse_non_array_type` → (`parse_type_reference` など)。
`.flow.txt` ではこのはしごが毎回出てくる。

## 式の `<`: 型引数か比較か (demo1・demo2・demo5)

| デモ | 入力 | 呼び出し順 (要点) | 結果 |
|---|---|---|---|
| demo1 | `f<T>(x);` | `parse_member_expression_rest` → `parse_type_arguments_in_expression`: checkpoint → re_lex L → `parse_ts_type` (`T`) → re_lex R → `can_follow_type_arguments_in_expr` [`(`] → 成功 → `parse_call_expression_rest` → `parse_call_arguments` | 型引数付きの呼び出し |
| demo2 | `a < b > c;` | 同じ入り口。`can_follow_type_arguments_in_expr` [`c` = Ident] → `is_start_of_expression` が真なので不可 → **`[rewind] from 8 back to 2`** → `parse_binary_expression_rest` が `<` を比較として読み直す | `(a < b) > c` |
| demo5 | `f<<T>() => T>(x);` | `<<` (ShiftLeft) で入る。checkpoint → **re_lex L** で `<` に割る → `parse_ts_type` → `parse_function_or_constructor_type` → `parse_ts_type_parameters` … `parse_return_type` → re_lex R → `can_follow` [`(`] → 成功 | 型引数 (関数型) 付きの呼び出し |

demo1 と demo2 は、`can_follow_type_arguments_in_expr` (types.rs:955) に至るまでの流れがまったく同じで、
**その 1 か所の判定だけで分かれる**。判定の中身:

```rust
match self.cur_kind() {
    Kind::LParen | Kind::NoSubstitutionTemplate | Kind::TemplateHead => true,
    Kind::LAngle | Kind::RAngle | Kind::Plus | Kind::Minus => false,
    _ => self.cur_token().is_on_new_line() || self.is_binary_operator() || !self.is_start_of_expression(),
}
```

なお `parse_type_arguments_in_expression` は、現在トークンが `<` か `<<` でなければ**checkpoint の前に** `None` で
抜ける (ソースのコメント: `a?.(` や `a?.b` の普通の経路で checkpoint / rewind の往復を避けるため)。
トレースの `[checkpoint]` が `<` の位置でしか出ないのは、そのため。

## 型の `<` / `>` (demo3・demo4)

| デモ | 入力 | 呼び出し順 (要点) |
|---|---|---|
| demo3 | `type A = Foo<<T>() => T>;` | `parse_ts_type_alias_declaration` → `parse_ts_type` → … → `parse_type_reference` → **`parse_type_arguments_of_type_reference`** [ShiftLeft]: **checkpoint なし**で re_lex L → `parse_ts_type` (関数型) …。式側と違い、`<` が型引数と決まっているので投機しない |
| demo4 | `type B = Array<Array<number>>;` + `a >> b;` | 型: 入れ子の `parse_type_arguments_of_type_reference` が 2 回 (re_lex L のみ、**re_lex R は出ない**: `>` は最初から 1 文字ずつ)。式: `parse_binary_expression_rest` [RAngle] が `>>` の結合を担う (`re_lex_right_angle` は仕込んでいないのでトレースには出ない) |

## `try_parse_type_arguments` の呼び出し元 (demo6・demo7・demo8・demo11・demo12)

`try_parse_type_arguments` (types.rs:861) は、`<` があれば型引数を読むだけの関数 (checkpoint も rewind も無い)。
呼び出し元 4 か所 (`typeof` / class の `extends` / interface の `extends` / JSX の要素名の後)。

| デモ | 入力 | 呼び出し順 (要点) |
|---|---|---|
| demo8 | `type T = typeof f<string>;` | `parse_non_array_type` → `parse_type_query` → `parse_ts_type_name` (`f`) → **`try_parse_type_arguments`** → re_lex L → `parse_ts_type` (`string`)。投機なし |
| demo7 | `interface I extends J<string> {}` | `parse_ts_interface_extends_clause`: **checkpoint** → `parse_ts_interface_heritage_type_name` (`J`) → `try_parse_type_arguments` → 次が `,` `{` `extends` `implements` EOF のどれかなので受け入れ。**rewind は起きない** |
| **demo6** | `class A extends B<string> {}` | `parse_class_extends_clause` → `parse_lhs_expression_or_higher` → `parse_member_expression_rest` → `parse_type_arguments_in_expression`: checkpoint → re_lex L → `parse_ts_type` (`string`) → re_lex R → `can_follow` [`{` = LCurly] → **`{` は式を開始できるので不可 → `[rewind] from 26 back to 17`** → 戻ってきた `parse_class_extends_clause` が **`try_parse_type_arguments` で `<string>` をもう一度読む** |
| demo11 | `class A extends B<string>` + 改行 + `{}` | demo6 と同じ入り口だが、`{` の前に改行があるので `can_follow` が `is_on_new_line()` で真 → **成功、rewind なし**。`try_parse_type_arguments` は呼ばれない (式側が作った `TSInstantiationExpression` を付け替える経路) |
| demo12 | `class A extends B<string> implements C {}` | `can_follow` [`implements`] は真 (式を開始しない) → 成功、rewind なし |

### demo6 が示すこと

`class A extends B<string> {}` は、書き方としては普通なのに、**式側の投機が一度失敗して、`<string>` が 2 回読まれる**
(1 回目: `parse_type_arguments_in_expression` の中、rewind で捨てられる。2 回目: `try_parse_type_arguments`)。
理由は、`extends` の右側が JS では式なので式パーサーが先に動くこと、そして直後の `{` が「式を開始できる」
(オブジェクトリテラル) と判定されて、`can_follow` が不可を返すこと。
`js/class.rs:210-220` の分岐と対応する:

```rust
let mut extend = self.parse_lhs_expression_or_higher();
if let Expression::TSInstantiationExpression(expr) = extend {   // 式側が成功したとき (demo11・demo12)
    extend = expr.expression;
    type_argument = Some(expr.type_arguments);                    // 付け替える
} else {
    type_argument = self.try_parse_type_arguments();              // 式側が失敗したとき (demo6): 自分で読み直す
}
```

## `<` で始まる式: アロー関数か型アサーションか (demo9・demo10)

式の**先頭**が `<` のとき。`parse_assignment_expression_or_higher` は、まず**アロー関数を試し**、だめなら
`parse_binary_expression_or_higher` (型アサーション `<T>expr` はこちら) へ進む。

| デモ | 入力 | 呼び出し順 (要点) | 結果 |
|---|---|---|---|
| demo9 | `<T>(x: T) => x;` | `try_parse_parenthesized_arrow_function_expression` → `is_parenthesized_arrow_function_expression`: checkpoint → `..._worker` → **rewind** (これは先読み) → `parse_possible_parenthesized_arrow_function_expression`: **checkpoint(error recovery)** → `parse_parenthesized_arrow_function_head` (`parse_ts_type_parameters_with_trailing_comma` で `<T>`、`parse_formal_parameters` で `(x: T)`、`parse_ts_return_type_annotation`) → `parse_arrow_function_expression_body` | ジェネリックなアロー関数。rewind なしで確定 |
| demo10 | `<string>x;` | demo9 と同じ入り口。`<string>` を**型パラメータとして**読み、`parse_formal_parameters` → `parse_ts_return_type_annotation` と進んで EOF (@11) に達し、アロー関数として成立しないので失敗 (どの時点で失敗と判定したかの内部は追っていない) → **`[rewind] from 11 back to 0`** → `parse_binary_expression_or_higher` → **`parse_ts_type_assertion`** → `parse_ts_type` (`string`) → `parse_simple_unary_expression` (`x`) | 型アサーション |

つまり `<` で始まる式は、**「ジェネリックなアロー関数として読めるか」を先に試して、失敗したら型アサーション**
の順で読まれる (投機の失敗時にエラーも巻き戻すために `checkpoint_with_error_recovery` を使う。Session 5.5 で読む場所)。

## `<` の 3 つの読み方の整理

| `<` が現れる場所 | 読み方 | 担当 | 曖昧性の解き方 |
|---|---|---|---|
| **式の途中** (`f<`) | 型引数 or 比較 | `parse_type_arguments_in_expression` | 投機 (checkpoint) + `can_follow_type_arguments_in_expr` |
| **式の先頭** (`<T>...`) | ジェネリックなアロー関数 or 型アサーション | `parse_possible_parenthesized_arrow_function_expression` → `parse_ts_type_assertion` | アローを先に投機 (error recovery 付き)、失敗したら型アサーション |
| **型引数が確定する位置** (型参照 `Foo<`、`typeof f<`、`extends B<` 等) | 型引数 | `parse_type_arguments_of_type_reference` / `try_parse_type_arguments` | 投機なし (無条件に読む)。ただし class の `extends` は式側が先に動く |
