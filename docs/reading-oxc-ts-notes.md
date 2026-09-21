# oxc TS パーサー読書メモ

[reading-oxc-ts.md](reading-oxc-ts.md) のセッションを進めながら気づきを記録する。
読んだ時点の rev: `1aa5ec11ce` (2026-09 時点の HEAD。ガイドの `d814c729f0` から数行ズレあり。
例: `checkpoint()` は cursor.rs:305→309)。

## Session 0: checkpoint / rewind / re-lex (2026-09-13)

### checkpoint が保存するもの / しないもの

`ParserCheckpoint` (cursor.rs:309) = レキサーの checkpoint + 現在トークン + `prev_token_end`
+ エラー数 + `fatal_error`。`LexerCheckpoint` (lexer/mod.rs:51) = ソース位置 + エラースナップショット
+ 収集済みトークン数 + コメント関連 2 つ。

**ガイドの問いの答え**: checkpoint がコピーするのは「位置とカウンタ」だけ。
**arena に確保済みの AST ノードは巻き戻さない**。投機パースが失敗して rewind しても、
その間に作った AST ノードは arena に残ったまま放置される (arena ごと drop されるまで生きる)。
bump allocator だから個別 free は不可能で、そもそもする必要がない — 誰も参照しなくなるだけ。
「rewind = 状態の復元」ではなく「rewind = カーソルとカウンタの復元 + ゴミは見なかったことにする」。

### エラーの巻き戻しは 2 段構え

- 通常の `checkpoint()`: エラーは **件数だけ** 保存 (`ErrorSnapshot::Count`)。
  rewind 時は `truncate`。投機パース中はエラーが「増えるだけ」という前提
- `checkpoint_with_error_recovery()` (cursor.rs:319): エラー vec を **丸ごと clone**
  (`ErrorSnapshot::Full`)。投機パース中に既存エラーが変化しうる場合用。コメントに
  「more expensive」と明記 — 使い分け自体が最適化

### token フィールドの不変条件が checkpoint を軽くしている

lexer/mod.rs:92 のコメントが良い: レキサーの `token` (構築中トークン) は
**レキサー呼び出しの合間は常に `Token::default()`**。だから checkpoint に保存不要。
checkpoint/rewind 両方が `debug_assert_eq!(self.token, Token::default())` で検証している。
不変条件をコメント + debug_assert で守って保存コストを削る、という設計。

### lookahead は checkpoint/rewind の薄い皮

`lookahead()` (cursor.rs:340) = checkpoint → クロージャ実行 → 無条件 rewind。
`peek_token()` (lexer/mod.rs:255) もレキサー版の同じパターン。
「先読み」という別機構はなく、全部投機実行で統一されている。

### 白眉: re_lex (lexer/typescript.rs, 53 行)

`foo<<T>()>` のような入力で、レキサーが先に `<<` (ShiftLeft) を作ってしまう問題。
パーサーが「ここは型引数が始まりうる」と判断したら `re_lex_as_typescript_l_angle` で
ソース位置を 1 文字戻して `<` として再字句解析する。**レキサーとパーサーは独立ではない**。

L (`<`) と R (`>`) で後始末が非対称なのが面白い:

- **L 側** (`<<`→`<`): 複合トークンは checkpoint **より前に** トークン列に push 済み
  (checkpoint 時点の「現在トークン」だから)。なので `rewrite_last_collected_token` で
  トークン列の最後を `<` に上書きする。rewind 時は truncate がこの `<` を残すので、
  呼び出し側 (expression.rs) が元の複合トークンを書き戻す
- **R 側** (`>>`→`>`): `>` はレキサーが **遅延処理** していて、複合 `>>` は投機パース中
  (= checkpoint 後) に Replace モードで作られる。だから rewind の
  `truncate(checkpoint.tokens_len)` が勝手に消してくれる。後始末コード不要

`<=` / `<<=` も再字句解析経路に来るが「必ず失敗する」とコメントに明記
(`parse_type_arguments_in_expression` が先に弾く)。Session 3 で確認する。

### re-lex の協調タイムライン (`foo<<T>()>`)

```
① `<<` が ShiftLeft として tokens に push 済み。parser.token = `<<`
② expression.rs:920 → parse_type_arguments_in_expression
③ checkpoint 取得 ★境界線: tokens_len は `<<` 込み、cur_token = `<<`
④ re_lex_as_typescript_l_angle: source.back(1) + 単独 `<` を作り、
   tokens 末尾の `<<` を `<` に上書き (境界線より手前への変更!)
⑤ 投機続行: 2個目の `<` 以降は checkpoint 後の push
⑥成功: tokens = [..., `<`, `<`, T, ...] で無傷
⑥失敗: truncate が⑤を消す / ④の上書きだけ残る
   → expression.rs:933 が rewind で復元された parser.token (= 元の `<<`)
     を rewrite_last_collected_token で書き戻す
```

- `finish_re_lex` は「tokens に触らない」と明記 (lexer/mod.rs:340)。上書きが要るのは
  L 側だけなので別関数に分離 → R 側の後始末不要はこの分離のおかげ
- 失敗時に書き戻す `<<` は checkpoint の `cur_token` 経由で戻ってきたもの。
  checkpoint の保存フィールドが後始末の材料を兼ねる
- 協調ルールの本質: **checkpoint 境界線の手前をいじった者だけが失敗時の後始末責務を負う**。
  それを typescript.rs / cursor.rs / expression.rs のコメントが相互参照で文書化している

### 一般化: レキサーとパーサーは JS の時点で双方向

re-lex を「TS の特殊事情」と捉えると狭い。整理すると:

- レキサーは基本 **最長一致 (maximal munch)** で貪欲に切る (`<<` → ShiftLeft)。
  TS モードでもトークン列の 99.9% は JS と同じ。re-lex は曖昧地点だけのピンポイント修正
- **例外: `>` だけは最初から最長一致を放棄**して常に1個ずつ切る (遅延)。
  シフト演算子 `>>` が必要な文脈で逆に **結合** する (`re_lex_right_angle`, Replace モード)。
  理由は頻度の賭け: ネストジェネリクスの閉じ `>>` は日常、`foo<<T>()>` は珍事。
  この遅延のおかげで R 側は push も Replace も checkpoint 後 → truncate が全部消す →
  後始末コードゼロ。L/R 非対称の根はここ
- 正しい切り方が **文法依存** な例は JS の時点からある:
  - `/` — 除算か正規表現リテラルの開始か (`a / b` vs `a = /b/`)
  - テンプレートの `}` — ブロック終端か `${...}` の続きか
  - JSX のテキスト
  - → oxc レキサーには `next_token` と別にこれら用の特殊エントリポイントが生えている
- TS の re-lex が過激なのは「切り直すだけでなく **push 済みトークンまで書き換える**」点
- 教訓: **JS/TS は字句と構文を綺麗に分離できない言語**。oxc はその漏れを
  「パーサー→レキサーの逆方向 API (re-lex 系)」として設計に組み込んだ。
  教科書のパイプライン (レキサー→パーサー一方通行) と実物の対比は LT ネタになる

### 曖昧性への対処は3階層 (goal symbol / 投機 / カバー文法)

- 「レキサー単体で JS は切れない」は **ECMA-262 公式**: 字句文法に goal symbol が
  複数ある (`InputElementDiv` / `InputElementRegExp` など)。どれを使うかは構文文脈が決める。
  oxc の re-lex 系 API はこの実装形
- 同じ病気 (「`(` や `<` の時点で規則を決められない」) への薬が階層別に3つ:
  1. トークン階層: re-lex / goal symbol 切り替え
  2. 構文階層・投機パース: checkpoint → 試す → 失敗なら rewind。一度で決まるが二度読み
  3. 構文階層・カバー文法: 緩い文法で読み進めて後から再解釈
     (`CoverParenthesizedExpressionAndArrowParameterList`: `(a, b)` を括弧式で読み、
     `=>` が見えたらアロー引数に読み替え)。巻き戻し不要だが再解釈・検証コードが要る
- **仕様はアローにカバー文法を使うが、oxc は投機パースを選んだ** (js/arrow.rs)。
  arena で投機の失敗が安いから巻き戻すほうが素直、という判断。
  ただし分割代入の式→パターン変換などカバー文法的な処理も残っており、混成

### Session 3 先取り: `a < b > c` が失敗する正確な理由 (demo2 の深掘り)

意外な事実: **`c` は投機パースの中では一切パースされない**。`<b>` の型引数パース自体は
成功する (`b` という名前の型かもしれないので)。失敗は閉じ `>` の直後のトークンを
チラ見する **follow 判定** で起きる。コードの3段連鎖:

1. **rewind の引き金** types.rs:944 —
   `!self.can_follow_type_arguments_in_expr()` なら `rewind(checkpoint)`
2. **follow 判定表** types.rs:955 `can_follow_type_arguments_in_expr` (tsc の
   `canFollowTypeArgumentsInExpression` の移植):
   - `(` / テンプレート → **型引数で確定** (`f<T>(x)` / tagged template)
   - `<` `>` `+` `-` → 拒否
   - fallback: 改行 ‖ 二項演算子 ‖ **式を開始できないトークン** (`;` `)` `,` 等) → 採用
     (だから `f<T>;` は TSInstantiationExpression になる)
   - 裏返すと「次に式が始まってしまうときだけ拒否」— `a<b> c` は式の無演算子隣接に
     なってしまうから。比較 `(a<b)>c` なら合法なのでそちらに倒す
3. **「式を開始できるか」** types.rs:1657 `is_start_of_expression` — Ident は最後の腕
   types.rs:1666 `kind.is_ts_identifier(...)` で true → 拒否確定

`parse_type_arguments_in_expression` (types.rs:914) は Session 0 の総集編:
- 917-923 コメント: `<=` / `<<=` は「`=` から始まる型はない」ので checkpoint を取る前に弾く
  (無駄な checkpoint/rewind 往復の回避)
- 929 で L 側 re-lex、942 で R 側 re-lex
- 937: `a < b> = c` は合法だが `a < b >= c` は BinaryExpression (`>=` を先にチェックして即 rewind)

### 「型は `=` から始まらない」の正体と、tsc / oxc の構成の違い

- 型変数名 (識別子に使える単語) の話ではなく、**型という構文要素の開始トークン集合** の話。
  `f<=T>(x)` を型引数と読むには `<=` を `<`+`=` に割る必要があるが、割ると型が `=` から
  始まることになる。型文法に `=` で始まる規則はない → 必ず失敗
- コード上の姿: `parse_non_array_type` (types.rs:426) の巨大 match に `Kind::Eq` の腕がない。
  **match の腕の一覧 = 型の開始トークン集合の定義そのもの** (TS に正式な仕様書はなく、
  文法の定義は事実上 tsc の実装。tsc 側の対応物は `isStartOfType`)
- 同じ事実の置き場所が tsc と oxc で逆:
  - **tsc**: スキャナの `reScanLessThanToken` は `<<` しか割れない。「`<=` を割るべきか」という
    問い自体が発生しない。知識が **機能の不在** として埋め込まれ、構造的に事故が起きない。
    ただし理由はどこにも書かれない
  - **oxc**: レキサーは `<<` `<=` `<<=` 全部割れる万能ヘルパー (cursor.rs:282-287)。
    判断は呼び出し側に分散 — 式文脈 (types.rs:924) は性能のため事前ガード、
    型文脈 (types.rs:864, 890) は「割って失敗しても rewind で無傷」という論証で通す
  - oxc の選択理由: ヘルパーを文脈別に2つ持つより「機構は汎用に、ポリシーは呼び出し側に」。
    代償は安全性が **構造から文書化された論証に降格** すること。typescript.rs の doc コメントが
    53行中40行なのはこの必然 (論証を書き残さないと将来壊される)

### 感想・LT ネタ候補

- 投機パースの本質は「失敗のコストを arena が吸収する」こと。GC 言語や malloc/free だと
  この設計は重くなる。arena allocator と投機パースの相性の良さが oxc の速さの一因

#### LT 有力候補: 「コードよりコメントが長いファイル」の話

typescript.rs は 53 行中コード約 10 行、コメント約 40 行。これは趣味ではなく設計の必然、
というのが Session 0 を通しで読むと分かる構造:

- **なぜ長いか**: oxc は「機構は汎用に、ポリシーは呼び出し側に」を選んだ結果、
  正しさが「構造的に不可能」ではなく「こういう理屈で必ず安全に失敗する」という
  **論証** に支えられている。論証はコードに表現できないので、コメントに書き残すしかない。
  書き残さないと将来のリファクタで壊される (tsc は逆に「機能の不在」で表現したので
  説明不要、ただし理由はどこにも読めない)
- **コメントがファイルを越えて相互参照する**: typescript.rs ↔ cursor.rs ↔ expression.rs が
  「checkpoint 境界の手前をいじった者が後始末する」という契約を、それぞれの持ち場から
  説明し合っている。単一ファイルでは成立しない不変条件の文書化
- **コメント + debug_assert のセット運用**: lexer/mod.rs:83-92 の `token` 不変条件 —
  散文で理由を説明し、checkpoint/rewind 両方の debug_assert で機械的に検証し、
  その分 checkpoint の保存フィールドを1個削る。**コメントが性能最適化の根拠**になっている
- **「なぜこのチェックで十分か」の論証コメント**: rewrite_last_collected_token
  (lexer/mod.rs:362-370) は4行かけて分岐1個の省略を正当化。
  types.rs:917-923 は「`=` から始まる型はない」という文法的事実から
  checkpoint 往復の省略を導出
- LT の筋書き案: 「良いコードにコメントは不要」という俗説への反例として。
  高速パーサーは横断的不変条件だらけで、それは **コードでは表現できない制約** —
  コメントは飾りではなく、安全性を担う一級の成果物。5分なら typescript.rs を
  画面に映すだけで「53行中40行」の絵面が語ってくれる

### checkpoint の呼び出し箇所一覧 = 曖昧ポイント地図

grep した結果、呼び出しは十数か所で2パターン:

- **lookahead 経由 (必ず巻き戻す)**: arrow.rs:67 (`(` はアロー引数?) /
  declaration.rs:47 (`using`) / ts/statement.rs:403 (index signature) /
  ts/statement.rs:894 (`declare`) / ts/types.rs:375,483,600,1042 (mapped/named tuple ほか)
- **手動 checkpoint (失敗時のみ巻き戻す=投機パース)**: ts/types.rs:927
  `parse_type_arguments_in_expression` (**`f<T>(x)` の本丸**、typescript.rs コメントの
  `try_parse` はこれ) / js/arrow.rs:365 (唯一 `checkpoint_with_error_recovery` を使う) /
  ts/types.rs:347 (infer constraint) / ts/statement.rs:288 (interface extends) /
  js/statement.rs:65 (directive prologue 用の取り置き) / error_handler.rs:185 (conflict marker)

このリストはガイドの Session 1〜5 の読みどころとほぼ一致。checkpoint が数フィールドの
コピーで済む軽さだからこそ、この頻度で呼べる。

### re-lex の使用箇所マップ (回収済み)

2段ラッパー経由で、**`<` `>` の再読の**需要は全部「型引数リストの開閉」
(訂正 2026-09-21: re-lex は他にもある。テンプレートの `}` の再読は下の「テンプレート」を参照):

- 第1段 cursor.rs:276/293 `re_lex_ts_l_angle` / `re_lex_ts_r_angle` —
  トークン種別で何文字戻すか振り分けるだけ (`<<`→2, `<<=`→3 / `>>`→2, `>>>`→3)
- 第2段 ts/types.rs の3関数: `try_parse_type_arguments` (864, 型文脈) /
  `parse_type_arguments_of_type_reference` (890, 改行チェック付き) /
  `parse_type_arguments_in_expression` (929 で開き `<`、942 で閉じ `>`) ← 本丸
- 失敗時の `<<` 書き戻しは expression.rs:929 / 1130 の2箇所
- 「`<=` は必ず失敗する」の答え: types.rs:920 のコメント —
  **型は `=` から始まらない** ので `f<=T>` の `=T>` は型引数になりえず、事前に弾く

Session 3 ではこの3関数の中身 (成功と判定する条件) を読む。

- **テンプレートの `}` の再読 (2.3 で追加)**: `cursor.rs:242` `re_lex_template_substitution_tail` →
  レキサー側 `lexer/template.rs:398` `next_template_substitution_tail`。呼び出しは4か所:
  型 `ts/types.rs:773, 789` (`parse_template_type`) と、式 `js/expression.rs:571, 583`
  (`parse_template_literal`)。型と式で同じ作りを共有している。`<` `>` と違って型引数とは無関係

## Session 1: 型式コア (2026-09-13 開始、15-44行まで)

### parse_ts_type (types.rs:15-44) は「最下位優先度の階層関数」

再帰下降の各階層は「強い階層を呼ぶ + 自分の担当演算子を処理」が定型。
parse_ts_type は最下位版で、30行が3ブロックに分解できる:

| 行 | 正体 |
|---|---|
| 16-18 | 最下位階層の別産品 (関数型/コンストラクタ型) への分岐。`(` では判別できず lookahead (86行、Session 0 の checkpoint 実戦例) |
| 19-20 | 定型部: `parse_union_type_or_higher` を呼ぶ |
| 21-42 | 自分の担当演算子 = conditional (`extends ... ? ... :`) |

- 関数型が最下位にいる理由: `=>` の右側がすべてを飲む (`() => A | B` は union を返す関数)
- conditional が union より上の答え: `A | B extends C ? X : Y` = `(A|B) extends C ? X : Y`。
  **関数の呼び出し順がそのまま優先順位** (Pratt の優先順位表と対照的)
- 行番号補正 (現 HEAD): union 245 / intersection 241 / type_operator 282 /
  postfix 358 / non_array 411。intersection がファイル上 union より先に定義されている罠

### 宿題2問の答え (実証: demos/oxc-step1/)

**Q1: extends 節のネスト禁止 (`DisallowConditionalTypes`) はなぜ** —
`?` と `extends` の対応付けを決定的にするため。`T extends U extends V ? X : Y` は
`? X : Y` の付属先が曖昧 (**dangling else の親戚** = 付属先曖昧性)。
dangling else は「近い方に付ける」慣習で解決したが、TS は **文法を狭めて禁止** を選んだ。
違い: dangling else はどちらの読みも完結するが、こちらは内側に付けると外側の
`? :` が不完全になり、遅くて分かりにくい破綻になる。入口で即エラーの方が診断が明快。
括弧なら OK (demo2)、false 側のチェーンは常時 OK (demo3、三項演算子と同じ右結合)。
曖昧性対処の道具箱: 投機する (Session 0) / 文法を狭める (これ) / 慣習で裁く (dangling else)

**Q2: `extends` 前の改行チェック (types.rs:22) はなぜ** —
予約語はメンバー名に使えるので、`a: string` ␤ `extends: number` の `extends` が
「conditional 開始」か「次のメンバー名」か曖昧になる。改行が来たらメンバー名側に倒す
ASI 的裁定 (demo5)。代償: conditional の `extends` を行頭に折り返せない (demo4 はエラー。
prettier が `T extends` を同じ行に保つのは文法上の制約だった)

### 1.2 関数型/コンストラクタ型 (types.rs:46-143)

#### 判定は「コストの安い順に3段」— 全部 rewind するわけではない

`is_start_of_function_type_or_constructor_type` (86) の match は判定コストで並んでいる:

| トークン | 判定方法 | コスト |
|---|---|---|
| `<` / `new` (88) | 現在トークンだけで即 true | checkpoint すら取らない |
| `abstract` (89) | `peek_token()` で次が `new` か見るだけ | パーサー状態は動かさない |
| `(` (90-120) | **checkpoint → 投機 → 必ず rewind** | 唯一の投機パース |
| それ以外 (121) | 即 false | — |

`<` が即 true でいい理由: **`<` で始まる型は関数型しかない**。`parse_non_array_type`
(411-506) の match に `Kind::LAngle` の腕がない (`Foo<T>` は先頭が識別子)。
「`=` から始まる型はない」と同じ読み方。文法が曖昧でない場所では投機しない、が徹底されている。

#### `(` の投機: 括弧型 vs 関数型を分ける「最小の証拠」4パターン

`(A | B)` (括弧型) と `(x: T) => U` (関数型) の判別。全部読まずに証拠1個で即決:

1. `()` / `(...` (95) → 引数リスト確定 (空の型は書けない、rest は引数だけ)
2. `(x:` `(x,` `(x?` `(x=` (104-107) → これらの記号は括弧型の中に現れえない
   → `x` は型ではなく **引数名** だったと確定
3. `(x) =>` (112) → 2 で決まらないとき。`)` の次の `=>` まで見る
4. どれでもない → 括弧型 (`(A | B)` は `A` の後が `|` なので 2 に該当せず、`)` の後も `=>` でない)

`can_follow_type_arguments_in_expr` (Session 0) と同じ思想 = **判定に必要な最小限だけ読んで即 rewind**。

#### `skip_parameter_start` (125-143) の見どころ

- modifier を先に飛ばす (127) — `(private x: number)` パラメータプロパティ用 (Session 6)
- `this` を許す (131) — `(this: Foo) => void`
- 分割代入 `({a, b}: T) => U` は **実際にパースを試して判定** (135-141)。
  成功条件が「fatal error なし **かつ エラー件数が増えていない**」。
  Session 0 で読んだ「checkpoint はエラー件数を保存して truncate で戻す」仕組みが、
  ここでは **判定条件そのもの** として使われている

#### 本体 `parse_function_or_constructor_type` (46-84) は読む順=文法規則

`abstract` → `new` → `<T>` → `(params)` → `=> ReturnType` の順に食べるだけ。

- 48-49: `abstract new () => T` という並びしか許されないことがコードの順序に出ている
- **51: `context_remove(DisallowConditionalTypes)`** — 引数リストの中は conditional 解禁
  (`(x: T extends U ? A : B) => void` が書ける)。1.1 で見たフラグのもう1つの解除ポイント
- 61-65: `new (this: number) => any` は違法 → パースは共通関数でやり、
  **違法な組み合わせは後から検査** する作り

### 1.3 union / intersection 階層 (types.rs:241-280)

#### 「次に呼ぶ関数」を引数で渡す = 階層のつなぎ方を外から差し替える

union と intersection は構造が完全に同じで、違うのは2点だけ (区切り記号 / 1つ下の階層)。
なので本体は `parse_union_type_or_intersection_type` (252) 1つで、2点を引数で受け取る:

```rust
fn parse_intersection_type_or_higher(&mut self) {       // 中身なし、引数を変えて呼ぶだけ
    self.parse_union_type_or_intersection_type(Kind::Amp, Self::parse_type_operator_or_higher)
}
fn parse_union_type_or_higher(&mut self) {
    self.parse_union_type_or_intersection_type(Kind::Pipe, Self::parse_intersection_type_or_higher)
}
```

1.1 の「**次にどの関数を呼ぶか = 優先順位そのもの**」を引数化した形。
`Self::parse_type_operator_or_higher` はメソッドを関数値として渡す書き方で、
Session 0 の `lookahead(Self::is_unambiguously_index_signature)` と同じ。
境界は `F: Fn(&mut Self) -> TSType<'a>` (258)。ジェネリクスなので単相化され、
関数ポインタ経由にならず抽象化コストはゼロ。

本体 (260-279) の仕掛け2つ:
- 261 `has_leading_operator` — `type A = | X | Y` の先頭区切り記法。これがあると
  後続に `|` が無くても union ノードで包む (264 の `|| has_leading_operator`)
- 264 の条件と 279 — 区切りが1個も無ければ union ノードを作らず **中身をそのまま返す**。
  階層を何段降りても無駄なノードが積み上がらない

#### コメントアウトされた tsc 原文 = 「未移植」の目印

262 / 268 の `/* hasLeadingOperator && parseFunctionOrConstructorTypeToError(isUnionType) ||*/`
は **tsc のソースをそのまま貼った未移植マーカー**。キャメルケースの変数名と `||` の結合が
原文のまま残っている (tsc: `let type = hasLeadingOperator && parseFunctionOrConstructorTypeToError(...) || parseConstituentType();`)。
逐語訳移植が、移植しなかった箇所すら原文の位置に残すレベルで徹底されている。

tsc 側のその関数はエラーメッセージ品質専用のヘルパー。union/intersection の構成要素に
括弧なしの関数型を書いた場合を捕まえ、`Function type notation must be parenthesized
when used in a union type` と名指しで報告する (パースは成功させたうえで診断を出す)。
括弧が必要な理由は 1.1 の通り **`=>` が右を全部飲む** (関数型は `|` より弱い) から。

実測した診断の差 (どちらも受理はしない。失われるのはメッセージの質だけ):

| 入力 | oxc | tsc |
|---|---|---|
| `type A = string \| () => void;` | `Unexpected token` (`)` を指す) | 関数型は括弧で囲め、と名指し |
| `type C = string & new () => void;` | `Expected a semicolon...` (`(` を指す) | コンストラクタ型は括弧で囲め、と名指し |

oxc では `(` が括弧型の開始として読まれ中身が空でコケる / `new` が型名として読まれた挙句
ASI エラー、と原因から遠い診断になる。**「tsc は IDE のために回復へ投資、oxc は正しいコードを
速く」の具体的な証拠品**。同種のコメントは他にもある (`parse_non_array_type` の JSDoc 型
431-444 など) ので、見かけたら「tsc にはあるが未移植」の目印として読む。
実証: demos/oxc-step1 の demo6-8 (エラー時は Program の body が空になる点も確認)

#### なぜ移植しなかったのか (推測)

コメントが入ったのは 2024-06-26 の #3903 "improve parsing of TypeScript types"
(types.rs を tsc の parser.ts に寄せて書き直した大リファクタ)。
コミット本文は "- [x] fix everything" の一行で、理由は書かれていない。以下は推測だが根拠のある線:

1. **互換性の指標が動かない** — oxc の検証は test262 / babel / TS conformance で、
   見るのは「パースが通るか否か」。メッセージ文言は比較対象外。今回は受理/拒否の判定が
   一致しているので、移植してもテスト通過率は1ミリも変わらない
2. **性能コストが正常系に乗る** (これが一番効いてそう) — tsc の元コードは union/intersection の
   **構成要素ごとに** `isStartOfFunctionTypeOrConstructorType()` を呼ぶ。1.2 で読んだ通り
   この判定は `(` で始まるとき checkpoint→投機→rewind の往復をする。つまり
   `string | (() => void)` という **正しいコード** を書くたびに無駄な投機が1往復増える。
   エラー時のメッセージのために正常時のホットパスに払う形で、「正しいコードを最速で」と相性が悪い
3. **fatal error で AST を捨てている** — 丁寧なメッセージを出しても後続の解析は走らない。
   tsc が回復に投資するのは壊れたコードでも補完/hover を返す必要があるから。oxc にその顧客はいない

まとめ: **コストは正常系、利益は異常系のメッセージだけ、しかもテストで測られない** の三重苦。
原文をコメントで残したのは「知らずに漏らしたのではなく意図的に省いた」という意思表示と読める。
なお oxc も必要性の高い診断は独自実装している (1.1 の `expect_conditional_alternative` など)
ので、診断軽視ではなく **費用対効果で個別判断** しているのが実態に近い。

### 1.3 後半: 前置型演算子と infer (types.rs:282-356)

#### `parse_type_operator_or_higher` (282) は前置演算子の階層

union/intersection が中置だったのに対しこちらは左端に演算子が来る形なので、
**先頭トークンを見るだけで分岐でき投機不要**: `keyof` / `unique` / `readonly` / `infer`。

`_` の腕 (288-291) で1つ下へ降りるとき **`DisallowConditionalTypes` を外している**。
この先には括弧 `(...)` や `{...}` という閉じた文脈が来るから。
`T extends (U extends V ? A : B) ? X : Y` の括弧内が合法なのはこの解除のおかげで、
逆にフラグが効く範囲は「括弧に入るまでの裸の型式」だけ。

`parse_type_operator` (295) は演算子を1つ食べて **自分自身を再帰呼び出し** (299)。
`keyof keyof T` が自然に扱える (demo9)。前置演算子階層の定石。

300-305: `readonly` は配列型/タプル型にしか付けられない (`readonly string` は NG) を
**パース後に検査** して報告。1.2 の `new (this:...)` と同じ「読んでから中身を見る」作り。

#### `parse_constraint_of_infer_type` (335-356) — 曖昧なときだけ投機

`infer T extends U` の `extends` は「U の制約」か「外側 conditional の一部」か2通りに読める。
解決が2段構え:

1. **曖昧でない場合は投機しない** (343-346) — `infer` は普通 conditional の extends 節にいて、
   そこは既に `DisallowConditionalTypes` が立っている。後ろに `?` が来ても conditional として
   再解釈される余地がないので **checkpoint なしで** 制約を読む (demo12 がこの経路)
2. **曖昧な場合だけ投機** (347-355) — checkpoint を取って制約を読み、直後が `?` なら
   「この extends は外側のものだった」と判断して rewind し制約なしで返す

doc コメント (326-334) が2ケースを明示。Session 0 の「曖昧でない場所では投機しない」の
いちばん凝った実例。

**4分岐を demo で全網羅** (`demos/oxc-step1/README.md` 追記分): `extends` 自体がない即 `None`
(demo13) / フラグ立ってて checkpoint なし (demo12) / 曖昧だが直後が `?` でないので採用 (demo14) /
曖昧で直後が `?` なので rewind (demo15)。demo14 と demo15 は入力の先頭 `infer U extends string`
が完全に同一で、読み終えた直後の1トークン (`;` か `?` か) だけで採用/破棄が分かれるのが対比の肝。

`None` を返したときに何が起きるか (`parse_constraint_of_infer_type` の呼び出し元は types.rs:319
`parse_type_parameter_of_infer_type`) を追った:

1. `extends` がそもそもない場合 → 何も読まず即 `None`。`TSTypeParameter.constraint` が `None` になり
   `infer T` (制約なし) として確定。これが実は **infer の元々の・今も主流の書き方**
   (`type ElementType<T> = T extends (infer U)[] ? U : T;` のように制約なしで使うのが基本形)。
   `infer T extends U` の制約付き構文は後発 (下記) なので、「まず `extends` の有無だけ見る」という
   コードの形が歴史的な順序をそのまま反映している
2. 曖昧で rewind した場合 → `constraint` は `None` になるが、**カーソル位置は `extends` の直前まで
   巻き戻っている**。`parse_infer_type` は制約なしの `TSInferType` を返し、それが union/intersection/
   postfix の階層を素通りして `parse_ts_type` まで戻る。`parse_ts_type` は checkType を読み終えた後に
   「次が `extends` か」を見るので、巻き戻された `extends` をそこで検出し、**今度は外側 conditional の
   extends 節として** 読み直す。つまり `type X<T> = infer U extends V ? A : B` は
   `(infer U) extends V ? A : B` という1個の conditional (checkType = `infer U`) に確定する。
   `None` は「読み損ねた」ではなく「この `extends` は自分の担当ではない」という積極的な合図

**`infer T extends U` (制約付き) の経緯**: `infer` 自体は TS 2.8 (2018) からある classic な機能で
最初から制約なし。`infer T extends U` は **TS 4.7 (2022)** で追加された後発の拡張で、推論結果に
上限を設けたい用途 (`T extends [first: infer F extends string, ...unknown[]] ? F : never` など)。
つまり oxc のこの関数が「曖昧なとき」を気にするのは、後から増築された構文が既存の
`extends ... ? ... :` と字面上ぶつかったから、という TS 言語進化の副産物。

#### パーサーとチェッカーの境界線 = ローカルな情報で判定できるか

`type C<T> = keyof infer U;` (demo11) は **oxc がエラーなしで通す**。`infer` は conditional の
extends 節でしか書けないので TypeScript としては不正だが、これはバグではなく役割分担:

- 判定には **祖先ノードを遡って「自分は conditional の extends 節の中か」を調べる** 必要がある
  → 木を作り終えてからの仕事 → tsc でも **チェッカー** が報告している
- 対して 1.2 の `new (this: number) => any` は同じ関数内で `this_param` の有無を見れば済む
  → **パーサーが報告できる**

この軸は後の Session でも効きそう: **ローカルに閉じる検査はパーサー、木全体を見る検査はチェッカー**。

#### oxc 側の「チェッカー」の実体 (今回の読書範囲外だが、引っ越し先の住所として)

demo11 の TS1338 を実際に出しているのは `oxc_semantic`。実装は文字通り祖先を遡る形
([checker/typescript.rs:74-84](../../../oxc-project/oxc/crates/oxc_semantic/src/checker/typescript.rs#L74-L84)):

```rust
let is_in_conditional_extends_clause = ctx.ancestry().ancestor_kinds().any(|kind| {
    kind.as_ts_conditional_type().is_some_and(|conditional| {
        conditional.extends_type.span().contains_inclusive(infer_type.span)
    })
});
```

祖先に conditional がいるだけでは不十分 (true/false 節かもしれない) ので、
**extends 節の span に含まれるか** まで確認しているのが丁寧。診断も
`ts_error("1338", ...)` と **TS のエラーコードごと移植** (diagnostics.rs:382)。
1.3 で見た「移植しなかった診断」と対照的で、AST を一度歩く工程のついでに検査できるので安い
= 費用対効果が合うから実装した、と読める。

紛らわしい3つの別物:

| レイヤー | 実装 | 型情報 | TS1338 |
|---|---|---|---|
| `oxc_semantic` | 純 Rust。スコープ/シンボル + **構文的検査** | 使わない | **担当** |
| `oxc_type_checker` | 純 Rust。doc に "does *not* type check anything yet" と明記された **足場のみ** | まだ無し | 無関係 |
| oxlint の type-aware ルール | **tsgolint** = 外部実行ファイルを起動 (`executable_path: PathBuf`) | 本物の型 | 無関係 |

tsgolint は typescript-go (tsgo = tsc の Go 移植) の checker を抱えていて、
`no-floating-promises` のような型が要るルールを担当する。つまり oxc は
**型が要る仕事だけ本家に外注** している。パーサーは全ファイルが必ず通るので 1:1 移植する
価値があるが、型チェッカーは実装コストが桁違いで tsgo 自体も十分速い、という判断だろう。

更新版の境界線:
- ローカルに判定できる → **パーサー** (`readonly string`, `new (this:...)`)
- 木全体を見れば分かる → **oxc_semantic** (`infer` の位置, TS1338)
- 型を知る必要がある → **tsgolint 経由で tsgo** (oxc 自身は書いていない)

#### TS のエラーコードと、oxc での実装状況 (実データで確認)

`ts_error(code, message)` ヘルパーが **oxc_parser と oxc_semantic の両方** にあり
(`OxcDiagnostic::error(msg).with_error_code("TS", code)`)、tsc と同じ番号を出す。
規模はパーサー側が約107個、semantic 側が15個。

番号帯の **目安**: 1xxx = 構文・文法 / 2xxx = 意味・型。
`infer` の位置が **TS1338** (1xxx) なのは、tsc 自身が文法エラーに分類している証拠で、
報告場所が checker なのは AST を歩く工程がそこにあるだけ。型は使っていないので
**型を持たない oxc にも移植できた**。1.1 の Playground スクショも同じ対比:
`'?' expected. (1005)` = 文法 → oxc もパーサーで同判定 /
`Cannot find name 'extends'. (2304)` = 型・名前解決 → oxc は触らない。

**ただし「1xxx=パーサー、2xxx=チェッカー」は法則ではない** (パーサー側の実在コードを集計):

| 帯 | 件数感 | 中身の例 |
|---|---|---|
| 1xxx | 60件超 | 純粋な構文エラー |
| 2xxx | 9件 | `2681` コンストラクタに `this` パラメータ不可 / `2730` アロー関数に `this` 不可 / `2452` enum メンバーに数値名不可 / `2206` import type の重複指定 |
| 5xxx | 3件 | `5085` タプル要素が optional と rest を兼ねられない |
| 8xxx | 6件 | `8002` `import ... =` は TS ファイルのみ / `8011` 型引数は TS ファイルのみ (= **.js に TS 構文を書いた** 系) |
| 17000 / 18xxx | 数件 | JSX / `18010` private 識別子にアクセス修飾子不可 |

semantic 側には逆に `1234` (ambient module 宣言はトップレベルのみ) という 1xxx がいる。

パーサーが持つ 2xxx は **全部ローカルに判定できるもの**。`2681` は 1.2 で読んだ
`new (this: number) => any` のチェックそのもので、同じ関数内だけ見れば分かる。
TS が 2xxx を振ったのは「tsc がチェッカーのフェーズで報告している」という実装都合であって、
判定に型が要るからではない。

**正しい言い方**:
- 本質は **判定に必要な情報の範囲** (ローカル / 木全体 / 型)
- TS のコード番号は tsc がどのフェーズで報告するかの反映。相関はあるが一致しない
- oxc は「型が要らないものは全部自前で出す」方針なので、パーサーの番号帯がばらける

### Identifier と IdentifierName — 予約語なのに `extends: number` が書ける理由

JS の文法には「名前」が2種類ある:

- **Identifier** (変数などの束縛名): 予約語禁止。`let extends = 1` は SyntaxError
- **IdentifierName** (プロパティ名・メンバー名・`.` の後): **予約語も OK** (ES5 から)。
  `obj.extends` / `({ extends: 1 })` / `class C { extends() {} }` は全部合法

直感: 変数名は `extends + 1` のように裸で式に現れるから予約語と衝突するが、
プロパティ名は常に `.` や `{` の後ろという文脈に守られていて衝突しようがない。

TS 周りの「キーワード」は3階級:

| 階級 | 例 | 変数名 | メンバー名 |
|---|---|---|---|
| 完全予約語 | `extends` `class` `if` | ❌ | ✅ |
| 文脈依存キーワード (JS) | `async` `of` `get` | ✅ | ✅ |
| 文脈依存キーワード (TS) | `declare` `readonly` `type` `namespace` | ✅ | ✅ |

3段目が Session 4 の主役 (`let declare = 1` すら合法なので毎回 lookahead で判定)。
demo5 の `extends` は1段目の「予約語だがメンバー名 OK」で JS 由来の普通の挙動。

歴史の連鎖: ES5 が IdentifierName を緩めた → TS がメンバー名に予約語を許す →
`a: string` ␤ `extends: number` の曖昧性が生まれる → conditional type (TS 2.8) が
改行ガード (types.rs:22) でツケを払う。もしメンバー名も予約語禁止だったら
demo4/demo5 の問題ごと存在しなかった

### 文法の決断は全部 tsc 側 — oxc はほぼ逐語訳

parse_ts_type は tsc の `parseTypeWorker` の Rust 移植。対応表:
`isStartOfFunctionTypeOrConstructorType` ↔ `is_start_of_function_type_or_constructor_type` /
`noConditionalTypes` 引数 ↔ `Context::DisallowConditionalTypes` フラグ /
`scanner.hasPrecedingLineBreak()` ↔ `cur_token().is_on_new_line()`。
conditional type の文法ガード2つは TS 2.8 (2018, conditional type 導入 PR) 時点の
TS チームの決断。oxc は互換パーサーなので文法を決める権利がなく、
oxc 側の決断は「どう速く実装するか」だけ、という役割分担で読む。

### LT 材料: 同じ入力、同じ判定、違うエラー回復 (tsc vs oxc)

demo1 (`T extends U extends V ? X : Y`) を両方に食わせた結果:

- **パース判定は完全一致**: tsc `'?' expected. (1005)` / oxc `` Expected `?` but found `extends` ``。
  逐語訳移植の証拠
- **エラー後が別世界**:
  - tsc: 回復して AST を作り続け、checker まで到達 (`Cannot find name 'extends'. (2304)`
    は意味解析のエラー! hover も `type extends = /*unresolved*/ any` と応答)。
    壊れたコードでも補完を出す **IDE ファースト** の設計
  - oxc: fatal error で Program の body が空。主用途 (lint/bundle/format) は
    正しいコードを速く処理することなので、深い回復に投資しない
- LT の筋: 「パーサーの互換性とは何か」— 文法判定は 1:1 でも、
  エラー回復戦略は消費者 (IDE vs ビルドツール) が決める、という対比が1枚で描ける

### LT 有力候補: 「コメントアウトされた他人のコード」= 移植とは何を捨てるかの選択

1.3 の `parseFunctionOrConstructorTypeToError` の話 (詳細は 1.3 のセクション参照)。
上の「違うエラー回復」をもう一段具体にした版で、**捨てた瞬間が1行のコメントとして
コードに残っている** のが強い。

- **掴み**: 他人 (tsc) のソースが JS のままコメントで残っているスクショ1枚。
  「これ何だと思います?」で始められる
- **中身**: 移植とはコピーではない。oxc は文法の決断を 1:1 でコピーする一方 (権利がない)、
  エラー回復は自由に捨てている。境界線は **互換性テストで測られるか否か**
- **オチ**: 捨てた理由は3つとも「測られない・正常系にコストが乗る・後続処理がない」で、
  技術的判断として筋が通っている。コメントは「知らずに漏らした」ではなく
  「意図的に省いた」という意思表示
- **絵になる素材**: コメント行 / oxc の `Unexpected token` / tsc Playground の名指しエラー、
  の3枚並べ

### LT 候補: 「そのエラー、誰が出してるの?」— 検査の住み分け

`keyof infer U` が oxc のパーサーを素通りする話 (1.3 のセクション参照) を軸に、
**1つの言語の検査が3つの実装に分かれている** ことを見せる筋。

- **掴み**: `type C<T> = keyof infer U;` を oxc に食わせると通る。tsc では TS1338 が出る。
  「oxc のバグ?」と振っておいて、違うと明かす
- **中身**: 判定に必要な情報の範囲で担当が決まる —
  ローカル (パーサー) / 木全体 (oxc_semantic、祖先を遡る実際のコードが10行で読める) /
  型 (tsgolint 経由で Go 製の tsgo に外注)
- **小ネタ**: TS のエラーコードの番号帯 (1xxx 文法 / 2xxx 意味) が目安になる。
  ただし法則ではなく、パーサーにも 2xxx が9件いる (全部ローカル判定可能なもの) — の落差も面白い
- **オチ**: 「Rust で全部書き直す」のではなく **型が要る仕事だけ本家に外注** している。
  パーサーは全ファイルが必ず通るので 1:1 移植する価値があるが、型チェッカーは割に合わない

#### LT 候補の整理 (Session 2.2 時点で F-I を追加)

| 候補 | 掴み | 難度 | 備考 |
|---|---|---|---|
| A. `f<T>(x)` vs `a<b>c` の曖昧性 | 誰でも読めるコード | 中 (動きの説明が要る) | 説明型。5分だと駆け足 |
| B. コードよりコメントが長いファイル | 53行中40行の絵面 | 低 | 主張型。息切れしにくい |
| C. コメントアウトされた他人のコード | JS のままのコメント | 低 | **B の具体版。1行+診断2枚で完結** |
| D. tsc と oxc のエラー回復の違い | Playground との対比 | 中 | C に内包できる |
| E. そのエラー、誰が出してるの? | 「通っちゃった」の意外性 | 低〜中 | パーサー読書の外の話も入れられる。3層の絵が描きやすい |
| F. `null` の AST は誰に合わせるか | 同じ `null` 型が tsc / typescript-estree / oxc で形が違う | 低〜中 | **3者の関係が1つの型で見える**。tsc 4.0 の変更 → typescript-estree が剥がす → oxc が合わせる |
| G. oxc は何を捨てて速いのか | `[...S, ...S]` を oxc は通し、tsc は TS1265 | 低 | **demo11 が1行で差を見せる**。適合率 100% / 62.56% の数字も添えられる |
| H. キーワードが識別子になる | `namespace string {}` が書ける | 低 | JS 仕様 (IdentifierName) と TS のソフトキーワード。デモ未作成 |
| I. 移植とは何を捨てるか (tsc 6.0 → ts-go 7.x) | `JSDocFunctionType` を Microsoft 自身も Go 移植で落とした | 低 | C の補強。oxc の未移植が「独自の手抜き」ではない裏付け |

B と C は同じ「コメントから設計を読む」筋なので、どちらか1本に統合するのが自然。
C を軸に B の補強 (typescript.rs の 53行中40行) を入れるのが今のところ最有力。
E は毛色が違い (コードの細部でなくエコシステムの構造)、聴衆が TS ユーザー中心なら
こちらの方が刺さるかもしれない。

#### LT の軸候補 (Session 2.2 時点): 「パーサーの範囲? チェッカーの範囲? — tsc / typescript-estree / oxc」

E・F・G をまとめると、**「同じ TypeScript を読む3者は、どこまでを・どの形で扱うか」** という1本の軸になる。
ニッチさ対策として、最初に3者の関係を1枚で置く:

```
                 TypeScript のソース
                        │
     ┌──────────────────┼───────────────────────┐
     ▼                  ▼                       ▼
    tsc            typescript-estree            oxc
 パーサー+チェッカー   tsc の AST を ESTree 形式に変換   自前パーサー (Rust)
 (型の解決までやる)   (ESLint 向け)              AST は typescript-estree の形に合わせて出力
```

- **tsc**: 本家。パーサーの後ろにチェッカーがあり、型を解決して検査する
- **typescript-estree**: ESLint が TS を読むための変換層。tsc の AST を ESTree 形式に直す
  (`convert.js:2439` に `null` を剥がすコメントがある = F の根拠)
- **oxc**: 高速な再実装。型は解決しない。AST の形は typescript-estree に合わせる

この軸で話せること (実測済みのもの):

| 観点 | 具体例 | 材料 |
|---|---|---|
| 検査を出す場所が違う | tuple の rest 検査 (TS1265): tsc は型を解決するチェッカー、oxc は構文の見た目で近似 | demos/oxc-step2 demo11-14、メモ 2.2 の節 |
| 近似の代償 | `readonly` / `ReadonlyArray` / エイリアス / `ns.Array<X>` は見逃す。ただし AST には影響しない | demo11-14 |
| oxc の優先順位 | 正しいコードは 100% 通す (Positive)、間違いを弾くのは 62.56% (Negative) | parser_typescript.snap (メモの出典) |
| AST の形は誰に合わせるか | `null`: tsc 4.0 以降 `LiteralType > NullKeyword`、typescript-estree と oxc は `TSNullKeyword` | 実測 (メモの `null` の節) |
| 検査の住み分けの軸 | 「ローカルに閉じるか」ではなく「**判定に型の解決 / 木全体が要るか**」 | メモ 1.3・2.2 (訂正済み) |

**発表のゴール (2026-09-20 に決定)**: 聴いた人が、**自分と同じように oxc の TS パーサーを読むときの「当たり」がつく**
こと。つまり「oxc の TS はすごい」ではなく「どこから読み始めて、何に出会うか」の地図と読み方。

**発表の骨格案 (5分、2026-09-20 に再度組み直し)**: 内容は減らして 1 つのメッセージに絞る。
**ライブでは走らせない** (静止画で見せる)。軸は「**tsc と同じ所 / 違う所**」の 2 部構成。

持ち帰ってほしいこと: 「oxc の TS は `ts/types.rs` の `parse_ts_type` から読む。tsc と見比べると、
同じ所と違う所がある」

| # | 時間 | スライド |
|---|---|---|
| 1 | 0:00-0:30 | 題 + 問い (「oxc で TS をパースするコード、どこから読む?」) |
| 2 | 0:30-1:45 | **地図**: ファイル 4 つ (`ts/types.rs` / `js/expression.rs` / `lexer/typescript.rs` / `cursor.rs`) + 入口は `parse_ts_type` の 1 関数 |
| 3 | 1:45-2:30 | **読み方のコツ 2 つ**: tsc を横に置く / `--estree` で動かして読む (聴衆が後でやる話。当日は走らせない) |
| 4 | 2:30-3:30 | **同じ所**: `<` の曖昧性。3 つの入力で tsc と oxc の結果が一致 (下の表) |
| 5 | 3:30-4:40 | **違う所**: `[...S, ...S]` (tsc はエラー / oxc は通る)、`null` の AST (形が違う) |
| 6 | 4:40-5:00 | まとめ + ブログ (トリビア集) の告知 |

**4 (同じ所) の材料**: `<` は tsc 5.9 と oxc で、3 つの入力とも結果が一致 (2026-09-20 に両方で実測):

| 入力 | tsc 5.9 | oxc |
|---|---|---|
| `f<T>(x);` | CallExpression (型引数付き) | CallExpression (型引数付き) |
| `a < b > c;` | Binary(Binary(a, b), c) = `(a < b) > c` | 同じ |
| `a < b > (c);` | CallExpression | 同じ |

見せる静止画: demos/oxc-step0 の demo1 / demo2 の AST と、投機のトレース (`checkpoint` → `rewind`)。
「同じ結果を出すために、oxc も投機して読み直している」という話につなげる。
(手法が tsc と同じか (tsc の `reScanLessThanToken` に対応するか) は、Session 0 のメモを見直して確認してから話す)

**5 (違う所) の材料**:

| 例 | tsc 5.9 | oxc | 何が違うか |
|---|---|---|---|
| `type S = string[]; type A = [...S, ...S];` (demo11) | TS1265 エラー | エラーなし | 検査を出す場所 (チェッカー / パーサーの近似) |
| `type A = null;` | `LiteralType > NullKeyword` | `TSNullKeyword` 単体 | AST の形。oxc は typescript-estree に合わせている |

見せる静止画: demo11 の tsc の出力 (`tsc_check.js`) と oxc の出力を並べた 1 枚、`null` のツリー 2 つ。
数字 (正しいコード 100% / 間違い 62.56%) は口頭で 1 文だけ、または削る。

**削ったもの** (ブログのトリビアに回す): 3 者の関係図 (口頭 1 文に)、コツ ① の目印、降下ルートの全階層。

**読み方のコツ** (実際に読んで効いたもの。聴衆が同じ順で読める形に):
1. **コード中の目印で道具が分かる**: `is_start_of_*` = 1 トークン以上の先読み / `checkpoint` + `rewind` = 投機 /
   `re_lex_*` = レキサーへの巻き戻し依頼 (曖昧性への 3 つの道具)。目印を見つけたら「ここは曖昧なんだな」と思って読む
2. **tsc を横に置く**: 関数名がほぼ 1:1 (`parseNonArrayType` ↔ `parse_non_array_type`、`parseMappedType` ↔
   `parse_mapped_type`)。oxc の未移植箇所は tsc の原文がコメントアウトで残っている。「なぜこの文法?」の答えは tsc 側
3. **動かしながら読む**: `cargo run -p oxc_parser --example parser -- x.ts --estree` で AST を見る。
   迷子になったら `parse_ts_type` と `parse_non_array_type` に戻る (型パーサーの全経路はこの 2 つを通る)
4. **「エラーを出す場所」は tsc と違うことがある**: tsc ではチェッカーが出す検査を、oxc はパーサーで近似したり
   出さなかったりする。これを知らずに読むと「tsc と挙動が違う」で混乱する → demo11 で見せる

**LT とブログの分担 (2026-09-20)**: LT は上の「地図・入口・読み方のコツ・落とし穴 1 つ」だけ。
細かい内容はブログに書く予定で、**形は「oxc の TS パーサーを読んで見つけたおもしろトリビア集」**
(各項目が独立した小ネタで、どこからでも読める。このメモに書いてきた発見がそのままネタになる)。
LT では触れずにブログ側へ回す。トリビアのタイトル案:

| # | トリビアのタイトル案 | 一言 | 材料 (このメモ内) |
|---|---|---|---|
| 1 | `null` 型の AST は TS 4.0 で変わった | tsc が `LiteralType` で包み、typescript-estree が剥がし、oxc はそれに合わせた | 「`null` だけ `TSLiteralType` ではなく `TSNullKeyword`」の節 |
| 2 | タプルの rest がなぜ 1 個までか | 境界が決まらないから。`...T` は展開後に union へ畳まれる (tsc 5.9 で実測) | 2.2 の節、demo17 |
| 3 | oxc は `[...S, ...S]` を通す | tsc はエラー。チェッカーの検査を構文の見た目で近似している | 2.2 の節、demo11-14 |
| 4 | oxc の適合率は「正しいコード 100% / 間違い 62.56%」 | 「Negative Passed」の意味と、除外リストの存在 | 「見逃しは許容されているのか」の節 |
| 5 | `namespace string {}` が書ける | JS は `string` を予約語にしなかった。だから `.` を 1 トークン先読みする | `string`/`number` は予約語じゃない、の節 |
| 6 | `infer T extends U ? A : B` は誰の `extends`? | 曖昧なときだけ checkpoint する 4 分岐 | 1.3 後半の節、demo12-15 |
| 7 | 「立っている」フラグは動的スコープ | `DisallowConditionalTypes` を付け外しして再帰を制御する | 1.3 後半の節 (`context_add`) |
| 8 | `<` は 2 回読まれる | レキサーへの re-lex 依頼。`lexer/typescript.rs` は 53 行中 40 行がコメント | Session 0 の節、demos/oxc-step0 |
| 9 | `{ [K in` と `{ [key:` は 4 トークン目で分かれる | 固定長の先読み。`is_start_of_type` との 2 重管理も | 1.4・2.1 の節 |
| 10 | 型の中に JSDoc が住んでいる | `T?` を JSDoc nullable と conditional の `?` で読み分ける。`*` は未移植 | 1.4 の JSDoc の節 |
| 11 | Microsoft 自身も落とした機能 | `JSDocFunctionType` は tsc 6.0 にあり、ts-go 7.x で無い | 1.4 の JSDoc の節 (比較表) |
| 12 | コメントアウトされた他人のコード | 移植とは何を捨てるかの選択 | 1.3 の LT 有力候補の節 |
| 13 | TS 専用のパーサーは無い | 同じパーサーが `is_ts` (54か所) で TS の枝を切り替える。`.js` に型注釈を書くと普通の JS としてエラー | 2.3 の節の「`is_ts` フラグ」 |
| 14 | AST は1つで JS と TS が混ざる | `TSLiteralType` の中に JS の `TemplateLiteral` が入る。`--estree` はその1つの木を書き出しただけ | 2.3 の節の「AST は1つ」 |
| 15 | tsc の移植の上に足された近道 | `at_start_of_ts_declaration` の高速経路は tsc・ts-go に無い。同じ判定を2か所に書き写し「exactly 一致」とコメントで保証 (`is_start_of_type` の2重管理・`<` の早期リターンも同種) | 4.1 の節の「高速経路」 |
| 21 | プロファイルの1番のホットスポットは、tsc の移植のままの無駄だった | `parse_call_expression_rest` が式の葉ごとに member-rest を再走査していた。tsc・ts-go も同じ形で最適化していない。oxc は1つの `if` で約13%高速化 (PR #23063、AST はバイト単位で同一) | 5.2 の節 |
| 16 | `class A extends B<string> {}` は `<string>` が2回読まれる | 式側の投機が `{` で失敗して rewind し、`try_parse_type_arguments` で読み直す (改行や `implements` が続くと成功する) | 3.3 見直しの節、demos/oxc-step3 の demo6・11・12 |
| 17 | `1 + 1 as number / 2` の `as` は思ったより優先順位が低い | 型を消すだけの除去ツールと意味が食い違う問題 (TypeScript#63527) → ts-go#4192 (Anders) → oxc#22986 (Boshen) が ts-go のマージから約16時間で追従。「`as` の優先順位がこんなに低いとは」と驚く人が多数 | 5.1 の節 |
| 18 | 式は Pratt、型は階層を関数で固定した再帰下降 | `parse_binary_expression_rest` (演算子が多い) と `parse_union_type_or_higher` → ... (演算子が `\|` `&` 程度)。自作の Pratt と並べられる | 5.1 の節 |
| 19 | `as` の優先順位が低いという知られざる仕様を、7.0 で意図的に breaking change にした | 「We shipped *what* precedence??」(Ryan) と TS チーム自身が驚き、ランタイム動作を変えないよう「優先順位の変更」ではなく「エラーにする」を選んだ。top1000 で試して影響を確認する方針 (#63527) | 5.1 の節 |
| 20 | `as` の右辺が式ではなく型だから `/ 2` を飲み込めない | 優先順位が低いだけでは説明できない。右辺が型 (`parse_ts_type`) なので、後ろの `/` を外側のループが `(1 + 1) as number` 全体の左辺として読む | 5.1 の節 |

書き方の方針案: 1 項目 = 入力 1 行 + 結果 (tsc と oxc の出力) + 1〜2 段落。デモは `demos/` から
そのまま引用できる。**未確認だった事項** (H の `namespace string {}` のデモなど) は、書く前にデモで確認する。

**図: TS を読むときに通るファイル** (`oxc/crates/`、行数は rev `1aa5ec11ce`):

```
 oxc_parser/src/
   lexer/typescript.rs        53行   ← `<` `>` の re-lex (`f<T>(x)` vs `a < b > c` のため)
   cursor.rs                 638行   ← checkpoint / rewind / lookahead (投機の道具)
   context.rs                190行   ← DisallowConditionalTypes などの文脈フラグ
   js/statement.rs           932行   ← `is_ts && at_start_of_ts_declaration` で TS の文へ分岐 (:191, :910)
   ts/statement.rs           962行   ← enum / interface / type alias / namespace / declare
   ts/types.rs             1,690行   ← 型の再帰下降 (このメモの主戦場)
   js/expression.rs        1,799行   ← `as` / `satisfies` / `!` / `<T>expr` が式パーサーに埋まっている
   js/arrow.rs               410行   ← アロー関数の曖昧性 (TS 版)
   diagnostics.rs          1,423行   ← TS のエラーコード (TS1265 など) の定義
 oxc_ast/src/ast/ts.rs     1,876行   ← TS ノードの定義 (TSTupleType など)
 oxc_semantic/src/checker/typescript.rs  343行  ← 木全体が要る検査 (infer の位置 TS1338 など)
```

**コツ 1 の実例 (読んでいて出会った曖昧性の道具。どれか 1〜2 個を「例」として使う。実測・デモ済み)**:
- `<` の re-lex: レキサーとパーサーが双方向で協調して `f<T>(x)` と `a < b > c` を見分ける。53 行の
  `lexer/typescript.rs` (Session 0 のメモと demos/oxc-step0)
- 曖昧なときだけ投機する `parse_constraint_of_infer_type` (demo12-15): 「曖昧でない場所では巻き戻さない」
- `{ [K in` と `{ [key:` を 4 トークン目で分ける先読み `is_start_of_mapped_type` (メモ 2.1)
- AST の互換: `null` を `TSNullKeyword` にする (typescript-estree に合わせる。メモの `null` の節)

**コツ 4 の実例 (「出会う落とし穴」。デモ済み)**:
- tuple の rest 検査: tsc は型を解決するチェッカーが出す TS1265 を、oxc は構文の見た目で近似
  → `readonly` / `ReadonlyArray` / エイリアス / `ns.Array<X>` は見逃す (demo11-14)。AST には影響しない
- 適合率: 正しいコードは 100% 通し、間違いを弾くのは 62.56% (未対応のバックログが 987 件)
- JSDoc の `*` (`JSDocAllType`) は未移植 (tsc 6.0 にあり、ts-go 7.x にもある)。`function(...)` 型は ts-go でも無い

**未確認 / 発表前に直しておくこと**:
- 3者の図の「typescript-estree は tsc の AST を変換」は `convert.js` を読んで確認済み。「oxc は typescript-estree の形に合わせる」は
  oxc 側のコメント (`Parse null as TSNullKeyword ... to align with typescript eslint`) と出力の一致から。oxc の公式説明は探していない
- E の TS1338 (infer の位置) は 1.3 のメモに「tsc でもチェッカーが報告」とあるが、tsc のソースでは再確認していない
- H (`namespace string {}`) は知識ベース。デモを作って oxc/tsc で確認してから使う
- ファイル図の呼び出し経路 (`lib.rs` の `parse` → `parse_program` → `js/statement.rs` → `ts/*`) は、分岐の2か所 (`js/statement.rs:191,910`) と各ファイルの行数を確認しただけ。図に載せるならパース全体の流れを実行して確かめる

### 1.4 `parse_postfix_type_or_higher` (358-408) 読み始め

`let mut ty = self.parse_non_array_type();` — ここで取れるのは型文法の**一番下の階層 (primary/atom)**。
まだ postfix (`!` / `?` / `[]`) が付く前の「型の核」で、キーワード型 (`string`/`number`/...) /
リテラル型 / `this` / `typeof x` / `{...}` (object型 or mapped) / `[...]` (タプル) / `(...)` (括弧型) /
`import(...)` / テンプレートリテラル型 / それ以外は `parse_type_reference` (識別子, `Foo.Bar`, `Foo<T>`)
に分配される。この後 `parse_postfix_type_or_higher` のループが `ty` に `!`/`?`/`[]` を後付けしていく。

#### `Kind::Question` 分岐 — `T?` (JSDoc nullable) と `T extends U ? A : B` の `?` の区別

```rust
if self.lookahead(|p| {
    p.bump_any();              // `?` を仮に食べる
    p.is_start_of_type(false)  // その次のトークンが型を開始できるか
}) {
    return ty;  // conditional の `?` だった → 食べずに ty を返す
}
```

`self.lookahead` は checkpoint → closure実行 → **結果に関わらず必ず rewind** なので、
このif自体はカーソル位置を一切変えない「2トークン先読み」。判定の理屈:

- `?` の次が型の開始トークンでない (`;` などで終わる) → `T?` (postfix nullable) 確定。
  この後の `self.bump_any()` で実際に `?` を消費し `JSDocNullableType` を作る
- `?` の次が型の開始トークン (`T extends U ? A : B` の `A` など) → conditional の
  `? trueType : falseType` に見える → **`?` を消費せず** `return ty`。呼び出し元を
  さかのぼって `parse_ts_type` の conditional 分岐まで戻り、そこの `self.expect(Kind::Question)`
  が同じ `?` を消費する

「実際に消費するのは判定が終わった後の本処理側だけ」という、投機と実消費を分離する
このファイルに繰り返し出てくる型のパターンがここにも。

#### なぜ TS パーサーに JSDoc 型が混ざっているのか

oxc の `oxc_parser` は `.ts` 専用ではなく **`.js` の JSDoc コメント型注釈** (`/** @type {string} */`)
もこの型パーサーで面倒を見ている。なので `parse_non_array_type` / `parse_postfix_type_or_higher` には
TS の文法と JSDoc 独自の文法 (`T?` nullable / `T!` non-nullable / `*` any型 など) が同じ関数の中に
混在している。

**実装は不揃い**: `?`(`JSDocNullableType`)と`!`(`JSDocNonNullableType`)は実装済みだが、
`parse_non_array_type` (411あたり) にコメントアウトで残る `*` (`JSDocAllType`) や
`JSDocFunctionType` は tsc 原文だけ残して**未移植**。TS本体の文法 (union/intersection/keyof/...)
がきっちり移植されているのに対し、JSDoc側は「困ったときだけ足す」程度の扱いに見える。
これも Session 1.3 で見た「未移植=優先度の判断」の一例。

未移植コメントの中身 (types.rs:431-444、1.3で見た「コメントアウトされた他人のコード」パターン):

```rust
// TODO: js doc types: `JSDocAllType`, `JSDocFunctionType`
// Kind::StarEq => {
// scanner.reScanAsteriskEqualsToken();
// falls through
// }
// Kind::Star => {
// return parseJSDocAllType();
// }
// case SyntaxKind.QuestionQuestionToken:
// // If there is '??', treat it as prefix-'?' in JSDoc type.
// scanner.reScanQuestionToken();
// // falls through
// case SyntaxKind.FunctionKeyword:
// return parseJSDocFunctionType();
```

- **`Star`(`*`) → `JSDocAllType`**: `@type {*}` (「any型」の意味)。`StarEq`(`*=`)の腕が
  要るのは、通常のJSレキサーが`*=`を1個の複合トークンとして字句解析してしまうから。
  `*=T`のようなJSDoc構文で`*`の直後に`=`が来ると`*=`にまとまるので、
  `reScanAsteriskEqualsToken()`で**レキサーに巻き戻させて**`*`と`=`に再分割してから
  `Star`の腕にフォールスルーする。Session 0の`re-lex`(`<`の再字句解析)と同じ発想が
  `*=`にも使われている
- **`QuestionQuestionToken`(`??`) → `FunctionKeyword` → `JSDocFunctionType`**:
  JSDocには`@type {function(string, number): boolean}`のように`function`キーワードを
  使う関数型の書き方があり、その手前に`??`(nullish coalescing)が来た場合も同様に
  `?`2個へ再分割してフォールスルーする

oxcはこの2つを両方未実装のまま放置している。

**ts-go (Microsoft公式のGo移植版、`~/ghq/github.com/microsoft/TypeScript`
`tsc/internal/parser/parser.go:2804` `parseNonArrayType`) で裏取りした結果、意外な非対称があった**:

```go
case ast.KindAsteriskEqualsToken:
    p.scanner.ReScanAsteriskEqualsToken()
    fallthrough
case ast.KindAsteriskToken:
    return p.parseJSDocAllType()   // ← ts-go は実装している
case ast.KindQuestionQuestionToken:
    p.scanner.ReScanQuestionToken()
    fallthrough
case ast.KindQuestionToken:
    return p.parseJSDocNullableType()
case ast.KindExclamationToken:
    return p.parseJSDocNonNullableType()
// KindFunctionKeyword の分岐が無い = JSDocFunctionType は無い
```

- **`JSDocAllType`(`*`)はts-goにちゃんと実装されている** — classic tscからそのまま移植済み。
  oxcだけがここを未移植
- **`JSDocFunctionType`(`function(...)`構文)はts-go自身にも無い** — TSチーム自身がJS→Go移植の
  際にこれを落としている。oxcの未移植はここに関しては「独自の手抜き」ではなく、
  tsc本家(Go版)も同じ判断をしていた、という裏付けになった

このリポジトリ自体は`go.work`があり中身が丸ごとGoなので、**Microsoft/TypeScriptは既にGo移植を
本体に統合済み**らしい。classic JSソース(`src/compiler/parser.ts`)が要るときは別クローン
`~/Documents/ecosystem/TypeScript`を見る(そちらの`parser.ts:4587`が`parseNonArrayType`)。

#### `[` には2つの入口がある — postfix の `[` とタプル型の `[` は別物

`[K, K, K]` のようなタプル型リテラルと、`postfix`ループの `Kind::LBrack` (`T[]`/`T[K]`) は
**別の場所で処理される完全に別コード**。振り分けの基準は「`[` の前に既にパース済みの型 `ty` が
あるかどうか」:

- `ty` が無い状態で `[` が出てくる (行頭・`=`の直後など) → `parse_non_array_type` (411あたり) の
  分配器がそのまま拾って `Kind::LBrack => self.parse_tuple_type()`。`[` 自体が型の**先頭トークン**
- `ty` が既にある状態で `[` が出てくる (`T[...]`) → `parse_postfix_type_or_higher` のループが
  postfix として拾う。ここは `T[]`(配列) か `T[K]`(インデックスアクセス) の2択だけで、
  複数要素・カンマ区切りは扱わない (`T[K, K, K]` という文法自体がTSに存在しない)

#### `null` だけ `TSLiteralType` ではなく `TSNullKeyword` — typescript-estree 仕様への追従

`parse_non_array_type` の `Kind::Null` にコメント: `Parse null as TSNullKeyword instead of
null literal to align with typescript eslint.`

`true`/`false`/文字列/数値リテラルは `parse_literal_type()` で **`TSLiteralType`**(内部に
`Literal` ノードを持つ「リテラル型」)として作られるのに、`null` だけは `string`/`number` と
同じ「キーワード型」グループに混ぜて `parse_keyword_type()` (単に `TSNullKeyword` ノードを作る
だけ)で処理される。

理由は TS コンパイラ自身の内部AST(`ts.SyntaxKind`)ではなく、**`@typescript-eslint/typescript-estree`**
(ESLint 向けにTSコードをESTree形式へ変換するパッケージ)の AST 仕様に合わせるため。そちらの
`AST_NODE_TYPES` では `null` 型が `TSNullKeyword` という専用ノードとして定義されている
(`TSAnyKeyword`/`TSBooleanKeyword`/`TSNeverKeyword`/...と同じキーワード型ファミリー)。

oxc の `--estree` 出力(実際に `cargo run --example parser -- ... --estree` で見ているJSON)が
typescript-estree の出力と構造的に一致するようにしているのは、typescript-eslint 向けに書かれた
既存の ESLint ルールがパーサーだけ oxc(oxlint)に差し替えてもそのまま動くようにするため。
「TSコンパイラの内部表現に忠実」ではなく「ESLintエコシステムが期待するASTの形に忠実」という、
ESTree互換性の優先度が単語ひとつのレベルにまで及んでいる例。

##### tsc 側の実測: `null` 型のツリーは **TS 4.0 で変わった** (バージョン依存)

`type A = null;` の AST を、手元の各バージョンの `ts.createSourceFile` で実際にダンプして比較
(`type B = undefined; type C = true; type D = string;` も同時に出して対比):

| 実装 | `null` | `undefined` | `true` | `string` |
|---|---|---|---|---|
| tsc 4.6.3 / 4.8.4 / 5.3.3 / 5.5.2 / 5.9.3 / 6.0.2 / 6.0.3 (**実測**) | `LiteralType > NullKeyword` | `UndefinedKeyword` | `LiteralType > TrueKeyword` | `StringKeyword` |
| tsc 3.9.10 以前 (**ソース読みのみ**、実行はしていない) | `NullKeyword` (単体) | 同左 | 同左 | 同左 |
| ts-go 7.1.0-dev (**ソース読みのみ**、Go 未インストールで実行不可) | `LiteralType > NullKeyword` 相当 | — | — | — |
| oxc `--estree` (**実測**) | `TSNullKeyword` (単体) | `TSUndefinedKeyword` | `TSLiteralType > Literal` | `TSStringKeyword` |

**変わった箇所**: `parseNonArrayType` の `case SyntaxKind.NullKeyword` が、3.9.10 までは
`VoidKeyword` と同じ腕で `return parseTokenNode<TypeNode>()` (=キーワードのトークンをそのまま
型ノードにする)、4.0.8 では `TrueKeyword`/`FalseKeyword` と同じ腕に移って
`return parseLiteralTypeNode()` になっている。変更コミットは
`eb3645f16b` 「Refactor node factory API, use node factory in parser (#35282)」
(2020-06-16、TS 4.0)。`null` 専用の変更ではなく、**ノードファクトリー導入リファクタの副作用**
として `null` が `LiteralType` の内側に入った、という差分に見える (diff の該当箇所は
`+case SyntaxKind.NullKeyword:` / `-case SyntaxKind.NullKeyword:` の移動だけ)。

ts-go (`tsc/internal/parser/parser.go`) も 4.0 以降の形を引き継いでいる:

```go
case ast.KindNoSubstitutionTemplateLiteral, ast.KindStringLiteral, ast.KindNumericLiteral,
     ast.KindBigIntLiteral, ast.KindTrueKeyword, ast.KindFalseKeyword, ast.KindNullKeyword:
    return p.parseLiteralTypeNode(false)
```

##### typescript-estree の答え合わせ (実物を読んで確認済み)

`@typescript-eslint/typescript-estree` 8.26.1 の `dist/convert.js:2439`:

```js
case SyntaxKind.LiteralType: {
    if (node.literal.kind === SyntaxKind.NullKeyword) {
        // 4.0 started nesting null types inside a LiteralType node
        // but our AST is designed around the old way of null being a keyword
        return this.createNode(node.literal, { type: AST_NODE_TYPES.TSNullKeyword });
    }
    return this.createNode(node, { type: AST_NODE_TYPES.TSLiteralType, literal: ... });
}
```

**これで経緯が一本につながった**: typescript-estree の AST は TS 3.9 以前の形
(`null` = キーワード型) を前提に設計されていて、TS 4.0 が `LiteralType` で包むようになった
とき、**typescript-estree 側が `LiteralType > NullKeyword` を `TSNullKeyword` に剥がして
互換性を保った**。oxc が `Kind::Null` を最初から `TSNullKeyword` として作るのは、その
typescript-estree の出力に合わせた結果。

訂正: 以前のメモの「`TSNullKeyword` は tsc 自身の AST には存在しない」は **不正確**。
TS ≤3.9 では `null` 型はキーワード単体のノードで、現行の tsc (4.0〜6.0 / ts-go) が
変わっただけ。oxc は「tsc の現行 AST とは違う」が「tsc の昔の AST・typescript-estree とは同じ」。

再実行 (tsc):

```js
const ts = require('<typescriptのパス>');
const sf = ts.createSourceFile('x.ts', 'type A = null;', ts.ScriptTarget.Latest, true);
// sf.statements[0].type.kind → LiteralType、その子が NullKeyword
```

#### `string`/`number`/`boolean` は予約語じゃない — `.` の1トークン先読みで見分ける

`parse_non_array_type` の `Kind::Any | Kind::Unknown | Kind::String | ... | Kind::Null` の腕:

```rust
if self.lexer.peek_token().kind() == Kind::Dot {
    self.parse_type_reference()   // 修飾名として読む
} else {
    self.parse_keyword_type()     // プリミティブ型として読む
}
```

TSでは`string`/`number`/`boolean`等は**予約語ではない**ので、識別子として(namespace名などに)
使える:

```ts
namespace string {
  export type Foo = number;
}
type X = string.Foo;  // ← ここの `string` はプリミティブ型ではなく namespace 名
```

これを素直に`parse_keyword_type()`で処理すると`string`だけ読んで`TSStringKeyword`を作った
時点で終わり、後ろの`.Foo`が浮いてエラーになる。なので**直後が`.`かだけ先に覗いて**、
続くなら「プリミティブ型ではなく修飾名の先頭」と判断し`parse_type_reference`に回す。
`Kind::Minus`(直後が数値なら負数リテラル)と同じ「1トークン覗いて2つの文法を振り分ける」パターン。

`parse_tuple_type` 自体の詳細 (named tuple member の曖昧性など) は Session 2.2 に回す。

### 型パーサー全体地図 (1.1-1.4 時点)

1.4 まで読んだところで、降下ルートと「あちこちから戻ってくる再入ポイント」の全体像を整理。

#### 主降下ルート — 優先順位が固定されたはしご

```
parse_ts_type (15)
  ├─ まず function/constructor 型かだけ判定 → 該当なら丸ごとバイパスして
  │   parse_function_or_constructor_type (46) へ (1.2 で読了。中の params/戻り値型で
  │   parse_ts_type に戻る再入あり)
  └─ そうでなければ parse_union_type_or_higher (245) へ … `|`
       └ parse_intersection_type_or_higher (241) … `&`
         └ parse_type_operator_or_higher (282) … 前置 keyof/unique/readonly/infer
           └ parse_postfix_type_or_higher (358) … 後置 `!`/`?`/`[]`/`[K]`
             └ parse_non_array_type (411) … primary の大分配器
                ├ keyword型 (string/number/…) → parse_keyword_type (504、配管のみ)
                ├ リテラル (str/true/false/数値) → parse_literal_type
                ├ `{` → is_start_of_mapped_type で投機 → parse_mapped_type (655) / parse_type_literal (697)
                ├ `[` → parse_tuple_type (978、Session 2.2)
                ├ `(` → parse_parenthesized_type (1109)
                ├ `import(...)` → parse_ts_import_type (1151)
                ├ `typeof` → parse_type_query
                ├ `asserts` → 投機 → parse_asserts_type_predicate (810) / parse_type_reference
                ├ テンプレート → parse_template_type (772、Session 2.3)
                └ それ以外 (識別子等) → parse_type_reference (822、配管のみ) →
                    parse_ts_type_name (左結合の `.` ループ) + 型引数は `<` の re-lex (Session 0/3.3 を再利用)
```

**conditional (`extends ... ? ... : ...`) はこのはしごのどこにもぶら下がらない** —
`parse_ts_type` 自身が `ty` を読み終えた**直後**(15-44)に自分で `extends` の有無を見て、
自分で3つの枝を読む。はしごの外側にいる特別扱い。

#### 再入ポイント — `parse_ts_type` に戻ってくる場所たち

はしごは一直線に見えて、実際は色んな場所から`parse_ts_type`(または`Self::parse_ts_type`)
に**戻ってくる**呼び出しがあり、木というよりグラフに近い。実コードを grep して洗い出した一覧:

| 呼び出し元 | 何のために戻るか | 行 |
|---|---|---|
| `parse_ts_type` 自身 | conditional の3枝 (extends_type/trueType/falseType) | 26, 30, 33 |
| `parse_constraint_of_infer_type` | `infer T extends U` の `U` | 345, 349 |
| `parse_postfix_type_or_higher` | `T[K]` の `K` (インデックスアクセス) | 392 |
| `parse_mapped_type` | `[K in T]` の制約式 / `as` 名前型 | 658, 660 |
| `parse_parenthesized_type` | `(...)` の中身 | 1109 |
| `parse_tuple_type` / タプル要素 | 各要素の型 | 1050, 1079 |
| `parse_return_type` → `parse_type_or_type_predicate` | 関数型の戻り値型 (`x is T` 上乗せの専用入口) | 1329-1338 |
| `parse_ts_import_type` | `import("mod").Foo` の中身 | 1151 |
| `try_parse_type_arguments` / `parse_type_arguments_in_expression` | `<T, U>` の各要素 (delimited list の callback) | 872, 898, 936 |
| `parse_ts_type_constraint` / `parse_ts_default_type` | 型パラメータの `extends U = V` | 751, 759 |
| `parse_template_type` | テンプレート型の `${T}` 部分 | 772, 788 |

`parse_type_literal` (`{ ... }` の object型) だけは違う経路で、`parse_ts_type_signature`
(Session 3.1 `parse_signature_member` 一族) に委譲していて、メンバー型は間接的にしか
`parse_ts_type` に戻らない。ここは Session 3.1 で深追いする。

**読み方の要点**: 「はしごを降りて `parse_non_array_type` で primary を1個読む」→
「その primary の中に別の型が埋まっていたら、そこで `parse_ts_type` を呼んで**はしごの一番上から
やり直す**」の繰り返し。だから全経路は結局 `parse_ts_type` (15) と `parse_non_array_type` (411)
の2つを通る、というロードマップの「読み方のコツ」の一文が実データで裏付けられた。

### 2.1 読み始め: `is_start_of_mapped_type` (types.rs:606) — 区別が最後の1トークンで決まる

`parse_non_array_type` の `Kind::LCurly` の腕は `self.lookahead(Self::is_start_of_mapped_type)` で
mapped type (`{ [K in T]: U }`) か普通のオブジェクト型リテラルかを決めてから
`parse_mapped_type` / `parse_type_literal` に分岐する。`lookahead` 内なので `bump` した分は判定後に全部巻き戻る。

```rust
fn is_start_of_mapped_type(&mut self) -> bool {
    self.bump_any();                          // `{`
    let kind = self.cur_kind();
    if kind == Kind::Plus || kind == Kind::Minus {
        self.bump_any();
        return self.at(Kind::Readonly);       // `{ +readonly` / `{ -readonly` → 即 mapped 確定
    }
    self.bump(Kind::Readonly);                // `readonly` があれば食べる (無くてもよい)
    if !self.eat(Kind::LBrack) && self.cur_kind().is_identifier_name() {
        return false;                         // `[` が無く識別子 → `{ a: string }` のプロパティ
    }
    self.bump_any();                          // `[` の次 (キー名 K)
    self.at(Kind::In)                         // 次が `in` なら mapped
}
```

**`{ [K in` と `{ [key:` は `[識別子` までまったく同じ形で、4トークン目で初めて分かれる**:

| 入力 | 判定 | 決め手 |
|---|---|---|
| `{ [K in T]: U }` | mapped | `[` `K` の次が `in` |
| `{ [key: string]: T }` | 普通 (index signature) | 次が `:` |
| `{ readonly [K in T]: U }` | mapped | `readonly` を読み飛ばした後は同じ |
| `{ -readonly [K in T]: U }` | mapped | `+`/`-` の後に `readonly` があれば即確定 |
| `{ a: string }` | 普通 | `[` が無く識別子 |

これまでの `is_start_of_*` 系で一番深い先読み (`{` `[` 識別子 `in` の4トークン、`readonly` があれば+1)。
「先頭の `{` だけでは決まらないが、固定長の先読みで必ず決まる」ので、投機 (checkpoint + 本パース + rewind)
ではなく純粋な先読み判定で済んでいる。

`+`/`-` の分岐で即 return できるのは、`{` の直後に `+`/`-` が来る正当な型が `+readonly`/`-readonly` の
mapped type しかないから (プロパティ名は `+`/`-` で始まらない)。mapped type の modifier は
`{ -readonly [K in T]-?: U }` のように `readonly` の前と `?` の前の2か所に付けられるが、この関数が見るのは
前者 (`{` の直後) だけで、後者の `-?` は本体 `parse_mapped_type` が読む。

### 2.1 `parse_mapped_type` (types.rs:640-697) — 読む順 = 文法規則、tsc との対応

`{ readonly? [K in T as N]? : V ; }` を上から順に読むだけの素直な関数 (投機なし。判定は呼び出し前の
`is_start_of_mapped_type` で済んでいる)。

```rust
self.expect(Kind::LCurly);
// readonly / +readonly / -readonly  → Option<TSMappedTypeModifierOperator>
self.expect(Kind::LBrack);
if !self.cur_kind().is_identifier_name() { return self.unexpected(); }   // キー名になれるトークンか
let key = self.parse_binding_identifier();      // K は「宣言する側」の名前 (BindingIdentifier)
self.expect(Kind::In);
let constraint = self.parse_ts_type();          // 回す対象のキー (keyof T など)
let name_type = if self.eat(Kind::As) { Some(self.parse_ts_type()) } else { None };  // as 句
self.expect(Kind::RBrack);
// ? / +? / -?  → Option<TSMappedTypeModifierOperator>
let type_annotation = self.eat(Kind::Colon).then(|| self.parse_ts_type());  // 値の型
self.bump(Kind::Semicolon);
self.expect(Kind::RCurly);
```

**型が3つ出てくる**: `constraint` (回す対象のキー) / `name_type` (`as` 句 = **キーの新しい名前を決める型**。
値の型ではなくキー側を書き換える) / `type_annotation` (`:` の後の値の型)。

**`TSMappedTypeModifierOperator` (True / Plus / Minus)**: `readonly` と `?` の修飾子が3通りの書き方
(`readonly` / `+readonly` / `-readonly`、`?` / `+?` / `-?`) を取れるので3値 enum。`True` は「符号なしで素直に
付いている」、無指定は `None`。`True` と `Plus` は意味が同じだがソース上で `+` を書いたかを区別するため別値。
ESTree では `True` → `true`、`Plus`/`Minus` → `"+"`/`"-"` (typescript-estree に合わせた形。`null` のときと同じ
「ESTree 互換のための形」の例)。

**`K` を `BindingIdentifier` で読む理由**: `[K in T]` の `K` は「mapped type の中だけで有効な型パラメータを
宣言する」名前だから。`BindingIdentifier` は semantic 解析で `symbol_id` が付く「宣言側」のノード
(使う側は `IdentifierReference`、`a.if` のような予約語も許す名前は `IdentifierName`)。`TSMappedType` に
`scope_id` があるのも同じ理由。キー位置の `is_identifier_name` チェックは緩い事前ゲート
(識別子とキーワード全般を通す)、その後の `parse_binding_identifier` が予約語を専用エラーで弾く。
事前チェックがなくても結果は変わらないはずだが、なぜ置いてあるかは未確認。

#### tsc (6.0.3 `parser.ts:4410` `parseMappedType`) との対応

| tsc `MappedTypeNode` | oxc `TSMappedType` |
|---|---|
| `typeParameter` (名前 + `in` + 制約を `TypeParameterDeclaration` 1個にまとめる) | `key` + `constraint` (別フィールド) |
| `nameType` | `name_type` |
| `type` | `type_annotation` |
| `readonlyToken` (`readonly`/`+`/`-` のトークンノード) | `readonly` (3値 enum) |
| `questionToken` (`?`/`+`/`-` のトークンノード) | `optional` (3値 enum) |
| `members` (「文法エラーを出すためだけ」のコメント付き) | 対応する処理が見当たらない |

`as` 句 (`parseOptional(AsKeyword) ? parseType() : undefined`) は oxc と 1:1 対応。キー名の読みは
tsc が `parseIdentifierName()` (予約語も許す緩い読み) のまま通すのに対し、oxc は
`is_identifier_name` → `parse_binding_identifier` の2段で、予約語だけ後段で弾く点が少し違う。

### 2.2 読み始め: `parse_tuple_type` (types.rs:967) — 並び順の検査と、パーサー/チェッカー境界の訂正

(`parse_tuple_element` = 要素1つの読み方は未読。ここは `parse_tuple_type` 本体の検査ロジックだけ。)

`parse_delimited_list` に渡すクロージャの中で、要素を1つ読むたびに次の2つの状態を更新する:

```rust
let mut seen_rest_span: Option<Span> = None;      // これまでに見た rest 要素 (`...T[]`) の位置
let mut seen_optional_span: Option<Span> = None;  // これまでに見た optional 要素 (`a?: T` / `T?`) の位置
```

`Option<Span>` なのは「まだ見ていない = `None`、見た = `Some(位置)`」で、位置は診断の
「先に見た側」のラベル (`First seen here`) にそのまま使うため。3つのエラーを出す:

| 入力 | エラー | 使う状態 |
|---|---|---|
| `[...string[], ...number[]]` | TS1265 rest の後に rest はだめ | `seen_rest_span` |
| `[a?: string, b: number]` / `[string?, number]` | TS1257 optional の後に必須はだめ | `seen_optional_span` |
| `[...string[], a?: number]` | TS1266 rest の後に optional はだめ | `seen_rest_span` |

エラーにならない: `[...string[], number]` (rest の後の必須は許される。tsc 5.9 で確認)、
`[a?: string, ...boolean[]]` (rest は「必須ではない要素」扱い)、`[...string[], ...T]` (`T` は型パラメータ)。
実証は `demos/oxc-step2/` (demo1-13 + `tsc_check.js`)。

**`me.error(...)` がやること**: 診断を1件ためるだけ ([error_handler.rs:59](../../../oxc-project/oxc/crates/oxc_parser/src/error_handler.rs#L59)
`self.errors.push(error)`)。**致命的ではない**ので、パースは止まらず AST も最後まで作られる
(`unexpected()` / `fatal_error` とは別物)。エラーは最後にまとめて報告される。

3つの診断は、どれも「今の要素」と「先に見た要素」の2か所にラベルを付けて作る
([diagnostics.rs:425-445](../../../oxc-project/oxc/crates/oxc_parser/src/diagnostics.rs#L425)):

| 関数 | TS | ラベル1 (今の要素) | ラベル2 (先に見た要素) |
|---|---|---|---|
| `required_element_cannot_follow_optional_element` | 1257 | Required element here | Optional element seen here |
| `rest_element_cannot_follow_another_rest_element` | 1265 | Second rest element here | First seen here |
| `optional_element_cannot_follow_rest_element` | 1266 | Optional element here | Rest element seen here |

渡す2つの `Span` が `tuple.span()` (今の要素) と `seen_*_span` (先に見た要素)。`seen_*_span` を
`Option<Span>` で持つのは、この「先に見た側」のラベルに使うため。

#### `parse_tuple_element` (types.rs:1044) — 名前付きの判別と、JSDocNullableType の読み直し

**名前付き (`[a: string]`) か普通の型 (`[a]`) かの判別**は、`lookahead` で「識別子 + 任意の `?` + `:`」
の3トークンを見るだけ (`is_next_token_colon_or_question_colon`、[types.rs:1080](../../../oxc-project/oxc/crates/oxc_parser/src/ts/types.rs#L1080) あたり):

```rust
fn is_next_token_colon_or_question_colon(&mut self) -> bool {
    self.bump_any();            // 識別子
    self.bump(Kind::Question);  // `?` があれば
    self.at(Kind::Colon)        // 次が `:` か
}
```

| 入力 | 判定 |
|---|---|
| `[a: string]` / `[a?: string]` | 名前付き |
| `[a]` / `[a?]` | 普通の型 (`a` / `a?`) |

`is_start_of_mapped_type` (4トークン) より短い固定長の先読み。判定より後ろの処理 (`...` や `?` の
位置違いを受け付けてエラーだけ出す) は変な書き方を拾うための後始末で、パースの本筋ではない。

**`JSDocNullableType` の読み直し (`convert_type_to_tuple_element`、:1083)**: `[string?]` の `string?` は、
まず `parse_postfix_type_or_higher` の `?` の分岐 (次が `]` で型の開始ではない) で
`JSDocNullableType` (postfix) として作られる。そのままだと「JSDoc の nullable」だが、タプルの中では
意味が「optional な要素」なので、タプルの文脈だけ `TSOptionalType` に読み替える:

```
string?  →  JSDocNullableType(postfix)  →  TSOptionalType   (タプルの中だけ)
```

```rust
fn convert_type_to_tuple_element(&self, ty: TSType<'a>) -> TSTupleElement<'a> {
    if let TSType::JSDocNullableType(ty) = ty {
        if ty.postfix { TSTupleElement::new_ts_optional_type(ty.span, ty.unbox().type_annotation, self) }
        else          { TSTupleElement::JSDocNullableType(ty) }      // 前置 `?T` はそのまま
    } else { TSTupleElement::from(ty) }
}
```

1.4 で見た「`?` の次が型の開始でなければ postfix nullable」という判定が、タプルの中では
optional 要素を作るための下ごしらえとして使われている、という接続。JSDoc 型が TS の型パーサーに
同居している (1.4) ことの、実際の使われ方の例でもある。

#### 訂正: この検査は tsc ではチェッカーが出している (oxc は構文で近似)

最初「同じタプル内の前の要素だけ見れば決まるのでパーサーが出す (1.3 のローカル/木全体の境界線どおり)」
と説明したが、**tsc の実物 (`checker.ts` `checkTupleType`) を読むと3つとも型の解決が要るチェッカーの検査**だった。

- tsc: `...` の後ろを `getTypeFromTypeNode` で解決し、`isArrayType(type)` (か rest を含むタプル) のときだけ
  `Variadic` を `Rest` に昇格させて数える。型パラメータ `T` は昇格しない (何に展開されるか未定なので)
- oxc: 解決はできないので、`...` の後ろが「`X[]` という構文」か「名前が `Array` の型参照」かという
  **書かれた見た目**で判定する近似

前に `...string[]` がある状態で `...` の後ろの書き方を変えて比べた結果 (実測):

| `...` の後ろ | oxc | tsc 5.9 |
|---|---|---|
| `number[]` / `Array<number>` | エラー | エラー |
| `readonly number[]` / `ReadonlyArray<number>` | OK (見逃す) | エラー |
| `S` (`type S = string[]` のエイリアス) | OK (見逃す) | エラー |
| `T` (型パラメータ) / `[number, string]` (タプル) | OK | OK |

`T` とタプルが通るのは両者で一致する。見逃すのは「見た目が配列らしくないだけで中身は配列」なもの
(`readonly` 付き・`ReadonlyArray`・エイリアス)。

1.3 の境界線は「ローカルに閉じる検査はパーサー」と書いたが、正確には**「判定に型の解決が要るかどうか」**が
軸で、タプル内の前の要素だけで決まるように**見える**検査でも、解決が要るなら本来はチェッカーの仕事。
oxc はそれを構文の範囲で近似してパーサーで出している (近似にした理由は確認していない)。

#### なぜ rest は1個までなのか — 区切りが決まらないから

意味的な理由は「**最初の rest と次の rest の境界が型から決まらない**」こと。tsc は
`createNormalizedTupleType` (`checker.ts:17267`) のコメントにある通り、タプル型を次の2つの形しか
取れないように正規化して保持する:

1. 必須要素 → optional 要素 → rest 0〜1個
2. 必須要素 → rest 1個 → 必須要素

`[...string[], ...number[]]` に `["a", "b", 1, 2]` を入れると、どこまでが `string[]` でどこからが
`number[]` かが決まらない (`[...string[], ...string[]]` なら境界は完全に不定)。すると `t[3]` の型・
`length`・代入できるかが定まらない。

ただし **`...T` が展開されて rest が2個になる分は tsc が黙って union に畳んで受け入れる**
(`createNormalizedTupleType` の「first rest と last optional/rest の間を1つの rest に畳む」処理)。
tsc 5.9 で実測 (`demos/oxc-step2/demo17`、`tsc_type_of.js`):

| 入力 (`A<T> = [...string[], ...T]`) | 解決後の型 |
|---|---|
| `A<number[]>` | `(string \| number)[]` |
| `A<[boolean, null]>` | `[...string[], boolean, null]` |
| `A<[]>` | `string[]` |

書かれたコードで2個ある形は拒否し、展開後に2個になる形は畳んで受け入れる、という使い分け。
これが「`...T` を最初のチェックで rest と数えない」理由 (前節の表の `T` が OK な理由) にもつながる。

#### 見逃しは許容されているのか — 正しいコードは全部通す / 間違いを弾くのは未対応が残っている

`readonly` / `ReadonlyArray` / エイリアスの見逃しは、**AST は普通に作られて診断が1件出ないだけ**で、
パースには影響しない。oxc 全体の状況は、TypeScript 本家のテストを使った互換性集計
(`parser_typescript.snap`。oxc は rev `1aa5ec11ce`。スナップショット先頭の `commit: b465fdbf` は
test262 (`be13516f`) や babel (`1eac4481`) の snap と値が違うので、oxc ではなくテストスイート側の
コミットと思われる。未確認) から読める:

| 項目 | 結果 |
|---|---|
| 正しいコードが通るか (Positive Passed) | 9783 / 9783 (**100%**) |
| エラーになるべきコードを弾けたか (Negative Passed) | 1649 / 2636 (**62.56%**) |
| 見逃し (`Expect Syntax Error:` の行数) | **987 件** (= 2636 − 1649) |

**「Negative Passed」の意味** (公式ドキュメントには記述なし。`website/src` の md/mdx と oxc の md を
「Negative Passed」で grep して、ヒットは `tasks/coverage/src/lib.rs` (出力) と
`tasks/coverage/src/typescript/constants.rs` の冒頭コメントだけ。README は実行方法のみ):

- 集計の目的 (constants.rs のコメント): パーサーは「正しい構文をエラーなく通す」ことと「不正な構文を
  検出する」こと、semantic は「対応しているチェックの検出」を測る
- **分母 2636 は tsc が出すエラー全部ではない**。型推論が要るテストやコンパイラオプション依存のテストは
  `NOT_SUPPORTED_TEST_PATHS` (ファイル単位) と `NOT_SUPPORTED_ERROR_CODES` (エラーコード単位) で
  **意図的に除外**してある。除外しないと「Negative Passed の数字が低いままになり、対応できるものが
  `Expect Syntax Error` の行に埋もれる」ため
- 除外した後にエラーコードが残るテストは「oxc も何らかのエラーを報告する必要がある」もの (原文)。
  つまり見逃し 987 件は「許容と決めたもの」ではなく、**意図的な対象外を除いた残り = 未対応のバックログ**

以前ここに「許容できる」と書いたのは言い過ぎだった。正しくは、意図的に対象外にしたものは除外リストに
入っていて、リストに入っていない見逃しは「まだ出来ていない」扱い。

同じコメントに **「同じエラーコードでも tsc の別の部品から出る。パース時に検出できる場合も、型推論の結果で
初めて分かる場合もある。oxc の対応が限定的だとコードだけでは除外できず、ファイル単位で除外する」** とある。
今回の TS1265 (tsc ではチェッカー、oxc では構文で近似したパーサー) はまさにこの例。

タプル関係では `restTupleElements1.ts` / `variadicTuples2.ts` (今回の3エラー) は弾けていて (1257/1265/1266 は
`NOT_SUPPORTED_ERROR_CODES` に入っていない = 対応対象)、`unionsOfTupleTypes1.ts` /
`contextualTypeTupleEnd.ts` は見逃しリストにある (後者2件の中身は未確認)。

#### 出典 (2.2 の節で書いたことの根拠)

oxc は rev `1aa5ec11ce`、リンクは `../../../oxc-project/oxc/` 配下。

- 並び順検査の実装 (`seen_rest_span` / `seen_optional_span`、3つのエラー):
  [types.rs:967-](../../../oxc-project/oxc/crates/oxc_parser/src/ts/types.rs#L967)、
  診断の定義 [diagnostics.rs:431](../../../oxc-project/oxc/crates/oxc_parser/src/diagnostics.rs#L431)
- tsc 側の同じ検査: `~/Documents/ecosystem/TypeScript/src/compiler/checker.ts:42029` `checkTupleType`
  (そのクローンは HEAD `15392346d0` 2025-02-28、package.json 5.9.0。`ts.version` は `5.9.0-dev`)。
  エラーの実測は `demos/oxc-step2/tsc_check.js` (同じ 5.9.0-dev)
- rest が1個までの理由と畳み込み: `~/Documents/ecosystem/TypeScript/src/compiler/checker.ts:17267` `createNormalizedTupleType`
  (同じ 5.9.0-dev)。実測は `demos/oxc-step2/demo17_rest_normalization.ts` + `tsc_type_of.js`
- 適合率の数字: [parser_typescript.snap:6](../../../oxc-project/oxc/tasks/coverage/snapshots/parser_typescript.snap#L6)
  (`Negative Passed: 1649/2636`)、`Expect Syntax Error:` 行の数 987 は `grep -c` で数えた
- 「Negative Passed」の意味:
  [constants.rs:15](../../../oxc-project/oxc/tasks/coverage/src/typescript/constants.rs#L15) (目的)、
  [:24](../../../oxc-project/oxc/tasks/coverage/src/typescript/constants.rs#L24) (除外しないと数字が低いままになる理由)、
  [:34](../../../oxc-project/oxc/tasks/coverage/src/typescript/constants.rs#L34) (除外後に残るコードは oxc も報告が必要)、
  [:46](../../../oxc-project/oxc/tasks/coverage/src/typescript/constants.rs#L46) (同じコードが tsc の別の部品から出る話)、
  除外リスト [:52](../../../oxc-project/oxc/tasks/coverage/src/typescript/constants.rs#L52) `NOT_SUPPORTED_TEST_PATHS`・
  [:108](../../../oxc-project/oxc/tasks/coverage/src/typescript/constants.rs#L108) `NOT_SUPPORTED_ERROR_CODES`、
  数字を出す側 [lib.rs:183](../../../oxc-project/oxc/tasks/coverage/src/lib.rs#L183)
- タプルのテスト: [restTupleElements1.ts](../../../oxc-project/oxc/tasks/coverage/snapshots/parser_typescript.snap#L31019)・
  [variadicTuples2.ts](../../../oxc-project/oxc/tasks/coverage/snapshots/parser_typescript.snap#L31029) の診断
- 公式ドキュメントに記述が無いという確認: `oxc-project/website/src` の md/mdx と oxc 内の md/rs を
  「Negative Passed」で grep。ヒットは lib.rs と constants.rs のコメントだけ (`tasks/coverage/README.md` は実行方法のみ)

未確認: `readonly` / エイリアスの見逃しを oxc が把握しているか (テストに名指しの項目は無かった)。
**推測**: oxc はフォーマッタ・リンタ・変換が主用途で、型の誤りは tsc を別に走らせて検出する前提。

### 2.3 `parse_template_type` (types.rs:762) — テンプレートリテラル型と `}` の再読

**4種類のトークン** (`lexer/template.rs`、ECMAScript 仕様の名前)。実際にレキサーが出した列
(`tokens_dump` で実測):

```
type A = `abc`;
  9..14  NoSubstitutionTemplate  "`abc`"          ← `${}` が無い。全体で1トークン

type B = `a${string}b${number}c`;
 25..29  TemplateHead      "`a${"                 ← 先頭: バッククォートから `${` まで
 29..35  String            "string"               ← `${ }` の中身は普通のトークン
 35..39  TemplateMiddle    "}b${"                 ← 途中: `}` から次の `${` まで
 39..45  Number            "number"
 45..48  TemplateTail      "}c`"                  ← 末尾: `}` からバッククォートまで
```

| トークン | 形 | いつ出るか |
|---|---|---|
| `NoSubstitutionTemplate` | `` `...` `` | `${}` が1つも無い |
| `TemplateHead` | `` `...${ `` | `${` で終わる最初の部分 |
| `TemplateMiddle` | `` }...${ `` | 途中の `}` から次の `${` まで |
| `TemplateTail` | `` }...` `` | 最後の `}` からバッククォートまで |

`${...}` の中の式や型は、普通のトークン (`String` / `Number` など) として間に挟まる。

`parse_non_array_type` の振り分けは、`NoSubstitutionTemplate` はそのままリテラル型 (`TSLiteralType`)、
`TemplateHead` は `parse_template_type` へ。`${}` の有無で別のトークンの種類に分かれているのが、
テンプレートリテラルを読む入口の分岐。

**`TemplateHead` の腕の流れ** (`` `a${string}b${number}c` `` で追う):

```rust
Kind::TemplateHead => {
    quasis.push(self.parse_template_element(tagged));   // ① `a${` を文字列部分として登録
    types.push(self.parse_ts_type());                    // ② `string` を型として読む
    self.re_lex_template_substitution_tail();            // ③ `}` を読み直す (レキサーとの連携)
    while self.fatal_error.is_none() {
        match self.cur_kind() {
            Kind::TemplateTail   => { quasis.push(..); break; }   // ④ 末尾で終了
            Kind::TemplateMiddle => { quasis.push(..); }          // ⑤ 途中を登録して次へ
            Kind::Eof            => { self.expect(Kind::TemplateTail); break; }  // ⑥ 閉じ忘れ
            _ => { types.push(self.parse_ts_type()); self.re_lex_template_substitution_tail(); } // ⑦ 次の型 → また再読
        }
    }
}
```

`quasis` (文字列の部分: `a` / `b` / `c`) と `types` (`${}` の中の型) が交互に並ぶ。

**「レキサーとの連携」の正体 = ③ `re_lex_template_substitution_tail`** (`cursor.rs:242`):

```rust
pub(crate) fn re_lex_template_substitution_tail(&mut self) {
    if self.at(Kind::RCurly) {
        self.token = self.lexer.next_template_substitution_tail();   // `}` から読み直し
    }
}
```

型 `string` を読み終えた時点の現在トークンは `}` (`RCurly`)。レキサーは前後の文脈を知らないので、
`}` を普通の記号として `RCurly` にしている。しかしその後ろはもうテンプレートの文字列部分
(`b${` や `c` `` ` ``) なので、**パーサーが「ここは `${` の終わりだ」と分かっている側として、レキサーに
「この `}` から、テンプレートの続きとして読み直して」と頼む**。すると `next_template_substitution_tail`
(`lexer/template.rs:398`) が `TemplateMiddle` か `TemplateTail` を作り直す。

Session 0 の `<` の re-lex と同じ型 (文脈を知るのはパーサー側なので、パーサーが再読を要請する)。
`<` では曖昧性の解消 (成功したら確定、失敗したら書き戻す) だったが、こちらは `}` の次が必ずテンプレートの
続きなので、**必ず再読が要る** (自分の読み。走らせて確かめてはいない)。`NoSubstitutionTemplate` は
`${}` が無く1トークンで終わるので再読は起きない。

型 (`parse_template_type`) と式 (`parse_template_literal`、`js/expression.rs:555`) は、ほぼ同じ作りで
`re_lex_template_substitution_tail` を共有している。

**`quasis` と `types` の違い** (`TSTemplateLiteralType`、`oxc_ast/src/ast/ts.rs:1639`):

- `quasis` = 文字列の部分。`types` = `${}` の中の型
- 交互に並び、**`quasis` は必ず `types` より1つ多い**。先頭や末尾が `${` / `}` で始まる・終わる場合も
  その位置に空文字列の要素が入る (ソースの doc コメント `ts.rs:1628` に「先頭と末尾の空文字列を含む」とある)

```
`a${string}b${number}c`  →  quasis ["a","b","c"] / types [string, number]
`${string}`              →  quasis ["",""]        / types [string]
`${T}.${U}`              →  quasis ["",".",""]    / types [T, U]
```

```
quasis[0]  types[0]  quasis[1]  types[1]  quasis[2]
   "a"      string      "b"      number      "c"
```

`quasi` という名前は ESTree のテンプレートリテラル (`quasis` と `expressions`) の用語。型版では
`expressions` に当たるものが `types`。`parse_template_type` のコードとの対応は、`quasis.push(parse_template_element)`
が Head / Middle / Tail の文字列部分、`types.push(parse_ts_type)` が `${}` の中の型。while ループでは
`TemplateMiddle` に出会うたびに `quasis` が1つ増え、その後の `_` の腕で `types` が1つ増える交互の形。

デモ: `demos/oxc-step2` の demo18-25 (README の「テンプレートリテラル型 (2.3)」)。demo18 は `${}` 無しで
`TSLiteralType`、demo22 は入れ子 (入れ子の深さを数えるコードは無く、再帰がそのまま対応を取る)、
demo24・25 は閉じ忘れ (`Expected `}` but found `EOF`` / `Unexpected token`)。

#### 2.3 で見えた構造: 文字列部分は式と共有、AST は1つ、`is_ts` フラグ

**文字列部分の読み方は式と型で共通**: `parse_template_element` は `js/expression.rs:625` にあり、型の
`parse_template_type` (`ts/types.rs:768-781`) もそれを呼ぶ (grep で呼び出しを確認)。
`${}` が無い型 (`` `abc` ``) は `parse_non_array_type` の `NoSubstitutionTemplate` の腕 (`ts/types.rs:451`) が
式の `parse_template_literal` をそのまま呼び、結果を `TSLiteralType` で包むだけ。`${}` がある型だけが
別ループ (`parse_template_type`) で、中身が式ではなく型なので式側の関数は使えないが、部品は共有する。

| | 使う関数 | AST |
|---|---|---|
| `${}` なし | 式の `parse_template_literal` | `TSLiteralType(TemplateLiteral)` |
| `${}` あり | 型の `parse_template_type` (ループだけ別、`parse_template_element` と `}` の再読は共有) | `TSTemplateLiteralType` |

`parse_template_element` がやること: ①`raw` を切り出す (両端の記号を削る: Head/Middle は末尾 `${` の2文字、
NoSubstitution/Tail は末尾 `` ` `` の1文字) ②エスケープを解釈した `cooked` を作る (不正なら `None`)
③タグ無しで `cooked` が `None` ならエラー (`tagged` 引数はここで効く) ④最後の要素かの `tail` の印を付けて返す。

**AST は1つで、JS のノードと TS のノードが同じ木に混ざる** (`oxc_ast/src/ast/`):

- TS のノードが JS のノードを持つ: `TSLiteralType.literal` は `TSLiteral` (`ts.rs:226`) で、
  `BooleanLiteral` / `NumericLiteral` / `StringLiteral` / `TemplateLiteral` (`js.rs:419`) / `UnaryExpression` を持つ
- JS のノードが TS のノードを持つ: `js.rs` の `type_annotation: Option<Box<TSTypeAnnotation>>` (1225 行など)
- `--estree` の JSON は、この1つの木をそのまま書き出したもの (各ノードの `ESTree` derive が木を歩く)。
  先頭の `TS-ESTree AST:` のとおりノード名・形を typescript-estree に合わせた出力で、別の TS 用の木への
  変換ではない。`null` を `TSNullKeyword` にする話や、`readonly` の `true` / `"+"` / `"-"` はこの出力に合わせる処理

```
`abc` (型の位置)
  TSTypeAliasDeclaration   (ts.rs)
    └ TSLiteralType        (ts.rs)
        └ TemplateLiteral  (js.rs)  ← 式の `abc` と同じ型
```

**JS だけのモード = `is_ts` フラグ**: TS 専用パーサーも JS 専用パーサーも無く、**同じパーサーが `is_ts` で
TS の枝を有効にするかを切り替える**。

```rust
// lib.rs:695
is_ts: source_type.is_typescript(),      // SourceType (oxc_span) が拡張子から決める
```

- 拡張子: `js` / `mjs` / `cjs` / `jsx` / `ts` / `mts` / `cts` / `tsx` (`oxc_span/src/source_type.rs:119`)
- パーサー内の `self.is_ts` は **54か所**。例: `js/statement.rs:191` (TS の宣言に入るのは TS のときだけ)、
  `js/expression.rs:915` (式の後ろの `!`)、`:920` (`<` を型引数として試すのは TS のときだけ)
- 実測: `let x: number = 1;` / `type A = string;` / `f<T>(x);` を、拡張子だけ変えて走らせると、
  `.ts` は成功、`.js` は `let x: number` のところで「セミコロンが必要」のエラー
  (型注釈の入口に入らないので TS 構文が普通の JS としてエラーになる)。`.js` では `<` の投機もせず
  最初から比較演算子として読まれる (最後の点はコードからの推測)

ロードマップの「3つの世界」の3つ目 (JS 側への食い込み: `as` / `satisfies` / `!` / `<T>expr` が式パーサーの中に
埋まっている) の仕組みそのもので、`is_ts` の分岐がその入口になっている。JSDoc の型 (`T?` / `T!`) を
どのモードで読んでいるかは未確認。

### 2.4 読み始め: 型述語 (`x is T` / `asserts x is T` / `this is T`)

構文の意味 (知識ベース。tsc のソースでは確認していない)。**戻り値の型の位置に書く**型ガード関連の構文:

```ts
// ① 型述語: `true` を返したなら x は string
function isString(x: unknown): x is string { return typeof x === "string"; }
if (isString(v)) { v; /* string に絞られる */ }

// ② アサーション関数: 例外が飛ばずに戻ってきたなら、その後ずっと x は string
function assertIsString(x: unknown): asserts x is string { if (typeof x !== "string") throw new Error(); }
assertIsString(v); v; /* string */
function assert(cond: unknown): asserts cond { }     // `is` を付けない形もある

// ③ this 型述語: メソッドが this (呼び出した対象自身) の型を絞る
class FileEntry { isDirectory(): this is DirectoryEntry { ... } }
```

`if` で判定する ① と、呼び出した後ずっと絞る ② の違い。`asserts` と `is` は予約語ではないソフトキーワード
(`string.Foo` の話と同じ)。だから `parse_non_array_type` の `Kind::Asserts` の腕
([types.rs:492](../../../oxc-project/oxc/crates/oxc_parser/src/ts/types.rs#L492)) は、`asserts` の次が識別子で
同じ行に続くときだけ述語として読み、そうでなければ普通の型名として読む。

関数の位置 (oxc rev `1aa5ec11ce`): `parse_return_type` (:1329、呼び出しは `:56` 関数型の `=>` の後と
`:1325` `:` の後の2か所)、`parse_type_or_type_predicate` (:1334)、`parse_type_predicate_prefix` (:1355)、
`parse_asserts_type_predicate` (:800)、`parse_this_type_predicate` (:723、入口は `Kind::This` の腕)。

#### `parse_type_predicate_prefix` (types.rs:1355) — 先読みは `peek_token` の1トークンだけ、3段階の絞り込み

doc コメントの意味: 「型述語の `<識別子> is` または `this is` の前置部分をパースする。現在のトークンの後ろに、
同じ行で `is` が続いていない場合は、**何も消費せずに** `None` を返す」。

```rust
fn parse_type_predicate_prefix(&mut self) -> Option<TSTypePredicateName<'a>> {
    if !self.cur_kind().is_identifier_name() { return None; }          // ① 今のトークンが名前になれるか
    let next = self.lexer.peek_token();                                  // ② 次のトークンを覗く (消費しない)
    if next.kind() != Kind::Is || next.is_on_new_line() { return None; } // ③ `is` で、かつ同じ行か
    // ここに来たら述語確定: 名前 (this なら this) を読み、`is` を食べる
}
```

- **先読みの道具は `self.lexer.peek_token()`**。次のトークンを1つ覗くだけで位置は動かない。
  `lookahead` (checkpoint → 巻き戻し) を使う `is_start_of_mapped_type` (最大4トークン) や
  `is_next_token_colon_or_question_colon` (3トークン) より軽い。1トークンで済むのは、`x is T` の `x` と `is` の間に
  何も入らないから
- **呼び出し側 `parse_type_or_type_predicate`** (:1334): `None` なら普通の型として `parse_ts_type()` を呼ぶだけ。
  `Some` なら `x is` の後の型を `parse_ts_type()` で読み、`TSTypePredicate` で包む
- **「同じ行で」**: `asserts` の腕の「次が識別子で改行なし」と同じ考え方。`is` が次の行から始まるなら
  述語とは見なさない

**①は実際に入る**: 戻り値の型が名前になれないトークンで始まるとき (oxc で5つとも成功を確認):

```ts
declare function f(x: unknown): { a: number };        // `{`
declare function f(x: unknown): [string, number];     // `[`
declare function f(x: unknown): "a" | "b";            // 文字列リテラル
declare function f(x: unknown): (y: number) => void;  // `(`
```

**①は「早めに落とす」だけでなく、②③だけでは誤判定する場合を防ぐ**:

```ts
type is = number;
declare function f(x: unknown): [is];   // `is` という名前の型をタプルに入れた形
```

`[is]` は、今のトークンが `[`、次が `is` で、②③だけを見ると `x is` の形に見える。①で `[` が名前になれないと
先に `None` になるので、普通の型として読まれる (oxc で成功)。①が無いと `[` を名前として読もうとして壊れる。
`string` のようなキーワード型は `is_identifier_name` を通る (キーワードも名前の一種として数える) ので①では
落ちず、③で「次が `is` でない」ことで `None` になる。

| 段階 | 落とすもの | 例 |
|---|---|---|
| ① 名前になれるか | 名前になれない記号やリテラルで始まる型 | `{...}` `[...]` `"a"` `(...)` |
| ② 次を覗く | (覗くだけ) | — |
| ③ `is` かつ同じ行か | 名前の後ろに `is` が来ない普通の型 | `string` `Foo` `T[]` |

#### `new_ts_type_predicate` — 3つの構文を1種類のノードで表す (`asserts` の true / false)

`TSType::new_ts_type_predicate(span, parameter_name, asserts, type_annotation, self)` は、型述語のノード
`TSTypePredicate` を作って `TSType` として返す**自動生成のビルダー関数** (`oxc_ast/src/generated/ast_builder.rs:21920`)。
ロジックは持たず、AST のノードを組み立てるだけ。`new_ts_template_literal_type` や
`new_ts_named_tuple_member` と同じ `new_ts_*` の一種。

ノード `TSTypePredicate` (`oxc_ast/src/ast/ts.rs:1191`) の3フィールドで、3つの構文を全部表せる:

| フィールド | `x is string` | `asserts x is string` | `asserts cond` |
|---|---|---|---|
| `parameter_name` | `x` | `x` | `cond` |
| `asserts` | `false` | `true` | `true` |
| `type_annotation` | `Some(string)` | `Some(string)` | `None` (`is` が無い) |

呼び出し元との対応: `parse_type_or_type_predicate` (:1338) は `asserts` を `false` 固定、
`parse_asserts_type_predicate` (:800) は `true` で、`type_annotation` は `eat(Is)` が成功したときだけ `Some`
(`asserts cond` の形が `None` になる部分)。

つまり `x is T` も `asserts x is T` も、**別々のノード型ではなく同じ `TSTypePredicate` の `asserts` の
true / false で区別している**。

#### `parse_non_array_type` の `Kind::Asserts` の腕 (types.rs:492) — 述語か型名か、そして TS1228 の住み分け

```rust
Kind::Asserts => {
    let next = self.lexer.peek_token();                                  // `asserts` の次を覗く
    if next.kind().is_identifier_name() && !next.is_on_new_line() {
        self.bump_any();                                                  // `asserts` を食べる
        self.parse_asserts_type_predicate(asserts_start)                 // → 述語
    } else {
        self.parse_type_reference()                                       // → 普通の型名 `asserts`
    }
}
```

判定は「次のトークンが名前になれて、かつ同じ行か」の1トークン先読み (`peek_token`)。6つの入力で oxc の AST を実測:

| 入力 | oxc の AST |
|---|---|
| `(x: unknown): asserts x is string` | `TSTypePredicate(asserts=true)` |
| `(x: unknown): asserts x` | `TSTypePredicate(asserts=true)` (`is` なし。`type_annotation` は `None`) |
| `(this: unknown): asserts this is string` | `TSTypePredicate(asserts=true)` (`this` も名前になれる) |
| `type asserts = number; (): asserts` | `TSTypeReference(asserts)` (次が名前でない) |
| `type asserts = number; let a: asserts;` | `TSTypeReference(asserts)` |
| `let a: asserts x;` | `TSTypePredicate(asserts=true)` (**戻り値の位置でなくても述語として読む**) |

`asserts` という名前の型 (`type asserts = number`) が書けるのは、`asserts` がソフトキーワードだから。

**最後の行 (`let a: asserts x;`) の検査は住み分けの例**: この分岐は `parse_non_array_type` にあるので、戻り値の位置以外でも
述語として読まれる。**パーサーはエラーにしない**が、次の段階が弾く:

| | 誰が出すか | 場所 |
|---|---|---|
| tsc | チェッカー | `checker.ts:41283` (TS1228 `A type predicate is only allowed in return type position...`) |
| oxc | **`oxc_semantic`** | `checker/typescript.rs:43` `check_ts_type_predicate`、診断は `diagnostics.rs:285` `type_predicate_only_in_return_type` |

`oxc_semantic` の例 (`cargo run -p oxc_semantic --example semantic -- x.ts`) で `let a: asserts x;` を走らせると
TS(1228) が出ることを確認。`oxc_parser` の例だけでは出ない。**訂正**: 最初「oxc は tsc と違ってエラーにしない」と
説明したが、`oxc_parser` の例だけを走らせた結果で、`oxc_semantic` まで見ると tsc と同じ住み分け
(パーサーは受け入れ、後段が報告) で、後段が別の crate なだけだった。TS1338 (`infer` の位置、1.3) と同じ形。

#### `parse_this_type_predicate` (:723) / `parse_asserts_type_predicate` (:800) — トークンを消費して `new_ts_type_predicate` を呼ぶだけ

面白いところはほぼ無い。判定のロジックは呼び出し側 (`parse_non_array_type` の `This` / `Asserts` の腕) に集まっている。
強いて言えば3点:

- **`this is T` は、`this` を先に食べてから `is` を見る** (`asserts` は `peek_token` で覗いてから食べるのと逆の順序)。
  `this` は必ず `TSThisType` として作るので、食べた後でも使い回せる (述語に包むか、そのまま返すかだけが違う)
  ため、と思われる (推測)
- **`parse_this_type_predicate` は `asserts` が `false` 固定、`type_annotation` は必ず `Some`**。`this is T` には
  `is` が必ずあるので `Option` にする必要がない
- **`parse_asserts_type_predicate` の `if self.eat(Kind::Is)`** は `asserts x` (`is` 無し) の形を受け付けるための任意の `is`。
  `type_annotation` が `None` になる部分

2.4 の中身は、`parse_type_predicate_prefix` (1トークン先読みの3段階の絞り込み) と `Kind::Asserts` の腕
(述語か型名か、TS1228 の住み分け) に尽きる。

#### `oxc_semantic` とは — パーサーの次の段階 (と、oxc は「パーサー」ではなくツールチェーン)

```
ソース → [oxc_parser] → AST → [oxc_semantic] → スコープ / シンボル情報 + 追加のエラー
                                    ↓
                        linter / transformer などがそれを使う
```

`oxc_semantic` の README: 「JS / TS の AST に対する包括的な意味解析」。主な仕事はスコープ解析 (スコープの木)、
シンボル解決 (宣言した名前 = `BindingIdentifier` に `symbol_id` を付ける。ast の doc コメントの「semantic 解析の
bind ステップで初期化」はこれ)、参照の追跡 (使う側 `IdentifierReference` がどの宣言を指すか)、任意で制御フロー、
JSDoc・モジュール解析。

**エラーを出す仕事もある**: `src/checker/` (`javascript.rs` 1,362 行、`typescript.rs` 343 行) が、パーサーが出さない
構文位置のエラーを木を見て報告する。`with_check_syntax_error(true)` で有効。TS1228 (型述語は戻り値の位置だけ) や
TS1338 (`infer` の位置、1.3) はここが出す。1.3 の「ローカルに閉じる検査はパーサー、木全体が要る検査は
(tsc ではチェッカー、oxc では) `oxc_semantic`」の後段はこの crate。**型は解決しない**
(型の解決は別 crate の `oxc_type_checker`、実験的で約 2,300 行。tuple の正規化なども無い)。

**oxc はツールチェーン全体** (`crates/`、`src` の行数 / 最初に追加された日、git 履歴で確認):

| crate | 役割 | 行数 | 最初の追加 |
|---|---|---|---|
| `oxc_parser` | ソース → AST | 約2.4万 | 2023-02-11 |
| `oxc_semantic` | スコープ / シンボル + 構文位置の検査 | 約1.1万 | 2023-02-25 10:20 |
| `oxc_linter` | リンター (oxlint) | 約45.7万 | 2023-02-25 10:48 (`linter prototype`) |
| `oxc_minifier` | ミニファイア | 約4.8万 | 2023-03-29 |
| `oxc_formatter` | フォーマッタ | 約5.3万 | 2023-05-07 (`oxc_printer` から改名) |
| `oxc_transformer` | 変換 (TS → JS など) | 約3.1万 | 2023-09-16 |
| `oxc_codegen` | AST → ソース出力 | 約0.8万 | 2023-10-12 |
| `oxc_type_checker` | 型チェッカー (実験的) | 約0.2万 | 2026-07-01 |

- **`oxc_semantic` は「oxlint が出てから追加」ではない**: リポジトリ開始 (2023-02-09) の2週間後、リンターの試作の
  **28分前**に同じ作者が作っている。最初の利用者はリンター (下の根拠)。書いた人の意図までは履歴からは断定できない
- **リンター試作の最初のコミット (`c86cca37a8`) の根拠** (`git show` で確認): `oxc_linter` の `Cargo.toml` に最初から
  `oxc_semantic` への依存があり、コードが `use oxc_semantic::SemanticBuilder;` と
  `SemanticBuilder::new().build(program)` を呼んでいる。さらに `crates/{oxc_semantic/src => oxc_linter}/Cargo.toml`
  というリネームを含み、`oxc_semantic` の中にあったファイルを `oxc_linter` の雛形として流用している。
  semantic の最初のコミット (`5f7a756229`) は依存が `oxc_ast` だけの単独 crate だった
- **`checker/` (構文位置の検査) は最初リンターにあった**: 2023-04-10 の
  `refactor(linter,semantic): move syntax check from linter to semantic (#272)` で semantic に移された。
  TS1228 のような検査が semantic にあるのはこの移動の結果。**PR #272 の本文 (`gh pr view 272`)**:
  「構文チェッカーは意味解析の一部で、意味エラーのためだけにユーザーがリンターを足すのは筋が通らない」
  (原文: `Syntax checker is part of semantic analyzer, it doesn't make sense for the user to add a linter
  just for semantic errors`)。semantic を作った理由ではなく、**構文チェックを移した理由**
- **PR #46・#48 の本文は空 (意図は書かれていない)**: `oxc_semantic` を作った PR #46 (2023-02-25 02:29Z、3分でマージ) と
  `oxc_linter` の試作の PR #48 (同日 08:48Z、8分でマージ) は、作者が出した本文が空の PR
- **意図は issue・discussion で確認できた (`gh` で読んだ)**: semantic は**リンターの設計の一部として作られた**。
  - **discussion #38「RFC: Linter」** (2023-02-23、semantic ができる2日前、作者 Boshen): リンターの実装方針。
    優先事項は「performance, simplicity and contribution friendly」。走査のアルゴリズムに「AST ごとに visiting pass を
    して、親を指す木 (indextree、ノードは untyped) を作り、**semantic analysis としてスコープ木・シンボルテーブル・
    制御フローグラフを作る**。そのあと lint ルールごとに `par_iter`」とある。ルールの trait は
    `fn run(&self, node: &AstNode, ctx: &Semantic)` で、「`Semantic` 構造体は AST に紐づくすべて (スコープ、シンボル、
    CFG、trivia など) を包む」という節もある
  - **issue #41「Umbrella: Linter MVP」** (2023-02-24): 「決定は discussion #38 に基づく」と明記し、作業リストに
    **PR #46 (semantic builder)** が入っている
  - PR #46 のタイトルの「untyped ast tree creation」は、RFC の「親を指す木、ノードは untyped」と一致
  - 時系列: 02-23 RFC → 02-24 Umbrella issue (PR #46 を含む) → 02-25 PR #46 (semantic) と #48 (linter 試作) をマージ
- **oxlint のリリース時期** (リポジトリのタグと `gh api repos/oxc-project/oxc/releases` で確認): 2023-02 に構想・試作 →
  `oxlint_v0.0.3` (2023-06-27)・`v0.0.4` (06-28)・`v0.0.5` / `v0.0.6` (07-01) が最初期の `oxlint_v*` タグ →
  **`oxlint_v1.0.0` は 2025-06-10** (構想から約2年4か月)。「2025年リリース」は v1.0.0 のこと。
  `oxlint_v0.0.3` より前のタグ (v0.0.1・v0.0.2 など) が `oxlint` という名前だったかは未確認。一般公開の告知の日は
  リポジトリの外 (ブログ・SNS) の話なので未確認
- `oxc_linter` が桁違いに大きい (45万行) のは、ルールを1つずつ実装しているから、というのは推測

LT の骨格 (地図の図) に `oxc_semantic` の行を足すと、冒頭の問い「oxc は TS をどこまで検査している?」にも答えられる。

### 3.3 見直し: `f<T>(x);` を通しで追う (demos/oxc-step3 の demo1 のトレース)

oxc の関数呼び出し順を採ったトレース (`demos/oxc-step3/`、README にトレースの読み方と12パターンの一覧) を、
`f<T>(x);` (demo1) の先頭から追う。**`types.rs` に入るのは、トレースの12行目 (`parse_type_arguments_in_expression`)**。
それまでは全部 `js/` のファイル。

**トレースに出ない一番上**: `parse_program` (`lib.rs:817`)。仕込みが `js/` `ts/` `jsx/` だけで `lib.rs` は入れて
いないため。実際の入口:

```rust
// lib.rs:817 parse_program
self.token = self.lexer.first_token();            // ① 最初のトークン (`f` = Ident @0)
let hashbang = self.parse_hashbang();             // ② `#!` 行があれば読む      ← トレースの1行目
self.ctx |= Context::TopLevel;
let (directives, mut statements) = self.parse_directives_and_statements(false);  // ③ 本体 ← トレースの2行目
```

- `parse_hashbang` (`js/statement.rs:17`): 現在のトークンが `HashbangComment` かを見るだけ。`f` なので `None`
- `parse_directives_and_statements` (`js/statement.rs:33`): 文を1つずつ読む `while` ループ。ループの中身は大きく3つ:
  ①(unambiguous モードのときだけ) 文ごとの `checkpoint` ②`parse_statement_list_item` で1文読む ③最初の文が文字列リテラル
  だけならディレクティブ (`"use strict"` など) として扱う。**トレースの `** [checkpoint] at 0` はこの文ごとの
  定型の checkpoint** で、`f<T>(x)` の曖昧性とは関係ない (step0 の README にも同じ注意がある)

**`f<T>(x);` の先頭12行と、どのファイルか**:

| 行 | 関数 | ファイル | 何をしているか |
|---|---|---|---|
| 1 | `parse_directives_and_statements` | `js/statement.rs:33` | 文を1つずつ読むループ |
| 3 | `parse_statement_list_item` | `js/statement.rs:131` | 文の種類を分岐 |
| 4 | `parse_expression_or_labeled_statement` | `js/statement.rs:251` | 式の文 (`f<T>(x);`) と判断 |
| 5 | `parse_assignment_expression_or_higher` | `js/expression.rs:1479` | 式の入口 |
| 6-8 | `try_parse_parenthesized_arrow_function_expression` など | `js/arrow.rs` | アロー関数かを先に試す (`f` で始まるので違う) |
| 9 | `parse_binary_expression_or_higher` | `js/expression.rs:1304` | 二項演算の優先順位のはしご |
| 10 | `parse_lhs_expression_or_higher` | `js/expression.rs:748` | 左辺式 (代入の左側になれる式) |
| 11 | `parse_primary_expression` | `js/expression.rs:228` | `f` を読む |
| 11 | `parse_member_expression_rest` | `js/expression.rs:859` | `f` の後ろの `.` `[` `<` `(` を見る。**`<` (LAngle @1) が来た** |
| **12** | **`parse_type_arguments_in_expression`** | **`ts/types.rs:914`** | **ここで初めて `types.rs`** |

- **入口は `js/expression.rs:859` の `parse_member_expression_rest`**。`f` を読み終えた直後の次のトークンが `<` だったので、
  「型引数かもしれない」と TS の側 (`types.rs`) を呼ぶ
- `types.rs` に入るときは、必ず**式のはしごの一番下 (`parse_member_expression_rest`) から**。式パーサーが「ここは TS かも
  しれない」と見て、`types.rs` の関数を呼ぶ。ロードマップの「3つの世界」の3つ目 (JS 側への食い込み: `foo<T>()` などが
  式パーサーの中に埋まっている) の、実際の入口

#### `parse_member_expression_rest` (`js/expression.rs:859`) — JS と TS の境界は、後置を読む `loop` の中の `match` の腕

`f<T>(x);` の JS 側と TS 側の**境界**は、この関数。左辺の式 (`f`) の後ろに続く後置を、`loop` で1段ずつ積んでいく:

```rust
let mut lhs = lhs;                          // 最初は `f`
loop {
    match self.cur_kind() {                 // 今のトークンで振り分け
        Kind::Dot                 => { lhs = 静的メンバー }                 // `a.b`
        Kind::QuestionDot         => { lhs = オプショナルチェーン }          // `a?.b`
        Kind::LBrack              => { lhs = 計算メンバー }                 // `a[0]`
        テンプレートの開始         => { lhs = タグ付きテンプレート }          // a`...`
        Kind::Bang if self.is_ts  => { lhs = TSNonNullExpression }          // `a!`     ← TS
        Kind::LAngle | Kind::ShiftLeft if self.is_ts => { ... }             // `f<T>`   ← TS (今回の入口)
        _ => return lhs,
    }
}
```

`a.b[0]!` なら `a` → `a.b` → `a.b[0]` → `a.b[0]!` の順に包み直す。**後置演算子を左から右へ積むループ**。

**`<` の腕**:

```rust
Kind::LAngle | Kind::ShiftLeft if self.is_ts => {
    if let Some(arguments) = self.parse_type_arguments_in_expression() {     // ts/types.rs:914
        lhs = Expression::new_ts_instantiation_expression(self.end_span(lhs_start), lhs, arguments, self);
    } else {
        self.lexer.rewrite_last_collected_token(self.token);   // `<<` を割ったのを書き戻す (Session 0 の話)
        return lhs;                                            // `f` だけ返す。`<` は消費されていない
    }
}
```

1. **後置として `<` が来たから**: `f` の直後の `<` は、JS では比較演算子、TS では型引数の始まりかもしれない。
   この場所は式の後置を読む場所なので、`.` `[` `!` と並べて `<` も選択肢の1つ
2. **`if self.is_ts`**: `.js` ではこの腕に入らず `_ => return lhs` に落ちる。`<` は比較演算子として、上の
   `parse_binary_expression_rest` に任される (`is_ts` の54か所のうちの1つ)
3. **成功したら `TSInstantiationExpression`** で `lhs` を包む。**`f<T>(x)` の `(x)` はここでは読まれない**。次のループ
   (実際は `parse_call_expression_rest`) が `(` を見て呼び出しとして読む
4. **失敗したら `return lhs`**: 呼び出し元 (`parse_binary_expression_rest`) が、消費されていない `<` を比較として読み直す
   (demo2 `a < b > c` の流れ)

**ロードマップの Session 5 との関係**: `Kind::Bang if self.is_ts` (5.2: `a!`) と `Kind::LAngle | Kind::ShiftLeft if self.is_ts`
(5.4: `TSInstantiationExpression`) は、**この1つの `match` の2つの腕**。「TS が JS の式パーサーに埋まっている」の、
一番わかりやすい形は、TS 固有の後置 (`!` と `<`) と JS の後置 (`.` `[` `?.` テンプレート) が同じ `loop` の中に並んでいること。

### 4.1 読み始め: enum / type alias / interface のパーサーはどこから呼ばれるか

構文の処理そのものは素直なので読まず、**呼び出し元** (`grep` で確認) だけを整理した。3つのパーサー
(`parse_ts_enum_declaration` `ts/statement.rs:21` / `parse_ts_type_alias_declaration` `:128` /
`parse_ts_interface_declaration` `:224`) は、**`parse_declaration` (`:640`) の `match` の腕**から呼ばれる
(`Kind::Type` → `:713`、`Kind::Enum` → `:714`、`Kind::Interface` → `:717`)。そこへ行く道:

```
parse_statement_list_item (js/statement.rs) ─ is_ts && at_start_of_ts_declaration() ─→ parse_ts_declaration_statement (ts/statement.rs:612)
                                                                                          └→ parse_declaration (:640) ─ match ─┬ Kind::Type      → type alias
js/module.rs:657 (`export` の後ろ) ──────────────────────────────────────────────→ parse_declaration                            ├ Kind::Enum      → enum
                                                                                                                                └ Kind::Interface → interface
```

| 経路 | 呼び出し元 | どんなとき |
|---|---|---|
| ① 普通の文 | `js/statement.rs:191-193` (キーワードの腕: `Abstract` `Accessor` `Static` `Readonly` `Global` など。条件 `self.is_ts && self.at_start_of_ts_declaration()`) → `parse_ts_declaration_statement` → `parse_declaration` (`:629`) | `type A = ...;` / `interface I {}` / `declare ...` など |
| ② `async` で始まる文 | `js/statement.rs:910-911` (`async function` でなければ、同じ条件で `parse_ts_declaration_statement`) | `async` の後ろが TS の宣言のとき (周りのコードからの読み。走らせていない) |
| ③ `export` の後ろ | `js/module.rs:657` が `parse_declaration` を直接呼ぶ | `export interface I {}` / `export type A = ...` / `export enum E {}` |
| ④ `const enum` | `js/statement.rs:880-885` `parse_const_statement` が `parse_ts_enum_declaration` を直接呼ぶ (`const` を食べた後 `self.is_ts && self.at(Kind::Enum)`) | `const enum E {}`。**普通の `enum E {}` は ① → `parse_declaration` の `Kind::Enum` の腕** |
| ⑤ `export default interface` | `js/module.rs:752-758` が `parse_ts_interface_declaration` を直接呼ぶ | `export default interface I {}` |

訂正: 最初 ③ を「`enum` だけ別で、予約語だから先読みが要らない」と説明したが誤り。`js/statement.rs:880-885` は
コメントに「Parse const declaration or `const enum`」とある通り **`const enum` の処理**だった。

**境界の対**: 式と文で、TS への入口が対になっている。

- 式: 式のはしごの一番下 `parse_member_expression_rest` の `match` の腕 (`!` / `<`) から `types.rs` へ
- 文: 文の入口 `parse_statement_list_item` の `is_ts && at_start_of_ts_declaration()` から `ts/statement.rs` へ

トレースでは demo3・demo7・demo8 に `parse_statement_list_item → at_start_of_ts_declaration →
parse_ts_declaration_statement → parse_declaration → parse_ts_type_alias_declaration` の順が出ている
(`demos/oxc-step3`)。

#### `at_start_of_ts_declaration` (`ts/statement.rs:863`) の高速経路 — tsc には無い、oxc 独自の最適化と「2重管理」

「ここから TS の宣言が始まるか」の判定。文の入口 (`js/statement.rs` の `is_ts && at_start_of_ts_declaration()`) が呼ぶ。
コメントの訳:

```
// 高速経路: キーワード1個で始まる宣言の形は、`cur_kind` (今のトークンの種類) と、多くても1個の先読みトークン
// (peek) で決まる。だから、ここで解決して、完全な `lookahead` (checkpoint + 投機的な部分パース + rewind)
// のコストを払わずに済ませる。各腕は、`at_start_of_ts_declaration_worker` の対応する腕と、そのまま一致している。
```

```rust
pub(crate) fn at_start_of_ts_declaration(&mut self) -> bool {
    match self.cur_kind() {
        Kind::Var | Kind::Let | Kind::Const | Kind::Function | Kind::Class | Kind::Enum => true,
        Kind::Interface | Kind::Type => { let next = self.lexer.peek_token(); next.kind().is_binding_identifier() && !next.is_on_new_line() }
        Kind::Module | Kind::Namespace => { /* 次が識別子か文字列で、同じ行 */ }
        Kind::Global => { /* 次が Ident / { / export */ }
        Kind::Import => { /* 次が 文字列 / * / { / 識別子 */ }
        _ => self.lookahead(Self::at_start_of_ts_declaration_worker),   // 複数トークンの形は本物の lookahead に任せる
    }
}
```

**tsc・ts-go にはこの高速経路は無い** (tsc 5.9 の `parser.ts:7250` `isStartOfDeclaration` は `lookAhead(isDeclaration)` のみ、
ts-go の `parser.go:6123` も `lookAhead(scanStartOfDeclaration)` のみ)。oxc の遅い経路 (`worker`) が tsc の `isDeclaration`
(`parser.ts:7155`、`while (true) { switch (token()) ... }`) の移植で、**高速経路はその前に oxc が足したもの**。
oxc が必要とした理由は、コメントの「`lookahead` のコスト」からの推測 (tsc の `lookAhead` はスキャナーの状態だけ
保存して戻すので軽く、oxc の `checkpoint` (パーサーとレキサーの状態、エラー数など) より軽いのかもしれないが、測っていない)。

**「各腕が `worker` の対応する腕と一致している」の意味**: 同じ判定を2か所に書き写していて、その2つが食い違わないことの
保証。書き方が違うだけで判定は同じ:

| トークン | 高速経路の腕 | `worker` の腕 |
|---|---|---|
| `type` / `interface` | `peek_token()` で次を覗く (消費しない) | `bump_any()` で進めてから見る (`lookahead` の中なので後で巻き戻る) |
| `module` / `namespace` / `global` / `import` | 同じ条件を `peek_token()` で | 同じ条件を `bump_any()` の後で |
| `var` `let` `const` `function` `class` `enum` | すぐ `true` | すぐ `true` |

片方だけ直して食い違うと、同じ入力でも高速経路に落ちるか `worker` を通るかで結果が変わる (バグになる)。コメントの
「exactly」は修正する人への注意。

高速経路が持たない腕 (`declare` `abstract` `export` `async` `static` `readonly` などの複数トークンの形、例:
`declare const x` / `export type T` / `abstract class C`) は `_` の腕で `worker` に任せる。**そのため `worker` には
`var` `let` などの高速経路と重複する腕も残っている**: `declare const x` を `worker` が読むとき、`declare` を食べた後の
`const` を `worker` 自身の `Kind::Const` の腕で判定するから。

**`worker` という名前**: 「実際の作業をする側の関数」につける名前の習慣。入口の関数が前処理 (ここでは高速経路) や
`lookahead` / `checkpoint` で包むことをして、裏で本体の処理をする関数を `_worker` と呼ぶ。`lookahead` の中で走るので、
トークンを進めても最後に自動で巻き戻る。トレースにも出る: `is_parenthesized_arrow_function_expression` (入口) →
`checkpoint` → `is_parenthesized_arrow_function_expression_worker` (作業) → `rewind` (demo9・demo10)。
`_worker` で終わる関数は「投機や先読みの中で走る本体」の目印: `at_start_of_ts_declaration_worker` (`ts/statement.rs:899`)、
`is_parenthesized_arrow_function_expression_worker` (`js/arrow.rs:77`)、`is_un_parenthesized_async_arrow_function_worker`
(`js/arrow.rs:217`)。LT の「読み方のコツ」の目印 (`is_start_of_*` = 先読み / `checkpoint` + `rewind` = 投機 /
`re_lex_*` = 再読) に **`*_worker` = 投機や先読みの中で走る本体** を足せる。

**`_` の腕のコメント** (`// Multi-token modifier chains ... need real lookahead, as do non-declaration tokens.`) の訳:
「複数トークンの修飾子の連なり (`declare const x`、`abstract class C`、`export type T`、`async function f`、`static …`) と、
`export = …` / `export default …` は、本物の lookahead が要る。宣言ではないトークンも同様」。

| ケース | 例 | なぜ1トークンで決まらないか |
|---|---|---|
| 修飾子の連なり | `declare const x` / `abstract class C` / `async function f` | 最初のトークンだけでは決まらず、後ろを見る。連なりの長さも決まっていない (`declare abstract class` など) |
| `export` の形 | `export type T` / `export = …` / `export default …` | `export` の後ろに何が来るかで分かれる |
| 宣言ではないトークン | `type = 1;` (`type` を変数名として使う式の文) など | 高速経路の腕のどれにも当たらないので `worker` が最後に `_ => return false` を返す |

**注意 (訂正)**: 最初「`foo();` の `foo` のような普通の式も `_` に落ちる」と説明したが誤り。呼び出し元の
`parse_statement_list_item` (`js/statement.rs:172-193`) が `at_start_of_ts_declaration` を呼ぶのは、
`Interface` `Type` `Module` `Namespace` `Declare` `Enum` `Private` `Protected` `Public` `Abstract` `Accessor` `Static`
`Readonly` `Global` の14種類のトークン (かつ `is_ts`) のときだけで、普通の式の文は `_ => parse_expression_or_labeled_statement()`
に落ちて、この関数は呼ばれない。「宣言ではないトークン」が `_` に来るのは、**このリストのトークンなのに宣言ではなかった
とき** (`type` などのソフトキーワードを変数名として使った式の文)。

**同じ種類の話**: `is_start_of_type` (型の FIRST 集合を別の場所で列挙し直す、1.4)、`parse_type_arguments_in_expression` の
「`<` でなければ checkpoint の前に抜ける」早期リターン (3.3)、`parse_type_predicate_prefix` の `peek_token`
(1トークン先読みは `lookahead` より軽い、2.4)。「oxc は速さのために、tsc の移植の上に1段の近道や2重管理を足している」
というブログのトリビア候補 (下の表に追加)。

### 5.1 `as` / `satisfies` (`js/expression.rs:1322` `parse_binary_expression_rest`) — Pratt の腕と、2026-06 に足された「消せない」判定

**`parse_binary_expression_rest` は oxc の Pratt パーサーの本体** (コメントに matklad の Pratt 解説記事へのリンク)。
`rest` は「左辺を読んだ後の続き」の意味で、`parse_member_expression_rest` (後置の続き) と同じ命名。
呼び出し元 `parse_binary_expression_or_higher` (:1304) が左辺を1つ読んで (`#x in o` の特別扱いもここ)、この関数に渡す。

```rust
loop {
    let kind = self.re_lex_right_angle();                                 // `>=` `>>` の結合もここ
    let Some(left_precedence) = kind_to_precedence(kind) else { break };  // 二項演算子でなければ終了
    let stop = if 右結合 { left < min } else { left <= min };              // 優先順位が低ければ止まる
    if stop { break; }
    if matches!(kind, Kind::As | Kind::Satisfies) { ...; continue; }      // ★ as / satisfies
    self.bump_any();                                                        // 演算子を食べる
    let rhs = self.parse_binary_expression_or_higher(left_precedence);     // 右辺を再帰で読む
    lhs = new_binary_expression(lhs, op, rhs);                              // lhs を包み直す
}
```

**自作 (`src/js/parser.rs:291` `parse_binary_expression(min_bp)`) との対比**: 考え方は同じ (優先順位を引数で持ち回る再帰)。
自作は `binary_binding_power(kind)` が返す結合力の組 `(l_bp, r_bp)` で左右結合を表し、oxc は `Precedence` の値1つ +
`is_right_associative()` で `<` と `<=` を切り替える。oxc は二項演算子の表 (`kind_to_precedence`) に `as` / `satisfies` (TS) や
`in` も載せている。**式は Pratt、型は階層を関数で固定した再帰下降** (演算子が `|` `&` 程度で少ない) という対比が、
1.1 のメモの「式パーサーとの対比が一番の学び」の実物。

**`as` / `satisfies` の腕**: 右辺を再帰で読む代わりに、`parse_ts_type()` を呼ぶ。**`types.rs` に入る3つ目の入口**
(1つ目: 後置 `!` / `<` = `parse_member_expression_rest`、2つ目: 文の `at_start_of_ts_declaration`)。

```rust
if matches!(kind, Kind::As | Kind::Satisfies) {
    if self.cur_token().is_on_new_line() { break; }          // 改行があれば止める (ASI: `var x = foo⏎as (Bar)`)
    self.bump_any();
    let type_annotation = self.parse_ts_type();              // ★ 右側は式ではなく型
    lhs = new_ts_as_expression / new_ts_satisfies_expression(span, lhs, type_annotation);
    // ... 「消せない」判定 (下) ...
    continue;
}
```

- `.js` でも構文としては読む。`if !self.is_ts { self.error(as_in_ts(span)) }` でエラーを出すだけで、`parse_member_expression_rest` の
  `!` / `<` (`if self.is_ts` で腕自体を分ける) とは作りが違う
- 改行の扱い (`is_on_new_line()` で `break`) は tsc 5.9 と同じ (コメントも同じ)

**「消せない」判定 (`last_operand_precedence`)**: 2026-06 に足された新しい挙動。

```rust
let mut last_operand_precedence: Option<Precedence> = None;   // 今の左辺を作った二項演算子の優先順位 (最初は None)
...
// as / satisfies の腕の最後:
if let Some(last_precedence) = last_operand_precedence
    && let Some(next_precedence) = kind_to_precedence(self.re_lex_right_angle())
    && next_precedence > last_precedence
{ break; }
```

コメントの訳: 「`a ## b as T` または `a ## b satisfies T` (`##` は何かの二項演算子) のとき、`##` より優先順位が高い演算子が
後ろに続いたら、そこでパースを止める。続けてしまうと、`as` や `satisfies` を消したときに式の意味が変わり、消せなくなるから」。
変数のコメントの訳: 「今の左辺のオペランドを作った演算子の優先順位。消せない `as` / `satisfies` を見つけるために使う。二項・論理演算子を
1つも消費していない間は `None` (TypeScript では最初のオペランドは単項式のパースから来るので二項式にならない)」。

```ts
1 + 1 as number * 2      // 続けて読むと ((1 + 1) as number) * 2 = 4。`as number` を消すと 1 + 1 * 2 = 3 で意味が変わる
```

| 式 | 結果 |
|---|---|
| `1 as number * 2` | OK |
| `1 * 1 as number + 2` | OK (`+` は `*` より強くない) |
| `1 + 1 as number * 2` | **エラー** (`*` が `+` より強い。`* 2` が宙に浮いてパースエラーになる) |
| `1 + 1 as any as number * 2` | **エラー** (連鎖では追跡している優先順位を**更新しない**ので、元の `+` と比べ続ける) |
| `1 >> 1 as number + 2` | **エラー** |
| `(1 + 1 as number) * 2` | OK |
| `1 + 1 as number === 2` | OK (`===` は `as` より低い) |

比べる相手は「`as` の直前の二項演算子 (`##`)」であって、`as` 自体ではない。`as` / `satisfies` の腕は `last_operand_precedence` を
書き換えず `continue` するので連鎖でも元の演算子が残る (書き換えるのは普通の二項演算子の腕だけ)。

**どういう場合にバグっていたか (`demos/oxc-step5`、実測)**: TS のパース自体は正しく、問題は「型を空白に置き換えるだけの素朴な除去」
と `erasableSyntaxOnly` の間にあった。`1 + 1 as number / 2` を tsc は `(1 + 1) / 2` (値1) と読むが、`as number` を空白にすると
`1 + 1 / 2` (値1.5) になり、tsc 6.0.3 の `erasableSyntaxOnly` はそれをエラーにしなかった。10ケースの実測で、
**「空白で消すと値が食い違う」5件 (`+` の後に `/` `*`、`>>` の後に `+`、連鎖、`satisfies`) と「oxc がエラーにする」5件が完全に一致**し、
「後ろの演算子が直前と同じか弱い」ケース (`1 * 1 as number + 2`、`10 - 2 as number - 3`) は消しても値が変わらず oxc も通す。

**なぜ `/ 2` が `(1 + 1)` 全体にかかるのか (`1 + 1 as number / 2`)**: ① `as` は `+` より弱いので、`1 + 1` を読むとき `+` の右辺は
`as` の手前で止まり、`1 + 1` が先に1つのかたまりになる。② 普通の弱い演算子なら右辺は**式**なので、右辺を読む再帰が後ろの
`/ 2` を飲み込む (`1 + 1 < 3 / 2` は `(1+1) < (3/2)`)。でも **`as` の右辺は式ではなく型** (`parse_ts_type()`) で、型のパーサーは
`/` を読めず `number` で止まる。`/ 2` を受け止める再帰が無いので、外側のループが `(1 + 1) as number` **全体を左辺として** `/` を
消費する (木は `/ (as (+ 1 1) number) 2`)。③ `(1 + 1) / 2` の括弧は**出力のときに** tsc が木の形を保つために補うもので、
パースで付くのではない。素朴な空白除去は木ではなく文字列を見るので括弧が無いまま `1 + 1 / 2` になり、別の木になる。
「優先順位が低い」だけだと `/ 2` が `number` の側に入りそうに見えるが、**右辺が型なので入れない**のが核心
(コードの動きから整理したもので、tsc の実際の木を出して確かめたわけではない)。

**修正は breaking change (パースエラーにする)。互換性は承知の上**: 修正後の ts-go 7.x と oxc では `1 + 1 as number / 2` が
**パースエラー** (`erasableSyntaxOnly` を付けなくてもエラー。oxc は普通の `.ts` としてエラーになることを実測)。
tsc 5.9 / 6.0.3 (JS 版) は変わらずエラーにならない。issue #63527 の議論 (原文の要点):

- **Ryan Cavanaugh** (2026-06-02): 「We shipped *what* precedence??」と驚き、「`1 || 2 ?? 3` がエラーなのと同じ扱いにできないか」
  「Strada (旧 JS 版) で **top1000 に対して試して**、**7.0 でここを break するだけ**でいいか確かめる価値がある」
  「**ランタイムの動作 (emit) を変えたくない**ので、単なる優先順位の変更ではなく**エラーにする**必要がある」
- **Anders Hejlsberg** (2026-06-03): 「`xxx as T` の後ろに意味のある形で続けられるのは、関係演算子と同じか低い演算子だけ。
  高い演算子が続くと `as number` を消せなくなるので、その演算子を式の一部とみなすのは筋が通らない」
  「演算子を見つけたらパースを止める PR を出す。**現実のコードで壊れるものがあったら、とても驚く**」
- **ts-go#4192 のコメント**: Jake Bailey が「top800 (人気リポジトリ800個) を回すのを待つのか」と質問、Anders は
  「ts-go 側のコードベースで試したい」と答え、`@typescript-bot test this` を実行 (CI の結果までは読んでいない)

つまり、優先順位を変えて意味を変える (既存コードの emit が変わって危ない) より、**エラーにして書き手に括弧を付けさせる**ほうを
選んだ意図的な breaking change で、影響は小さい見込み (実際のコードで `a + b as T * c` と書く人はほとんどいない)。
TypeScript は 6.0 まで (JS 版) と 7.0 から (ts-go) でメジャーバージョンが分かれていて、「in 7.0」は **メジャーの切り替わりで
こういう小さな breaking change を入れる**意味だと思われる (私の読みで、公式方針の記述は確認していない)。oxc は ts-go に合わせて
エラーにしているので、**tsc 6.0 では通っていたコードが oxc ではエラー**になりうる。

**実装での表現: 変数の3か所 (宣言・代入・判定)** (`js/expression.rs`)。`1 + 1 as number / 2` で値を追う:

| 行 | コード | 役割 |
|---|---|---|
| `:1336` | `let mut last_operand_precedence: Option<Precedence> = None;` | 宣言。ループに入る前は `None` |
| `:1441` | `last_operand_precedence = Some(left_precedence);` | 代入。**普通の二項演算子の腕の最後**で、その演算子の優先順位を入れる |
| `:1384-1389` | `if let Some(last) = last_operand_precedence && let Some(next) = kind_to_precedence(self.re_lex_right_angle()) && next > last { break; }` | 判定。**`as` / `satisfies` の腕の最後**で、後ろの演算子と比べる |

| 周 | 今のトークン | 何が起きるか | `lhs` | `last_operand_precedence` |
|---|---|---|---|---|
| 0 | — | 宣言 | `1` | `None` |
| 1 | `+` | 普通の二項演算子の腕。右辺 `1` を読んで包み、腕の最後で代入 | `1 + 1` | `Some(Add)` |
| 2 | `as` | `as` の腕。`number` (型) を読んで包む → 判定へ | `(1 + 1) as number` | `Some(Add)` のまま (`as` の腕は書き換えない) |
| 判定 | `/` | `Multiply > Add` が真 → `break` | — | — |

`break` でループを抜けて `(1 + 1) as number` を返し、`/ 2` は読まれずに残る。呼び出し元をたどって文の読み込み
(`parse_expression_statement`) に戻ると、文の終わり (`;`) のはずなのに `/` が来るので `Expected a semicolon or an implicit
semicolon after a statement, but found ...` になる (oxc の実測で、キャレットが `/` を指す)。**エラーを出す専用のコードは無く、
「`/ 2` を読まずに返す」だけで自然に出る** (oxc#22986 の本文「後ろの演算子を宙に浮かせてパースエラーとして表に出す」)。

判定の3部品: ① `last_operand_precedence` が `None` (`as` の前に二項演算子が無い、`1 as number * 2`) なら判定しない
② `re_lex_right_angle()` を通すのは `>=` `>>` を結合して見るため (二項演算子でなければ `None` で判定しない)
③ 「以上」ではなく**「より強い」**なので、同じ強さ (`10 - 2 as number - 3`) は通る。連鎖 `1 + 1 as any as number * 2` は
`as` の腕が書き換えないので、何回 `as` を挟んでも元の `+` と比べ続けて止まる。

**直し方は「括弧を付けて意図を明示させる」** (`demos/oxc-step5` の demo11-13、実測)。元の `1 + 1 as number / 2` は
`(1 + 1) / 2` と `1 + (1 / 2)` のどちらの意味か曖昧なので、パーサーが黙って1つに決めるのをやめ、エラーにして書き手に選ばせる:

| 書きたい意味 | 書き方 | tsc の出力 (値) | 空白で消した値 | oxc |
|---|---|---|---|---|
| `(1 + 1) / 2` (=1) | `(1 + 1) as number / 2` (demo11) | `(1 + 1) / 2` (1) | 1 一致 | OK |
| 同上 | `(1 + 1 as number) / 2` (demo12) | `(1 + 1) / 2` (1) | 1 一致 | OK |
| `1 + (1 / 2)` (=1.5) | `1 + (1 as number) / 2` (demo13) | `1 + 1 / 2` (1.5) | 1.5 一致 | OK |

3通りとも空白で消しても tsc の意味と一致し、oxc も通る。`(1 + 1) as number / 2` (demo11) が通るのは、`(1 + 1)` が括弧付きの
1つの式として単項の位置で読まれ、ループの中で二項演算子を消費していないので `last_operand_precedence` が `None` のまま判定されないから。
既存コードは括弧を付ければ直り、意味は「今まで tsc が出力していた JS と同じ」に保てる (自動で直すことも機械的にできそう、と思う)。

**TS チームが選ばなかった案** (議論の原文の要点から。私の読み):

| 案 | 扱い |
|---|---|
| `1 + 1 as (number / 2)` と読む (`as` の右辺を広げて型のパースエラーにする) | 優先順位そのものを変える案 |
| `1 \|\| 2 ?? 3` のように、混ぜるとエラーにする (`??` と同じ扱い) | **こちらに近い形で採用**。`||` / `&&` と `??` を括弧なしで混ぜるとエラーになるのと同じ発想を `as` にも入れた |
| 優先順位を変える | 既存コードのランタイム動作 (emit) が変わるので選ばなかった |

「曖昧なものをパーサーが黙って1つに決めるのをやめて、書き手に意図を書かせる」方向の変更。

**由来 (実物で確認)**:

| 版 | `as` / `satisfies` の腕 |
|---|---|
| tsc 5.9 (classic、`parser.ts` `parseBinaryExpressionRest`) | `as` を読んで `leftOperand` を包み直すだけ。後続の演算子の優先順位を見ない。改行のコメントも oxc と同じ |
| **ts-go 7.1-dev** (`parser.go:4642`) | `lastOperand` を追跡し、後続が `lastPrecedence` より強ければ `break`。コメントに issue #63527 の URL |
| oxc | `last_operand_precedence` で同じ考え方 (`None` は ts-go の `OperatorPrecedenceHighest`: 二項式でなければ何も止めない) |

**経緯 (`gh` で確認)**:

1. **microsoft/TypeScript#63527** (2026-06-02、robpalme): `erasableSyntaxOnly` が `console.log(1 + 1 as number / 2)` でエラーを出さない。
   型を空白に置き換えるだけの素朴な除去 (ts-blank-space など) だと `console.log(1 + 1           / 2)` = `1 + (1/2)` になり、
   TS 自身の出力 `(1 + 1) / 2` と意味が食い違う。ts-blank-space・SWC・Amaro は先に修正済み
2. **議論**: Ryan Cavanaugh は「`as` は2文字だから括弧を差し込める、では不十分。空白化して文字位置が保たれる構文だけを許す」。
   Anders Hejlsberg (2026-06-03): 「`as` / `satisfies` は関係演算子と同じ優先順位で、後ろに続けられるのは同じか低い演算子だけ。
   高い優先順位の演算子が続くなら、それを式の一部として解釈するのをやめる」。コメントでは「`as` の優先順位がこんなに低いとは
   思わなかった」「どこにも書いていない」と驚く人が複数いた
3. **microsoft/typescript-go#4192** (Anders、2026-06-03 18:49 → 06-04 21:14 マージ): 修正
4. **oxc-project/oxc#22986** (Boshen、2026-06-05 12:38 → 13:15 マージ。ts-go のマージから約16時間後): 本文が「Port of microsoft/typescript-go#4192」。
   `parse_binary_expression_rest` と `tasks/coverage/misc/` のテスト4つ (`ts-unerasable-as.ts` など) とスナップショット
5. **microsoft/TypeScript#63661** (2026-07-20、magic-akari): `**` の間に `as` が挟まるケース (TypeScript 7.0.2 で確認) は別件。
   oxc が追従したかは未確認

**今日の他の発見と逆向き**: 4.1 の高速経路は「tsc に無い oxc 独自の最適化」だったが、これは **tsc (ts-go) の仕様変更を oxc が
追いかけている**例。tsc 5.9 の移植の上に、2026-06 の修正が足されている。

**追従の速さ (`gh` で実測、UTC)**: ts-go#4192 を出す 06-03 18:49 → **ts-go のマージ 06-04 21:14** → **oxc#22986 を出す 06-05 12:38
(ts-go のマージから約15時間後)** → **oxc のマージ 06-05 13:15 (出してから37分後、ts-go のマージから約16時間後)**。
以前ここに「3日」と書いたのは ts-go の PR を出した日から数えていて不正確。マージからは1日以内。

- 修正が小さい: oxc#22986 は **+118 −11、コミット1つ、10ファイル**。ただし内訳はテスト4ファイル
  (`tasks/coverage/misc/fail|pass/ts-unerasable-*.ts` など) とスナップショット5ファイルが大半で、パーサー本体
  (`expression.rs`) の変更は小さい。ts-go#4192 は +925 −7 (6ファイル) だが、パース部分は `lastOperand` の追跡と `break` の数行で、
  残りはテストと基準ファイルだと思う (推測)
- 移植先の構造が同じ: oxc の `parse_binary_expression_rest` は tsc の `parseBinaryExpressionRest` の移植で、腕の形が1:1に近い
  (コードを比べて確認)
- 追っているのは今回だけではない: 同じ時期の oxc に typescript-go 由来の PR が並ぶ (`#22845` TS1183、`#23999` conformance suite、
  `#24102` 以降の `oxc_type_checker` の PR など)。ただし本文に「Port of ... typescript-go」と書いた PR は検索で4件
- 作者 (Boshen) が oxc の中心的な作者で、同日にほかの PR も出していそう (推測)
- 判断も速い: リリースやバグ報告を待たず、ts-go のマージの翌日に取り込んでいる。tsc の実装を oxc のコードの隣に
  そのまま並べて保つ設計 (コメントアウトで tsc の原文を残す等、1.3 の「移植とは何を捨てるかの選択」) と整合する

### 5.2 読み始め: `parse_lhs_expression_or_higher_impl` (`js/expression.rs:752`) — 左辺式を1つ読む関数と、PR #23063 の近道

**左辺式 (LeftHandSideExpression) を1つ読む関数**。`f<T>(x)` の全体を作っていたのはここ (これまでのトレースで
`parse_lhs_expression_or_higher` → `parse_primary_expression` → `parse_member_expression_rest` → `parse_call_expression_rest` と出てきた)。
名前の `lhs` は Left-Hand-Side (代入の左辺になれる形の式: `a.b`・`f(x)`・`a?.b`)、`_or_higher` は「この段階かそれより強く結合する段階まで」、
`_impl` は本体 (入口の `parse_lhs_expression_or_higher` が `with_pure_comments` = `/* @__PURE__ */` の処理で包んで呼ぶ)。

```rust
let primary = self.parse_primary_expression();                          // ① 最初の1つ (`f`)
let member_expression = self.parse_member_expression_rest(start, primary, &mut in_optional_chain, true);  // ② 後置 (`.b` `[0]` `!` `<T>`)
let lhs = if matches!(self.cur_kind(), Kind::LParen | Kind::QuestionDot) {
    self.parse_call_expression_rest(start, member_expression, &mut in_optional_chain)   // ③ `(` か `?.` なら呼び出しの続き
} else { member_expression };
// 最後に、`?.` があれば (`in_optional_chain`) 全体を ChainExpression で包む (map_to_chain_expression)
```

`f<T>(x)` は「② の中で `<T>` が、③ の中で `(x)` が付く」順 (ES の `MemberExpression` = `a.b` `a[0]` タグ付きテンプレート、
`LeftHandSideExpression` = それに `(...)` か `?.` が付いたもの、に対応)。`map_to_chain_expression` の `match` に
`Expression::TSNonNullExpression` の腕があり、`a?.b!` の `!` は**オプショナルチェーンの中に入る要素**として扱われる (5.2 の入口)。

**`(` / `?.` のガードのコメントの訳**: 「完全にパースし終わった `MemberExpression` が `LeftHandSideExpression` に伸びるのは、
`Arguments` (`(`) か `OptionalChain` (`?.`) を経由するときだけ (ES の仕様の該当節)。だから、それ以外のときは
`parse_call_expression_rest` (と、その中の重複したメンバー式の再走査) を飛ばす」。

**この近道は PR #23063 (`perf(parser): skip parse_call_expression_rest when no call follows`、Boshen、2026-06-07 09:00Z →
12:16Z マージ、+20 −21、2ファイル) で入った** (`gh pr view` で確認):

| 項目 | 内容 |
|---|---|
| 何を | `(` か `?.` が続かないとき `parse_call_expression_rest` を呼ばない。呼び出し元が1か所だけだった `parse_member_expression_or_higher` もインライン化し、読み順が `primary` → `member_expression` → `lhs` の上から下になる |
| なぜ | `parse_call_expression_rest` の `loop` は中でもう一度 `parse_member_expression_rest` を実行する。**プロファイルで「1番のホットスポット」**。式の葉 (`a`・`1` など) ごとに無駄なメンバー式の再走査を払っていた。`sample` のプロファイルでは式の LHS の連鎖がパース時間の**約50%** |
| 正しさ | 振る舞いを変えない。`(` でも `?.` でもないとき `parse_call_expression_rest` は no-op。`a<T>()` は影響なし (`<T>` は member-rest で消費され `cur` が `(` になる)。**estree (AST・スパン・トークン) が `main` とバイト単位で同一**、メモリ割り当てのスナップショットも同一 |
| 効果 | 式が密な 2.5 MB のファイルで約22.6 ms → 約19.7 ms、**約13%高速化**。CodSpeed は kitchen-sink.tsx で +5.62%。混在した実際のコードでは中立 (節約は式の葉ごと) |

PR コメントに「`(` / `?.` のガードは経験則ではなく仕様に忠実」の節があり、ES の文法 (`CallExpression : MemberExpression Arguments` と
`OptionalExpression : MemberExpression OptionalChain`) を引用している。**tsc と ts-go の実装は同じ形で同じ重複した member-rest の再入を
持ち、最適化していない** (PR の本文の記述)。oxc が結果を保ったまま無駄を省いた例で、4.1 の `at_start_of_ts_declaration` の高速経路や
`parse_type_arguments_in_expression` の早期リターンと同じ種類の話 (前の「近道は #23063 と一致するようだが断定できない」は、PR を読んで確認できた)。

#### `<<` を割って失敗したとき: 収集済みトークン列の書き戻し (demos/oxc-step3 の demo13)

`parse_member_expression_rest` の `<` の腕 (と `parse_call_expression_rest` の `?.<`) の失敗側にある、コメント付きの書き戻し
(`self.lexer.rewrite_last_collected_token(self.token)`)。コメントの訳: 「`re_lex_as_typescript_l_angle` は収集済みのトークン列の元の `<<` を、
読み直した単独の `<` で上書きしてしまっているかもしれない。rewind はパーサーの現在のトークンを元に戻したので、その `<` の上に元のトークンを
書き戻す。トークン収集が静的に無効なとき (`NoTokensLexerConfig`) は何もしない」。Session 0 のメモの「失敗時の `<<` 書き戻し」の後始末そのもの。

`rewind` が戻すのはパーサーの状態とレキサーの位置だけで、**下流のツールに渡す「集めたトークンの列」は別のバッファ**なので自動では戻らない
(`lexer/typescript.rs:17-30` にも同じ趣旨の説明)。必要になるのは「`<<` を割って、しかも型引数として失敗する」入力だけ (`a << b;` など。
`Foo<<T>() => T>` や `f<<T>() => T>(x)` は成功するので不要、`a < b > c` は `<` が最初から単独なので上書きされない)。

実測 (`a << b;`、`tokens_dump` で収集済みトークン列を出す。書き戻しの1行を一時的にコメントアウトして比較):

| | 収集済みトークン列 |
|---|---|
| 書き戻しあり (通常) | `a` / `ShiftLeft "<<"` (2..4) / `b` / `;` |
| 書き戻しを無効にした場合 | `a` / **`LAngle "<"` (2..3)** / `b` / `;` ← `<<` が `<` に化けて1文字分が消える |

パースの結果 (AST) は同じでも**トークン列だけが壊れる**ので、トークンを使う下流のツール向けの後始末。トレースは `demo13_shift_left_fail.flow.txt`
(`[checkpoint]` → `[re_lex L]` → `parse_ts_type` → `[rewind] from 8 back to 2` → `parse_binary_expression_rest` がシフトとして読み直す)。

#### 5.2 `!` の腕は3行。面白いのは `?.` と組み合わさったときの木の形 (demos/oxc-step5b)

`parse_member_expression_rest` の `Kind::Bang if self.is_ts && !self.cur_token().is_on_new_line()` の腕は、`!` を食べて
`TSNonNullExpression` で包むだけ (`is_ts` で腕を分けるのは `<` の腕と同じ作り、改行の裁定は `as` の腕と同じ ASI)。実測で面白かったのは
`?.` との木の形 (oxc・typescript-estree 8.26.1・tsc 6.0.3 を並べた、11ケース):

| 入力 | oxc / typescript-estree | tsc (`⛓` = `NodeFlags.OptionalChain` が付いたノード) |
|---|---|---|
| `a?.b!` | `Chain(NonNull(Member?(a, b)))` | `NonNull(Prop?⛓(a, b))` (`!` が末尾なら NonNull はフラグ無しでチェーンの外側) |
| `a?.b!.c` | `Chain(Member(NonNull(Member?(a, b)), c))` | `Prop⛓(NonNull⛓(Prop?⛓(a, b)), c)` (`!` が途中なら `NonNull⛓`) |
| `(a?.b)!.c` | `Member(NonNull(Chain(Member?(a, b))), c)` (括弧がチェーンを閉じる) | `Prop(NonNull(Paren(Prop?⛓(a, b))), c)` |
| `a` 改行 `!b` | 2文 (`a` と `!b`) | 同じ |
| `a!` (`.js`) | エラー (`!` を後置と読まない) | `NonNull(a)` (tsc は JS でも木は作る) |

- **oxc の木は typescript-estree と全ケースで一致** (括弧のノードの有無だけ違う)。`null` を `TSNullKeyword` にするのと同じ「ESTree の形に合わせる」方針
- **tsc は `ChainExpression` を持たず、ノードに `OptionalChain` フラグを付ける**作り。oxc は `parse_lhs_expression_or_higher_impl` が `in_optional_chain` の旗を見て、
  `map_to_chain_expression` で式全体を `Chain` に包む。この `match` の **`TSNonNullExpression` の腕が、`a?.b!` の `NonNull` を `Chain` の中に入れる橋渡し**
  (腕が無いと `NonNull` が `Chain` の外に出る)
- **`a?.b!.c` は全体が1つの `Chain`**、`(a?.b)!.c` は括弧で `Chain` が閉じて外側の `.c` は含まれない (`?.` が失敗したとき `.c` も `undefined` になるかの違い)
- **`.js` では `!` を後置として読まない**: `as` (`.js` でも読んでエラーだけ出す) と違い、`is_ts` が偽だと `!` の腕に入らず、次の文の読み込みが「セミコロンが必要」のエラーにする
- **改行は `!` の前だけ見る** (`Kind::Bang if self.is_ts && !self.cur_token().is_on_new_line()` の `is_on_new_line()` は「今のトークン (`!`) の直前に
  改行があったか」の旗)。曖昧になるのは「`!` が後置か、次の文の前置 `!b` か」だけなので (ASI の考え方)、`!` の**後**の改行には規則が無い。実測 (oxc):

  | 入力 (`⏎` = 改行) | 木 |
  |---|---|
  | `a⏎!b` | `a` ; `!b` (2文。`!` の前の改行で後置でなくなる) |
  | `a!⏎.b` | `Member(NonNull(a), b)` (`!` の後の改行は関係なく `.b` が続く) |
  | `a!⏎(x)` | `Call(NonNull(a))` |
  | `a!⏎!b` | `NonNull(a)` ; `!b` (1つ目の `!` は後置、2つ目は前に改行があるので別の文) |

  `!` を食べた後は `parse_member_expression_rest` の `loop` に戻って次のトークンを見るだけで、改行の有無は見ない (`a⏎.b` がメンバーアクセスとして読める
  JS の普通のルールと同じ)。`as` の腕の改行 (`var x = foo⏎as (Bar)`) も `as` の**前**の改行で、同じ裁定
- **`?.template`...`` は仕様上不正だが、パーサーは読んでからエラーにする**: `parse_member_expression_rest` の `?.` の腕に
  `next_kind.is_template_start_of_tagged_template()` の分岐があり、`a?.`x`` も `a?.b`x`` も木を作ってから `parse_tagged_template_rest` の中で
  `in_optional_chain` が真ならエラー (oxc: `Tagged template expressions are not permitted in an optional chain`、node:
  `SyntaxError: Invalid tagged template on optional chain` を実測)。読まずに「予期しないトークン」で止めるより分かりやすいメッセージを出せるから、
  と思われる (推測)。仕様で禁止の理由は確認していない

## 進捗

- [x] Session 0: checkpoint / rewind / re-lex
- [x] Session 1: 型式コア (1.1 parse_ts_type / 1.2 関数型 / 1.3 union・intersection・前置演算子・infer /
      1.4 postfix・non_array_type)
- [x] Session 2: 型の難所 (2.1 mapped・2.2 tuple・2.3 template・2.4 predicate・2.5 infer は 1.3 で完了扱い)
- [x] Session 3: 3.3 (型引数) は先取り + 見直し (2026-09-21、demos/oxc-step3 の呼び出し順トレース) で完了。
      3.1・3.2 はスキップ
- [x] Session 4 (縮小): 4.1 は呼び出し元と高速経路まで読んだ (2026-09-21)。4.2・4.4 はスキップ
- [ ] Session 5: JS 式への食い込み (5.1 as/satisfies・5.2 `!` は完了、5.4 はスキップ、残りは 5.3 `<T>expr` と 5.5 arrow 曖昧性)。
      `!` と instantiation は `parse_member_expression_rest` の `match` の腕として見えている
- [x] Session 6: 丸ごとスキップ
