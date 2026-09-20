# oxc TS パーサー深読みガイド

対象: `~/ghq/github.com/oxc-project/oxc`。行番号は rev `1aa5ec11ce` (2026-09-13 時点) 基準。
ズレたら関数名で grep する。読書メモは [reading-oxc-ts-notes.md](reading-oxc-ts-notes.md)、
実証デモは `demos/oxc-stepN/` に置く。

1回の読書 = 1ユニット (30分〜1h) を目安に分割してある。ユニット単位でチェックを付ける。

## 全体像: TS パースは「3 つの世界」でできている

1. **型の文法世界** — `ts/types.rs` (1,673 行)。式とは別の再帰下降パーサーがまるごと 1 個入っている
2. **TS 固有の文** — `ts/statement.rs` (910 行)。enum / interface / type alias / namespace / declare
3. **JS 側への食い込み** — `js/expression.rs` ほか。`as` / `satisfies` / `!` / `<T>expr` / `foo<T>()` が
   **式パーサーの中に** 埋まっている。ここが一番 oxc らしい (曖昧性との戦い)

---

## Session 0: 足回り — 投機パースの仕組み (~1h) ✅ 完了

TS 文法は LL(1) では決まらない箇所だらけ。oxc の答えは **checkpoint → 試しに読む → だめなら rewind**。

- [x] 0.1 `cursor.rs:309` `checkpoint()` / `:329` `rewind()` / `:340` `lookahead()` —
      保存するのは位置とカウンタだけ。arena の AST は巻き戻さない
- [x] 0.2 `lexer/mod.rs:51` `LexerCheckpoint` / `:198` 実装本体 —
      エラー2段構え (Count/Full)、`token` 不変条件 + debug_assert
- [x] 0.3 `lexer/typescript.rs` (全部) — re-lex の L/R 非対称。
      `>` は最初から遅延字句解析、という設計判断が根
- [x] 0.4 実証: demos/oxc-step0 (AST / トークン列 / トレースの3点ダンプ)

**回収済みの問い**: checkpoint は何をコピーしていて、何をコピーしていないか → メモ参照

## Session 1: 型式の再帰下降コア — `ts/types.rs` 前半 (~3h)

エントリは `types.rs:15` `parse_ts_type`。降下の階層が固定で並んでいる:

```
parse_ts_type                          types.rs:15   … conditional は最上位でここに直書き
└ parse_union_type_or_higher           types.rs:245  … `|`
  └ parse_intersection_type_or_higher  types.rs:241  … `&` (ファイル上は union より先に定義)
    └ parse_type_operator_or_higher    types.rs:282  … keyof/unique/readonly
      └ parse_postfix_type_or_higher   types.rs:358  … `T[]` / `T[K]`
        └ parse_non_array_type         types.rs:411  … 全プライマリ型の分配器
```

- [x] 1.1 `parse_ts_type` (15-44) — 3ブロック構造 (関数型分岐 / union 呼び出し / conditional)。
      `DisallowConditionalTypes` の付け外し、`extends` 前の改行ガード。
      実証: demos/oxc-step1 (ネスト禁止 / 改行 / `extends` メンバー名)
- [x] 1.2 `parse_function_or_constructor_type` (46-84) +
      `is_start_of_function_type_or_constructor_type` (86-143) —
      判定コスト3段 (`<`/`new` 即答 / `abstract` は peek / `(` だけ投機)、
      括弧型 vs 関数型を分ける最小の証拠4パターン、`skip_parameter_start` の
      「エラー件数が増えなければ成功」判定。メモ参照
      (`parse_ts_type_parameters` (145) は未読、必要になったら)
- [x] 1.3 union → intersection → type_operator (241-356) —
      前半: 階層のつなぎ方を引数で渡す形、先頭の `|` `&`、tsc 未移植マーカーのコメント。
      後半: 前置演算子は投機不要、`readonly` の事後検査、
      `parse_constraint_of_infer_type` の「曖昧なときだけ投機」、
      パーサー/チェッカーの境界線 (`keyof infer U` が通る理由)。メモ・demos 参照
- [ ] 1.4 `parse_postfix_type_or_higher` (358) + `parse_non_array_type` (411) の match を地図として眺める —
      **match の腕の一覧 = 型の開始トークン集合の定義** という視点で

> 🔖 **次回の再開地点 (2026-09-20 時点)**
>
> `parse_constraint_of_infer_type` (types.rs:335-356) の復習は完了。`None` の意味
> (曖昧解消の合図)・`Context::DisallowConditionalTypes` が動的スコープのフラグである
> こと・4分岐 (extends なし / フラグ立ち / 曖昧+採用 / 曖昧+rewind) を demo13-15 で
> 全網羅済み。`infer T extends U` が TS 4.7 後発の拡張という経緯もメモ済み。詳細はメモの
> 「1.3 後半」セクションと `demos/oxc-step1/README.md`。
>
> **次は 1.4** `parse_postfix_type_or_higher` + `parse_non_array_type` から。

**回収済みの問い**: conditional が union より上の理由 → `(A|B) extends C ? X : Y`。メモ参照。
**式パーサーとの対比が一番の学び**: 型は演算子が少ないので階層を関数で固定した素朴な再帰下降。
自作 Pratt と読み比べる。

## Session 2: 型の難所たち — `ts/types.rs` 後半 (~3h)

行番号はガイド作成時のもの (数行ズレあり、関数名で grep)。

- [ ] 2.1 `parse_mapped_type` (~655) — `{ [K in keyof T]?: U }`。`+readonly` / `-?` の modifier 処理
- [ ] 2.2 `parse_tuple_type` (~978) / `parse_tuple_element` (~1044) —
      named tuple member の曖昧性。`is_next_token_colon_or_question_colon` (~1097) の lookahead
- [ ] 2.3 `parse_template_type` (~777) — テンプレートリテラル型。レキサーとの連携
- [ ] 2.4 `parse_type_or_type_predicate` (~1341) + asserts (~815) —
      `x is string` / `asserts x is string`。戻り値型の位置だけで許される文法。
      `parse_return_type` (1329) は Session 1.2 の関数型から呼ばれていた部品
- [ ] 2.5 `parse_infer_type` (~324) — `infer T extends U` の constraint と
      conditional の `extends` の衝突回避 (~350)

## Session 3: signature member と型引数 (~2h)

- [ ] 3.1 `parse_signature_member` (~1380) — interface body と `{ ... }` 型リテラルの共有部品
- [ ] 3.2 `parse_index_signature_declaration` (~1527) +
      `ts/statement.rs` `is_unambiguously_index_signature` —
      `[x: string]` (index signature) vs `[x]` (computed property) の判別
- [x] 3.3 `try_parse_type_arguments` (861) / `parse_type_arguments_in_expression` (914) —
      **`f<T>(x)` vs `f < T > (x)` 問題の本丸**。Session 0-1 で先取り回収済み:
      re-lex の使用箇所 / `<=` を事前に弾く理由 / `can_follow_type_arguments_in_expr` の
      follow 判定表 (955) / `is_start_of_expression` (1657)。メモ参照

**回収済みの問い**: 成功と判定する条件 → 閉じ `>` の直後が「式を開始できないトークン」
または `(` / テンプレート。demo2 (`a < b > c`) の失敗理由もメモ参照。

## Session 4: TS 固有の文 — `ts/statement.rs` (~2-3h)

- [ ] 4.1 enum (~21) / type alias (~128) / interface (~224) — 素直なので速い
- [ ] 4.2 module declaration 一族 (~392) — `namespace X {}` / `module "foo" {}` / `global {}`。
      `X.Y.Z` ネストの再帰表現 (~494)
- [ ] 4.3 `parse_declaration` (~584) / `at_start_of_ts_declaration` (~811 → 現 894) —
      **`declare` は予約語ではない** ので識別子か modifier かを lookahead で決める
- [ ] 4.4 `modifiers.rs` (934 行) — `try_parse_modifier` (~550) の投機パターン。
      キーワード3階級 (メモ参照) の3段目が全員ここにいる。
      「TS に予約語を増やせなかった歴史」をざっと眺める

## Session 5: JS 式パーサーへの食い込み — 曖昧性の最前線 (~3h)

`js/expression.rs` の中の TS を拾い読みする回。一番むずかしくて一番おいしい。

- [ ] 5.1 `as` / `satisfies` (~1322-1370) — バイナリ式ループ内での処理、
      優先順位違反 (`a + b as T` の再結合) のコメント
- [ ] 5.2 postfix `!` (TSNonNullExpression, ~896 → 現 915) — optional chain との絡み (`a?.b!`)
- [ ] 5.3 `<T>expr` 型アサーション (~1235/1257) — .ts のみ (.tsx では JSX と衝突)
- [ ] 5.4 `TSInstantiationExpression` (~770/1020/1122 → 現 920-935 で一部確認済み) —
      `foo<T>` を式として残すやつ。失敗時の `<<` 書き戻し (929/1130) は Session 0 で確認済み
- [ ] 5.5 `js/arrow.rs:18` `try_parse_parenthesized_arrow_function_expression` —
      アロー曖昧性の TS 版。唯一 `checkpoint_with_error_recovery` を使う場所 (365)。
      カバー文法 (tsc/仕様) との対比はメモの「曖昧性への対処は3階層」参照

**回収済みの問い**: `a < b > c` は .ts で `(a < b) > c` (demo2 で実証済み)。

## Session 6 (任意): 仕上げ — class/function の TS 装飾 (~1-2h)

- [ ] 6.1 `js/function.rs` — return type / `this` パラメータ / declare function。
      `parse_formal_parameters` (45) は Session 1.2 の関数型から呼ばれていた部品
- [ ] 6.2 `js/class.rs` — parameter properties / abstract / accessor
- [ ] 6.3 `js/module.rs` — `import type` / `export type` / `import x = require(...)`

---

## 読み方のコツ

- **テストを動かしながら読む**: `cargo run -q -p oxc_parser --example parser -- 適当な.ts --estree`。
  トークン列は `--example tokens_dump` (Session 0 で自作、oxc 側に未追跡ファイルで設置済み)
- 迷子になったら `parse_ts_type` (types.rs:15) と `parse_non_array_type` (types.rs:411) に戻る。
  型パーサーの全経路はこの 2 つを通る
- 自作 `src/js/` との対応: 自作の Pratt = oxc の `js/expression.rs`、
  自作にない世界 = `ts/types.rs`。「式と型で文法が別」を体で覚えるのが今回の主目的
- 「なぜこの文法?」の答えは常に tsc 側 (oxc は逐語訳移植)。oxc 側の決断は「どう速くやるか」だけ

## 進捗サマリ

- [x] Session 0: checkpoint / rewind / re-lex (完了)
- [ ] Session 1: 型式コア (1.1 済み / 残り 1.2-1.4)
- [ ] Session 2: mapped / tuple / template / predicate / infer
- [ ] Session 3: signature member (3.3 は先取り済み / 残り 3.1-3.2)
- [ ] Session 4: ts/statement.rs + modifiers.rs
- [ ] Session 5: as / satisfies / `!` / instantiation / arrow 曖昧性
- [ ] Session 6: class / function / module の TS 装飾
