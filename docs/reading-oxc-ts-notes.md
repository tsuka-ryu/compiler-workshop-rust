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

2段ラッパー経由で、需要は全部「型引数リストの開閉」:

- 第1段 cursor.rs:276/293 `re_lex_ts_l_angle` / `re_lex_ts_r_angle` —
  トークン種別で何文字戻すか振り分けるだけ (`<<`→2, `<<=`→3 / `>>`→2, `>>>`→3)
- 第2段 ts/types.rs の3関数: `try_parse_type_arguments` (864, 型文脈) /
  `parse_type_arguments_of_type_reference` (890, 改行チェック付き) /
  `parse_type_arguments_in_expression` (929 で開き `<`、942 で閉じ `>`) ← 本丸
- 失敗時の `<<` 書き戻しは expression.rs:929 / 1130 の2箇所
- 「`<=` は必ず失敗する」の答え: types.rs:920 のコメント —
  **型は `=` から始まらない** ので `f<=T>` の `=T>` は型引数になりえず、事前に弾く

Session 3 ではこの3関数の中身 (成功と判定する条件) を読む。

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

#### LT 候補の整理 (Session 0-1 時点)

| 候補 | 掴み | 難度 | 備考 |
|---|---|---|---|
| A. `f<T>(x)` vs `a<b>c` の曖昧性 | 誰でも読めるコード | 中 (動きの説明が要る) | 説明型。5分だと駆け足 |
| B. コードよりコメントが長いファイル | 53行中40行の絵面 | 低 | 主張型。息切れしにくい |
| C. コメントアウトされた他人のコード | JS のままのコメント | 低 | **B の具体版。1行+診断2枚で完結** |
| D. tsc と oxc のエラー回復の違い | Playground との対比 | 中 | C に内包できる |
| E. そのエラー、誰が出してるの? | 「通っちゃった」の意外性 | 低〜中 | パーサー読書の外の話も入れられる。3層の絵が描きやすい |

B と C は同じ「コメントから設計を読む」筋なので、どちらか1本に統合するのが自然。
C を軸に B の補強 (typescript.rs の 53行中40行) を入れるのが今のところ最有力。
E は毛色が違い (コードの細部でなくエコシステムの構造)、聴衆が TS ユーザー中心なら
こちらの方が刺さるかもしれない。

## 進捗

- [x] Session 0: checkpoint / rewind / re-lex
- [ ] Session 1: 型式コア (1.1 parse_ts_type / 1.2 関数型 / 1.3 前半 union・intersection 読了。
      次は `parse_type_operator_or_higher` 282 から)
- [ ] Session 2: 型の難所
- [ ] Session 3: signature member / try_parse_type_arguments
- [ ] Session 4: ts/statement.rs + modifiers.rs
- [ ] Session 5: JS 式への食い込み
- [ ] Session 6: class / function / module
