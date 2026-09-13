# oxc TS パーサー深読みガイド

対象: `~/ghq/github.com/oxc-project/oxc` (rev `d814c729f0`, 2026-07-19)。
行番号はこのリビジョン基準。ズレたら関数名で grep する。

## 全体像: TS パースは「3 つの世界」でできている

1. **型の文法世界** — `ts/types.rs` (1,673 行)。式とは別の再帰下降パーサーがまるごと 1 個入っている
2. **TS 固有の文** — `ts/statement.rs` (910 行)。enum / interface / type alias / namespace / declare
3. **JS 側への食い込み** — `js/expression.rs` ほか。`as` / `satisfies` / `!` / `<T>expr` / `foo<T>()` が
   **式パーサーの中に** 埋まっている。ここが一番 oxc らしい (曖昧性との戦い)

読む順もこの順。ただし最初に「足回り」を 30 分だけ見ると後が速い。

---

## Session 0: 足回り — 投機パースの仕組み (~1h)

TS 文法は LL(1) では決まらない箇所だらけ (`<` がジェネリクスか比較か、`(` がアロー引数か括弧式か)。
oxc の答えは **checkpoint → 試しに読む → だめなら rewind**。全部ここに載っている。

- `cursor.rs:305` `checkpoint()` / `cursor.rs:325` `rewind()` — パーサー+レキサー状態の保存復元
- `cursor.rs:336` `lookahead()` — checkpoint/rewind を関数に包んだだけの薄いヘルパー
- `lexer/mod.rs:187` `LexerCheckpoint` — レキサー側で何を保存しているか
- `lexer/typescript.rs` (52 行、全部読む) — **白眉**。`foo<<T>()>` の `<<` を `<` に再字句解析する
  `re_lex_as_typescript_l_angle`。コメントが checkpoint との相互作用まで説明していて、
  「レキサーとパーサーが独立でない」ことの最良の教材

**問い**: checkpoint は何をコピーしていて、何をコピーしていないか (arena に確保済みの AST はどうなる?)

## Session 1: 型式の再帰下降コア — `ts/types.rs` 前半 (~3h)

エントリは `types.rs:15` `parse_ts_type`。降下の階層が固定で並んでいる:

```
parse_ts_type                     … conditional (`X extends Y ? A : B`) は最上位でここに直書き
└ parse_union_type_or_higher      … `|`        (types.rs:260)
  └ parse_intersection_type_or_higher … `&`    (types.rs:256)
    └ parse_type_operator_or_higher   … keyof/unique/readonly (types.rs:297)
      └ parse_postfix_type_or_higher  … `T[]` / `T[K]`        (types.rs:373)
        └ parse_non_array_type        … 全プライマリ型の分配器 (types.rs:426)
```

- **式パーサーとの対比が一番の学び**: `js/expression.rs` は Pratt (優先順位テーブル + ループ) だが、
  型は演算子が少ないので **階層を関数で固定** した素朴な再帰下降。同じ oxc 内で両方式が読み比べられる
- `types.rs:15` conditional type: `Context::DisallowConditionalTypes` の付け外しに注目。
  `T extends U ? A : B` の `extends` 節の中でネストを禁止する理由を考える
- `types.rs:86` `is_start_of_function_type_or_constructor_type` — `(` を見た瞬間には
  関数型か括弧型か分からない問題を lookahead でどう解いているか
- `types.rs:426` `parse_non_array_type` は巨大 match。ここから各プライマリ型に飛ぶ地図として使う

**問い**: なぜ conditional type は union より上の階層なのか (`A | B extends C ? ...` は何と解釈される?)

## Session 2: 型の難所たち — `ts/types.rs` 後半 (~3h)

行数の割に密度が高い順:

- `types.rs:655` `parse_mapped_type` — `{ [K in keyof T]?: U }`。`+readonly` / `-?` の modifier 処理
- `types.rs:978` `parse_tuple_type` / `types.rs:1044` `parse_tuple_element` —
  named tuple member (`[a: string, b?: number]`) の曖昧性。`types.rs:1097`
  `is_next_token_colon_or_question_colon` が lookahead で解決している
- `types.rs:777` `parse_template_type` — テンプレートリテラル型。レキサーとの連携
- `types.rs:1341` `parse_type_or_type_predicate` / `types.rs:815` asserts —
  `x is string` / `asserts x is string`。戻り値型の位置だけで許される文法
- `types.rs:324` `parse_infer_type` — `infer T extends U` の constraint と
  conditional の `extends` の衝突をどう回避しているか (`types.rs:350`)

## Session 3: signature member と型引数 — interface/type literal の共有部品 (~2h)

- `types.rs:1380` `parse_signature_member` — interface body と `{ ... }` 型リテラルの両方から呼ばれる
- `types.rs:1527` `parse_index_signature_declaration` +
  `ts/statement.rs:365` `is_unambiguously_index_signature` —
  `[x: string]` (index signature) vs `[x]` (computed property) の判別
- `types.rs:875` `try_parse_type_arguments` / `types.rs:925` `parse_type_arguments_in_expression` —
  **`f<T>(x)` vs `f < T > (x)` 問題の本丸**。Session 0 の re-lex がここで使われる。
  失敗したら rewind して「ただの比較演算」として読み直す流れを追う

**問い**: `parse_type_arguments_in_expression` が成功と判定する条件は? (`f<T>` の直後に何が来たら型引数として確定?)

## Session 4: TS 固有の文 — `ts/statement.rs` (~2-3h)

- `statement.rs:21` enum / `statement.rs:128` type alias / `statement.rs:224` interface — 素直なので速い
- `statement.rs:392` module declaration 一族 (`namespace X {}` / `module "foo" {}` / `global {}`) —
  `statement.rs:494` の `X.Y.Z` ネストの再帰表現が面白い
- `statement.rs:584` `parse_declaration` / `statement.rs:811` `at_start_of_ts_declaration` —
  **`declare` は予約語ではない** ので `declare` が識別子か modifier かを lookahead で決める。
  `statement.rs:847` の worker が本体
- `modifiers.rs` (934 行) — `public` / `readonly` / `abstract` / `declare` / `override` …
  全部「文脈次第で識別子にもなる」。`modifiers.rs:550` `try_parse_modifier` の投機パターン。
  ざっと眺めるだけでも「TS に予約語を増やせなかった歴史」が読み取れる

## Session 5: JS 式パーサーへの食い込み — 曖昧性の最前線 (~3h)

`js/expression.rs` の中の TS を拾い読みする回。ここが一番むずかしくて一番おいしい。

- `expression.rs:1322-1370` — `as` / `satisfies`。バイナリ式のループの中で処理し、
  優先順位違反 (`a + b as T` の再結合) をどう扱うかのコメントが濃い
- `expression.rs:896` あたり — postfix `!` (TSNonNullExpression)。optional chain との絡み
  (`expression.rs:788` `a?.b!` を chain にどう畳むか)
- `expression.rs:1235` / `1257` — `<T>expr` 型アサーション (.ts のみ。.tsx では JSX と衝突するので不可)
- `expression.rs:770` / `1020` / `1122` — `TSInstantiationExpression` (`foo<T>` を式として残すやつ)。
  後続トークンによって call になったり式のまま残ったりする分岐
- `js/arrow.rs:18` `try_parse_parenthesized_arrow_function_expression` —
  `<T>(x) => x` / `(x: T) => x` を checkpoint 付きで試すアロー曖昧性の TS 版。
  .tsx で `<T,>() => {}` にカンマが要る理由がコードで分かる

**問い**: `a < b > c` は .ts で何と解釈されるか。パーサーはどの時点でどっちに倒すか。

## Session 6 (任意): 仕上げ — class/function の TS 装飾 (~1-2h)

- `js/function.rs` — return type annotation / `this` パラメータ (`ts/statement.rs:802`) / declare function (`ts/statement.rs:687`)
- `js/class.rs` — parameter properties (`constructor(private x: number)`) / abstract / accessor
- `js/module.rs` — `import type` / `export type` / `import x = require(...)` (`ts/statement.rs:724`)

---

## 読み方のコツ

- **テストを動かしながら読む**: `just example parser` 系より、素直に
  `cargo run -p oxc_parser --example parser -- 適当な.ts` で AST ダンプを見つつ読むと速い
- 迷子になったら `parse_ts_type` (types.rs:15) と `parse_non_array_type` (types.rs:426) に戻る。
  型パーサーの全経路はこの 2 つを通る
- 自作 `src/js/` との対応: 自作の Pratt = oxc の `js/expression.rs`、
  自作にない世界 = `ts/types.rs`。「式と型で文法が別」を体で覚えるのが今回の主目的

## 進捗

- [x] Session 0: checkpoint / rewind / re-lex (メモ: [reading-oxc-ts-notes.md](reading-oxc-ts-notes.md))
- [ ] Session 1: 型式コア (parse_ts_type → non_array)
- [ ] Session 2: mapped / tuple / template / predicate / infer
- [ ] Session 3: signature member / try_parse_type_arguments
- [ ] Session 4: ts/statement.rs + modifiers.rs
- [ ] Session 5: as / satisfies / `!` / instantiation / arrow 曖昧性
- [ ] Session 6: class / function / module の TS 装飾
