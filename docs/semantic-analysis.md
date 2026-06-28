# 意味解析 (semantic analysis) — toy 版と JS 版の通し解説

パースで AST ができた後に、**「構文は正しいが意味が正しいか」を調べる**段階。
このリポジトリでは同じことを 3 回書いている:

1. toy `src/naming.rs` — スタック方式 (素朴版)
2. toy `src/naming_indexed.rs` — index 型方式 (oxc 流)
3. JS `src/js/semantic.rs` — ②をほぼそのまま JS subset に移植

本ドキュメントはこの 3 つを貫く「芯」と、各版の違いをまとめる。

---

## 1. 芯は「3 つの操作」だけ

部品 (Scope / Symbol / Reference / errors) は多いが、やっていることは実質これだけ:

| 操作 | いつ呼ぶ | 何をする | メソッド名 |
|---|---|---|---|
| **declare** | 宣言を見たとき (`let a` / `const a`) | 今のスコープに名前を登録 | `declare` |
| **resolve** | 名前を使ったとき (式中の `a`) | 今のスコープ → 親 → … と辿って宣言を探す | `resolve` |
| **enter / leave** | スコープの出入り (`{ }`、アロー関数) | 「今どこにいるか」(`current_scope`) を切り替える | `enter_scope` / `leave_scope` |

残りの `Symbol` / `Reference` / `errors` / `scopes` Vec は、**この 3 操作の記録帳**にすぎない。

> 一言でいうと: **意味解析 = declare で名前をスコープに入れ、resolve で使用箇所を宣言に結びつける。
> スコープの出入りで現在地を切り替える。** これだけ。

### なぜ「スコープツリー」が要るのか

`resolve` の「今のスコープに無ければ**親を辿る**」を成立させるため。これがあるから:

- 内側から外側の変数が見える (`let a; { a }` → 親で見つかる)
- 同名でも内側が優先される = **シャドーイング** (自分のスコープで先に見つかる)

`Scope { parent: Option<ScopeId> }` のツリーは **resolve のためだけの土台**。
「ツリーの目的は resolve」と覚えると忘れない。

---

## 2. toy の 2 段階 — 「なぜ index 型か」

toy は意味解析を 2 回書いており、その差分が「なぜ id (添字) を使うのか」の答えになっている。

### ① `naming.rs` — スタック方式 (素朴版)

- データ構造: `Vec<HashSet<String>>` (名前の集合を積んだスタック)
- enter で `push(HashSet::new())`、leave で `pop()` = **スコープを抜けたら捨てる**
- 答えられるのは「宣言されてる?」の **bool だけ** (`is_declared`)

```text
scopes (スタック):
  [ {a, b} ]            ← トップレベル
  [ {a, b}, {x} ]       ← ブロックに入ると push
  [ {a, b} ]            ← 抜けると pop して破棄
```

問題: 抜けると消えるので、後から「あの `x` はどの宣言だった?」を聞けない。
bool しか持たないので、使用箇所と宣言の**結びつき**が残らない。

### ② `naming_indexed.rs` — index 型方式 (oxc 流)

- データ構造: `Vec<Scope>` + 各 Scope が `parent: Option<ScopeId>` を持つ + `u32` newtype の id
  - `SymbolId(u32)` = `symbols: Vec<Symbol>` の添字
  - `ScopeId(u32)` = `scopes: Vec<Scope>` の添字
  - `ReferenceId(u32)` = `references: Vec<Reference>` の添字
- enter しても leave しても **Vec から消さない**。`current_scope` を 1 個付け替えるだけ
- 親子は**スタックの順序 (暗黙)** ではなく `parent` リンク (明示) で持つ
- 答えるのは「**どの宣言?**」= `SymbolId` (`resolve`)。参照も捨てず
  `Reference { resolved: Option<SymbolId> }` で残す

```text
symbols: [ Symbol{a, scope0}, Symbol{x, scope1} ]   ← 捨てずに溜める
scopes:  [ Scope{parent:None}, Scope{parent:Some(0)} ]
references: [ Reference{ "x", resolved: Some(1) } ]   ← 使用箇所も残る
```

### ①→② の本質

| | ① naming.rs | ② naming_indexed.rs |
|---|---|---|
| スコープ | スタックに push/pop (**捨てる**) | Vec に溜める (**残す**) |
| 親子関係 | スタック順序 (暗黙) | `parent: Option<ScopeId>` (明示) |
| 間接参照 | なし | `u32` 添字 (`SymbolId` 等) |
| resolve の答え | **bool** (宣言されてる?) | **SymbolId** (どの宣言?) |
| 参照 | bool 判定して捨てる | `Reference{resolved}` で残す |

進化の正体は **「bool で捨てる」→「id で残して結びつける」**。
残すからこそ、後段のツールが使える:

- **リンター**: 「宣言したが参照されない変数」= symbol と reference を突き合わせる
- **LSP の go-to-definition**: 「カーソル位置の `a` の宣言元へ」= `resolve` の結果そのもの
- **リネーム/ミニファイア**: 衝突しない変数名への置換

index 型 (`u32` 添字) を選ぶ理由は arena (節8) の `&'a` とは別軸で、
「相互参照するグラフ (symbol ↔ scope ↔ reference) を `'a` や `Rc` で絡めず、ただの `u32` で表す」ため。
`Copy` で軽く、シリアライズも容易。詳しくは [index-types.md](./index-types.md) 参照。

---

## 3. JS 版 (`src/js/semantic.rs`)

②をほぼそのまま JS subset に移植したもの。骨格 (SymbolId/ScopeId、Scope、
declare/resolve/enter/leave、visit_*) は **②と完全に同じ**。違いは 3 点だけ:

| | toy ② naming_indexed | JS semantic |
|---|---|---|
| スコープを作る所 | **アロー関数** `(x) => { }` | **ブロック** `{ }` (`BlockStatement`) |
| 未宣言参照 | **エラーにする** (`y` 未定義) | **エラーにしない** (JS はグローバル参照になる) → `resolved: None` で残すだけ |
| 返り値 | `Vec<NamingError>` (エラーだけ) | `Semantic` (scopes/symbols/references/errors 全部) |

JS 版で意味解析を「味わう」ために、subset には無かった `{ }` ブロックを足してある
(`ast.rs` の `BlockStatement`、`parser.rs` の `parse_block_statement`)。
これでネストしたスコープ・シャドーイングが作れる。

### メソッドを 3 操作に分類して読む

理解が散らからないコツ: 各メソッドを「declare / resolve / enter-leave のどれか」に分類する。

- `visit_statement` の `VariableDeclaration` 枝 → **declare**
- `visit_expression` の `Identifier` 枝 → **resolve**
- `visit_statement` の `BlockStatement` 枝 → **enter / leave**
- それ以外の枝は**木を降りているだけ** (子を `visit_*` で再帰)

これが全部。部品の多さに惑わされない。

---

## 4. 元チュートリアル (semantic_analysis 章) との対応

JS 版は章の「フル版」に対する subset。章は JS の全機能 (var/関数/アロー/strict/generator)
に対応するため部品が多いが、**芯 (3 操作 + scope tree) は同じ**。省いたものを並べる:

| 章フル版 | JS semantic.rs | 省いた理由 |
|---|---|---|
| `lexical` / `var` / `function` の 3 マップ | `bindings` 1 個 | var hoisting を扱わないため。重複は同一スコープ同名を一律 early error (実 JS の `var a; var a;` 許容は省略) |
| `indextree` (arena 木) | `Vec<Scope>` + `parent` | 手書きで十分。やることは同じ (親を辿る) |
| `ScopeFlags` (TOP/FUNCTION/ARROW/VAR の bitflags) | `ScopeKind { Top, Block }` | 関数/アロー/class が subset に無い |
| `strict_mode` の継承 | 無し | `"use strict"` の概念を入れていない |
| `[Yield]` / `[Await]` 文法コンテキスト | 無し | generator / async が subset に無い |
| var hoisting / TDZ | 無し | var を関数スコープに巻き上げる処理をしない |

### 章の Early Error (参考)

章が検出すべきとするブロックの Early Error は 2 つ:

```text
Block : { StatementList }
* LexicallyDeclaredNames に重複       → 構文エラー  (let a; let a;)
* LexicallyDeclaredNames ∩ VarDeclaredNames → 構文エラー  (let a; var a;)
```

JS の宣言は 2 系統あり、これを区別するのが肝:

- **LexicallyDeclaredNames** = `let` / `const` / `class` (ブロックスコープ)
- **VarDeclaredNames** = `var` (関数スコープに巻き上げ)

ルールは「let 同士の重複は NG / let と var の衝突も NG / だが var 同士は OK」。
これを判定するために章は名前マップを 3 つに分ける。JS semantic.rs は `bindings` 1 個に統合し、
種別問わず重複を一律エラーにする簡略版にした。

### 章の「コンテキスト」(`[Yield]` / `[Await]`)

`yield` / `await` は**文脈依存キーワード**。普通の関数では識別子に使えるが、
generator の中では `yield` が、async の中では `await` が予約語になり、変数名に使うと構文エラー。
仕様はこれを `BindingIdentifier[Yield, Await]` のように**プロダクションのパラメータ**で表し、
`[+Yield]` (フラグ ON) の文脈で `yield` が来たら early error とする。
実装するにはパース/意味解析中に文脈フラグを持ち回る必要がある。
subset には generator/async が無いので未実装 (`yield`/`await` は lexer でキーワードにすらしていない)。

---

## まとめ

- 意味解析の芯は **declare / resolve / enter-leave の 3 操作** + それを支える **scope tree**。
- toy は ① スタック/bool → ② Vec+id/SymbolId と進化させ、「捨てる」から「残して結びつける」へ。
- JS 版は②の移植。スコープを作るのが `{ }`、未宣言は非エラー、結果を全部返す、の 3 点だけ違う。
- 章フル版との差は全部「JS 特有の例外ルール (var/strict/yield) を足すか省くか」で、芯は不変。

関連: [index-types.md](./index-types.md) (なぜ `u32` 添字か) / [linter.md](./linter.md) (意味解析の結果を使う後段)
