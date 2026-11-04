# oxc に issue を立てる: `2 ** 3 as number ** 2` が通る

ブログ第5回 (`2026-11-04-oxc-ts-parser-as.md`) の「まだ追いついていない1件」から。
oxc の `as` / `satisfies` の判定が、`**` (右結合) の同じ強さを止めない。
issue を立てる前に、自分の手で再現して確かめる (oxc の CONTRIBUTING は AI 利用の明記と、投稿者が責任を持つことを求めている)。

## 状況 (2026-11-04 に Claude が確認)

- oxc の main `98b65d8353` (2026-11-04) でも `2 ** 3 as number ** 2;` は `Parsed Successfully.`。記事の rev `1aa5ec11ce` と同じ
- `as` の腕の判定は `next_precedence > last_precedence` のまま (`crates/oxc_parser/src/js/expression.rs`、main では 1529〜1534行目)
- oxc-project/oxc の issue / PR には、対応するものが見つからなかった (`gh search` で `63661`、`4676`、`as number ** 2`、`unerasable` などを検索)
- 上流:
  - [microsoft/TypeScript#63661](https://github.com/microsoft/TypeScript/issues/63661) (magic-akari さん、7.0.2 で確認、2026-07-20)
  - [microsoft/typescript-go#4676](https://github.com/microsoft/typescript-go/pull/4676) (2026-08-14 マージ)。`shouldConsumeBinaryOperator` が、同じ強さでも `**` なら止める
- 直し方の見込み: `next_precedence > last_precedence` を「`>`、または同じ強さで次が `**`」にする (まだ実装していない)

## 手元で確認する手順

```bash
cd /Users/tsukaryu/ghq/github.com/oxc-project/oxc
git fetch origin main && git switch --detach origin/main
printf '%s\n' '2 ** 3 as number ** 2;' > /tmp/x.ts
cargo run -q -p oxc_parser --example parser -- /tmp/x.ts
# Parsed Successfully. なら再現
git switch main   # 記事の rev (1aa5ec11ce) に戻す
```

## issue の下書き (英語)

タイトル:

```text
parser: `2 ** 3 as number ** 2` is accepted, but erasing `as number` changes the meaning
```

本文 (テンプレートは `Bug report` を使い、タイトルの先頭は `parser:`):

````markdown
`**` is right-associative, so with `as` erased, `2 ** 3 as number ** 2` becomes `2 ** (3 ** 2)` (512). With the type, it parses as `(2 ** 3) as number ** 2`, i.e. `(2 ** 3) ** 2` (64). The meaning changes when the type is erased, which #22986 (port of microsoft/typescript-go#4192) was meant to reject.

The check in `parse_binary_expression_rest` compares `next_precedence > last_precedence`, so an operator of equal precedence is not stopped. That is fine for left-associative operators, but not for `**`.

## Reproduction

```sh
printf '%s\n' '2 ** 3 as number ** 2;' > x.ts
cargo run -q -p oxc_parser --example parser -- x.ts
```

Result: `Parsed Successfully.` (checked on main 98b65d8353)

## Upstream

- microsoft/TypeScript#63661 (reported against 7.0.2)
- microsoft/typescript-go#4676 (merged 2026-08-14) adds `shouldConsumeBinaryOperator`, which also stops at equal precedence when the operator is `**`.

This issue was investigated with the help of Claude (AI) and I reproduced the result myself before filing.
````

## 投稿前のチェック

- [ ] 手元で再現した (上の手順)
- [ ] 末尾の「reproduced the result myself」は、再現してから残す
- [ ] 重複 issue がないか、oxc の issue 検索をもう一度見る
- [ ] 手順 3: <https://github.com/oxc-project/oxc/issues/new/choose> から Bug report で投稿 (または `gh issue create -R oxc-project/oxc`)
- [ ] 投稿したら、ブログ第5回の「まだ追いついていない1件」に issue のリンクを足すか決める
