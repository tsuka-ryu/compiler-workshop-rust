# LT: oxcのTypeScript parserを眺める

TSKaigi の5分の LT の内容を詰めるためのメモ。材料はブログ連載「oxcのTypeScriptパーサーを読む」(第0〜8回) と
[reading-oxc-ts-notes.md](reading-oxc-ts-notes.md)。

## 決めたこと

2026-10-03 に grill-me で詰め、2026-10-11 にスライドを1枚ずつ詰めながら直した。[reading-oxc-ts-notes.md](reading-oxc-ts-notes.md) の「LT の軸候補 (Session 2.2 時点)」の節にある骨格案とゴールは旧案。

| 項目 | 決定 |
|---|---|
| タイトル | oxcのTypeScript parserを眺める |
| 対象 | TS は書くが、パーサーは未経験の人 |
| ゴール | TS の構文の見方が変わる (コードは読まなくていい) |
| 一言 | TS は JS の中に住んでいる (締めで言う) |
| 主語 | oxc のパーサー (体験談として)。tsc との違いは基本的に触れない |
| 例 | `a < b > (c);` と `a < b > c;` の2つを並べる (違いは `c` の括弧だけなのに、関数呼び出しと比較に分かれる)。並べる順もたどる順も、成功する側 (呼び出し) が先 |
| 用語 | パーサー・AST・投機パースなどは、さらっと説明するだけ |
| 見取り図 | 2つの文がたどる関数の道を、パーサーのファイル構成の上に重ねた図。どこからが TS かを色で見せる |
| コード | 見せない |
| 締め | 一言を言うだけ (ブログには触れない) |
| 流れ | 「TypeScript parser を眺めに行ったら、TypeScript parser というものは無かった。TS は JS の中に住んでいた」でタイトルとオチをつなぐ |

裏取り (2026-10-03、oxc rev `1aa5ec11ce`、tsc 6.0.3): `a < b > (c);` は oxc・tsc とも `CallExpression`、`a < b > c;` は二重の `BinaryExpression` (oxc はブログ第3回で確認)。

## スライド (詰めている途中)

15枚、1枚20秒ほどでテンポよく話す (2026-10-11)。1枚で言うことは1つにする。6〜14枚目は同じファイル構成のツリーを使い回し、道を1区間ずつ伸ばしていく。たどるのは成功する `a < b > (c);` が先で、巻き戻る `a < b > c;` が後 (最初の道を一本道にして、「試しに」の意味を後半で効かせる。ブログ第3回と同じ順)。

| # | 画面に出すこと | 話すこと |
|---|---|---|
| 1 | タイトル「oxcのTypeScript parserを眺める」 | タイトルを読む |
| 2 | 自己紹介: 名前、所属 | 名前と所属。「仙台なので牛タンを食べにきました」 |
| 3 | 2つの文 `a < b > (c);` `a < b > c;` | 「今日はこの2つの文を例に、oxc のパーサーを眺めていこうと思います。片方は関数呼び出しで、片方は比較。これをパーサーがどうやって見分けているのかを見ていきます」(どっちがどっちかは見れば分かるので伏せない。問いは「どう見分けているか」) |
| 4 | 2つの文を左右2列に並べて、レキサーで切ったトークン列を出す。左 `[a] [<] [b] [>] [(] [c] [)] [;]`、右 `[a] [<] [b] [>] [c] [;]` | 「ソースコードは、まずレキサーが単語、トークンに切ります。`<` と `>` はどちらの文でも同じトークンで、違うのは括弧だけです」(`LAngle` `RAngle`、oxc で確認済み)。oxc の説明はしない (聴衆は知っている) |
| 5 | 4枚目の下に、パーサーが作る AST を足す。左 `CallExpression` (callee: `Identifier "a"` / typeArguments: `TSTypeParameterInstantiation` (params: [`TSTypeReference "b"`]) / arguments: [`Identifier "c"`])。右 `BinaryExpression ">"` (left: `BinaryExpression "<"` (left: `Identifier "a"` / right: `Identifier "b"`) / right: `Identifier "c"`)。木は TSESTree (oxc の `--estree`) の名前のまま、位置・`optional`・空の欄を省いた形 | 「それをパーサーが読んで、AST という木にします。トークンはほとんど同じなのに、木はまったく違う。見分けているのはパーサーです」 |
| 6 | oxc のパーサーのファイル構成のツリー (rev `1aa5ec11ce` で数え直し済み: 全体 24,406 行 / lexer/ 8,256 / js/ 7,171 / ts/ 2,654 / jsx/ 582 / 共通の部品 5,743)。`ts/` を強調 | 「oxc のパーサーのファイルはこんな構成です。TypeScript parser を眺めに来たのに、`ts/` は全体の1割ほどしかありません」。ファイル構成の数字はここで一度だけ言う |
| 7 | 同じツリーの上に、`a < b > (c);` の道を `js/` の中だけ引く: `js/statement.rs` の `parse_statement_list_item` → `js/expression.rs` の `parse_assignment_expression_or_higher` → `parse_binary_expression_or_higher` → `parse_lhs_expression_or_higher` → `parse_member_expression_rest` | 「oxc のパーサーは、再帰下降という作り方です。文法の規則1つにつき関数が1つあって、文の関数が式の関数を呼び、式の関数がもっと細かい式の関数を呼ぶ、と上から順に呼び降りていきます。括弧の中にまた式があれば、上の関数をもう一度呼ぶ。だから『再帰』です。`a < b > (c);` も、`js/` の関数をこうやって降りていきます」。口頭で軽く説明するだけで、再帰する様子の図は出さない。細かい分岐は追わない |
| 8 | 道を伸ばす: `parse_member_expression_rest` で `<` を見る → `ts/types.rs` の `parse_type_arguments_in_expression` | 「`<` を見たところで、初めて `ts/` に入ります。試しに型引数として読んでみます」 |
| 9 | 道を伸ばす: 型引数として読めた → `js/expression.rs` の `parse_call_expression_rest` | 「読めたので、そのまま呼び出しとして組み立てます」 |
| 10 | 同じツリーに `a < b > c;` の道。`parse_type_arguments_in_expression` までは7〜8枚目と同じ道 | 「`a < b > c;` も、途中までは同じ道です。`<` を見て `ts/` に入り、試しに型引数として読む」 |
| 11 | 道を伸ばす: 型引数としてはだめ → 戻って `js/expression.rs` の `parse_binary_expression_rest` | 「でも、こちらはだめでした。なので戻って、`<` を比較として読み直します」 |
| 12 | どこが違う? 閉じた `>` の直後の1トークン: `(` なら呼び出し、`c` のように式が始まってしまうなら却下 | 「どこが違ったのか。型引数として読み終えたあと、閉じた `>` の直後の1トークンだけを見ています。`(` なら呼び出し。`c` のように式が始まってしまうなら却下して、比較として読み直します」。根拠は `ts/types.rs` の `can_follow_type_arguments_in_expr` (ブログ第3回) だが、コードは見せない |
| 13 | どこからが TS? 2つの道を1枚に重ね、`js/` の区間と `ts/` の区間を色分けする。`ts/` は `parse_type_arguments_in_expression` の1か所だけ | 「2つの道を重ねて、TS の部分に色を塗るとこうなります。TS だったのは、`<` を見て型引数として読んでみた、この一瞬だけ。入るのも `js/` から、戻って比較や呼び出しを組み立てるのも `js/` です」。根拠はブログ第3回の「文の入口から、型パーサーが呼ばれるまで」(TS に入るのは呼び出しの13行目が最初) |
| 14 | 13枚目の入口 (`parse_member_expression_rest` で `<` を見るところ) に「TS のファイルなら」の札を付ける | 「しかもこの入口には『TS のファイルなら』という条件が1つ付いているだけ。`.js` ならここには入らず、そのまま比較として読みます」。根拠はブログ第3回の「境界は、後置を読むループの中の腕」(`Kind::LAngle \| Kind::ShiftLeft if self.is_ts`、`js/expression.rs:920`)。コードは見せない |
| 15 | 締め: 「TypeScript parser は無かった。TS は JS の中に住んでいる」 | 「TypeScript parser を眺めに来たのに、TypeScript parser というものは無かった。JS を読んでいる途中で、ちょっとだけ TS に寄り道する。TS は JS の中に住んでいる、というのが今日眺めて分かったことです」 |
