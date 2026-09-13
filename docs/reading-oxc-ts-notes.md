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

## 進捗

- [x] Session 0: checkpoint / rewind / re-lex
- [ ] Session 1: 型式コア (parse_ts_type 15-44 読了、次は union 245 から)
- [ ] Session 2: 型の難所
- [ ] Session 3: signature member / try_parse_type_arguments
- [ ] Session 4: ts/statement.rs + modifiers.rs
- [ ] Session 5: JS 式への食い込み
- [ ] Session 6: class / function / module
