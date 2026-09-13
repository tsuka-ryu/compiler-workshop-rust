# oxc Session 0 デモ: 投機パースと re-lex

[reading-oxc-ts-notes.md](../../docs/reading-oxc-ts-notes.md) の Session 0 の実証。
各 `demoN.ts` の隣に2種類のダンプがある:

- `demoN.estree.txt` — AST ダンプ (TS-ESTree 形式)
- `demoN.tokens.txt` — 最終的な収集済みトークン列。[tokens_dump.rs](tokens_dump.rs) で採取
  (oxc 側の `crates/oxc_parser/examples/tokens_dump.rs` にも同じものを未追跡ファイルとして設置済み。
  `cargo run -q -p oxc_parser --example tokens_dump -- <file>` で再実行できる)
- `demoN.trace.txt` — checkpoint / rewind / re-lex の発火ログ。
  oxc の `checkpoint()` / `rewind()` (cursor.rs) と `re_lex_as_typescript_l_angle`
  (lexer/typescript.rs) に一時的に `eprintln!` を仕込んで採取した (仕込みは採取後に revert 済み。
  再採取するには同じ場所に eprintln を入れ直す)

## トレースの読み方

```
demo1  f<T>(x);              [checkpoint] at 1 (LAngle)          ← 投機開始
                             (rewind なし)                        ← 成功、そのまま確定
demo2  a < b > c;            [checkpoint] at 2 (LAngle)
                             [rewind] 8 -> 2                      ← offset 8 (`c`) まで読んで
                                                                    型引数でないと判明、`<` へ巻き戻し
demo3  Foo<<T>() => T>       [re-lex L] at offset 12              ← `<<` を割った。checkpoint なし!
demo4  Array<Array<number>>  (rewind も re-lex もなし)             ← `>` 遅延字句解析のおかげ
```

各デモ先頭の `[checkpoint] at 0` は文ごとの定型 checkpoint (js/statement.rs:65 の
directive prologue 用取り置き) で、今回の主役ではない。

**demo3 の発見**: 型文脈 (`Foo<` の後) では `<` は型引数開始としか読めず曖昧性がないので、
`parse_type_arguments_of_type_reference` は **checkpoint なしで無条件に re-lex する**。
typescript.rs の doc コメントにある「`try_parse` の checkpoint」の物語は式文脈
(`f<<T>...`) 限定の話。式文脈でだけ「失敗したら `<<` を書き戻す」後始末が要るのは、
そもそも失敗がありうるのが式文脈だけだから。

再実行:

```bash
cd ~/ghq/github.com/oxc-project/oxc
cargo run -q -p oxc_parser --example parser -- <このディレクトリ>/demo2.ts --estree
```

| デモ | 入力 | 結果 | 実証していること |
|---|---|---|---|
| demo1 | `f<T>(x);` | CallExpression + typeArguments | `<` で checkpoint → 型引数の投機パース成功 |
| demo2 | `a < b > c;` | `(a < b) > c` の二重 BinaryExpression | 投機失敗 → rewind → 比較演算として読み直し。demo1 とトークン列は同型なのに AST が別物 |
| demo3 | `type A = Foo<<T>() => T>;` | 型引数に TSFunctionType | L 側 re-lex: レキサーが作った `<<` (ShiftLeft) を `<` に割る |
| demo4 | `type B = Array<Array<number>>;` + `a >> b;` | 両方正しくパース | R 側: `>` は遅延字句解析で常に単独。式文脈の `>>` はパーサー要請で結合 (Replace モード) |

遊び方: demo2 を `a < b > (c);` に変えると投機が成功して CallExpression に化ける
(`>` の直後の `(` で型引数と確定 — Session 3 の判定条件の先取り)。
demo3 の `=> T` を消すと L 側 re-lex の失敗経路 (`<<` の書き戻し) を踏める。
