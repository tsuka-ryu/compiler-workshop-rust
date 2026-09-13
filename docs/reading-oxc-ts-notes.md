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

### 感想・LT ネタ候補

- doc コメントの密度が異常。typescript.rs は 53 行中コード 10 行、残り全部が
  「checkpoint との相互作用」の説明。LT でそのまま見せられるサイズ
- 投機パースの本質は「失敗のコストを arena が吸収する」こと。GC 言語や malloc/free だと
  この設計は重くなる。arena allocator と投機パースの相性の良さが oxc の速さの一因

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

## 進捗

- [x] Session 0: checkpoint / rewind / re-lex
- [ ] Session 1: 型式コア
- [ ] Session 2: 型の難所
- [ ] Session 3: signature member / try_parse_type_arguments
- [ ] Session 4: ts/statement.rs + modifiers.rs
- [ ] Session 5: JS 式への食い込み
- [ ] Session 6: class / function / module
