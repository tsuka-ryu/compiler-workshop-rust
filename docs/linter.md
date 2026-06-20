# Linter (3 ルール)

[next-steps.md](./next-steps.md) 節 11 の実装メモ。
節 5 の [`Visit`](../src/visit.rs) trait を土台に、`ast_span` AST を走査して警告を集める。
oxc の linter と同じく **ルール毎に visitor を 1 個** 作り、[`lint`](../src/lint.rs) が全ルールを走らせて結合する。

対応ファイル: [src/lint.rs](../src/lint.rs)

---

## 0. 全体構造

```rust
pub struct LintWarning {
    pub rule_name: String,
    pub message: String,
    pub span: Span,
}

pub fn lint(statements: &[Statement]) -> Vec<LintWarning> {
    let mut warnings = Vec::new();
    warnings.extend(constant_condition(statements));
    warnings.extend(unreachable_code(statements));
    warnings.extend(no_unused_vars(statements));
    warnings
}
```

各ルールは「**実行係の関数 1 個 + visitor struct 1 個**」のペア。`lint()` は 3 ルールを順に走らせて結果を結合するだけ。

---

## 1. ルールごとの具体例

実際に出る警告 (メッセージは実装そのまま) で示す。

### constant-condition

三項 `test ? a : b` の **test が `true`/`false` リテラル** のとき発火。

```js
const x = true ? 1 : 2;   // ⚠ Conditional test is always true; only the consequent runs
const x = false ? 1 : 2;  // ⚠ Conditional test is always false; only the alternate runs
const x = a ? 1 : 2;      // 警告なし (test が変数 = 動的)
```

ネストも拾う (`walk_expression` で子も辿るため)。

### unreachable-code

文の列で **`return` より後ろの文** が発火。後続 1 文につき 1 件。

```js
const f = (x) => {
  return x;
  const y = 1;   // ⚠ Unreachable code after return
};

const f = (x) => { return x; };   // 警告なし (return が最後)
```

到達不能な文の **中** のアロー関数 body も検査する (入れ子も拾う設計)。

### no-unused-vars

**宣言 (const + アロー引数) が一度も参照されない** と発火。スコープを抜けるとき回収。

```js
const x = 1;                      // ⚠ 'x' is declared but never used
const x = 1; const y = x;         // x は使用済み → 'y' の 1 件だけ
const f = (x) => { return 1; };   // ⚠ 'x' (param 未使用) + ⚠ 'f' (未使用) の 2 件
const f = (x) => { return x; };   // x は使用済み → 'f' の 1 件だけ
```

### ルールはまとめて走る

`lint()` は 3 ルール全部を流すので、1 入力で複数ルールが重なる:

```js
const x = true ? 1 : 2;
// → constant-condition (true 定数) + no-unused-vars (x 未使用) の 2 件
```

---

## 2. 「実行係の関数」は必要か

例えば `constant_condition`:

```rust
fn constant_condition(statements: &[Statement]) -> Vec<LintWarning> {
    let mut rule = ConstantCondition { warnings: Vec::new() };
    for stmt in statements {
        rule.visit_statement(stmt);
    }
    rule.warnings
}
```

`impl Visit` は「ノードを**どう訪問するか**」を定義するだけ。次の 3 つは trait には書けないので、誰かがやる必要がある:

1. visitor を **生成** (`ConstantCondition { warnings: Vec::new() }`)
2. トップレベルの `Vec<Statement>` を **回す** 入口 (walk は子へ再帰するが、ルート集合は手で回す)
3. `warnings` を **取り出して返す**

機能上は `lint()` にインライン化できる (関数を消せる)。残している理由は **ルール単体でテストできるから**:

```rust
let warnings = constant_condition(&parse("const x = true ? 1 : 2;"));
assert_eq!(warnings.len(), 1);   // このルールだけを検証 (lint() だと他ルールが混ざる)
```

oxc も rule ごとに独立した型 (`run` メソッド) に分けていて、動機は同じ。

---

## 3. `visit_expression` の中身 (constant-condition)

```rust
fn visit_expression(&mut self, expr: &Expression) {
    if let Expression::Conditional { test, span, .. } = expr {        // ①
        if let Expression::Boolean { value, .. } = test.as_ref() {    // ②
            let branch = if *value { "consequent" } else { "alternate" };  // ③
            self.warnings.push(LintWarning { ..., span: *span });
        }
    }
    walk_expression(self, expr);
}
```

- **① `if let ... Conditional`** — `expr` (10 種の enum) が `Conditional` バリアントのときだけ中へ。`..` は残りフィールド (`consequent`/`alternate`) を無視。`match` で 1 ケースだけ書くより短い定番形。
- **② `test.as_ref()`** — `test` は `Box<Expression>`。`as_ref()` で中身の `&Expression` を **借りて**、`Boolean` か判定する (後述)。
- **③ `*value`** — `test.as_ref()` が参照なので、取り出した `value` は `&bool`。`if` には `bool` が要るので `*` で外す。`span: *span` も `&Span` を `*` で外して `Span` (Copy) にしている。

### `as_ref()` とは

`Box<Expression>` → `&Expression` (中身への参照) を取り出すメソッド。パターンマッチで比べたい相手は `Box` でなく中身の `Expression` なので、その橋渡し。

```rust
test.as_ref()   // 明示的に &Expression を得る (採用)
&**test         // Box を *2 回で外して & で借り直す (記号が増えて読みにくい)
```

`AsRef` trait のメソッドで「**所有したまま参照だけ取り出す**」汎用の道具 (`String → &str`, `Box<i32> → &i32` など)。
linter は AST を**読むだけ**で壊したくないので、所有権を move せず `as_ref()` で借りるのが理にかなっている。

---

## 4. unreachable-code: 横 (`check_block`) と縦 (`visit_expression`)

`UnreachableCode` には役割の違う 3 つの登場人物がいる。

```rust
fn unreachable_code(statements) -> Vec<LintWarning> {   // ① 入口 (1 回呼ぶだけ)
    let mut rule = UnreachableCode { warnings: Vec::new() };
    rule.check_block(statements);
    rule.warnings
}

impl UnreachableCode {
    fn check_block(&mut self, statements: &[Statement]) {   // ② 文の「列」を見る
        let mut returned = false;
        for stmt in statements {
            if returned { /* 警告を push */ }
            self.visit_statement(stmt);   // 各文の「中」へ降りる
            if let Statement::Return { .. } = stmt { returned = true; }
        }
    }
}

impl Visit for UnreachableCode {
    fn visit_expression(&mut self, expr: &Expression) {   // ③ 式の「中」を降りる
        if let Expression::ArrowFunction { body, .. } = expr {
            self.check_block(body);   // アロー body は新しい「列」なので ② に戻す
            return;                   // walk に任せると body 二重訪問になるので打ち切る
        }
        walk_expression(self, expr);
    }
}
```

### なぜ `Visit` だけでは足りないか

「return の後」は **文の列を順番に見ないと判定できない** (前に return があったかという位置情報が要る)。
`Visit` の `walk_statement` は文を 1 個ずつ子へ降ろすだけで、「Vec の何番目か」「前に何があったか」を隠す。だから `returned` フラグを持って Vec を自分で for で回す `check_block` が必要。

| 見たいもの | 手段 |
|---|---|
| ノード単体の性質 (三項か?) | `Visit` の walk に任せる (constant-condition) |
| **兄弟ノードの順序** (return の後か?) | Vec を自分で回す (`check_block`) |

### ② と ③ は別の軸

- **② check_block** = 「横」を見る。同じ階層の文 `a; b; c;` を順に追う。
- **③ visit_expression** = 「縦」を見る。式の中に潜ってアロー関数を探す。

アロー関数の body も**また文の列**なので、③ で見つけたら ② に戻す。**②(横) と ③(縦) が交互に呼び合って** ネストした body まで届く。

```
② check_block (列を回す)
   └ 各文を ③ で降りる
       └ ArrowFunction を見つけたら body を ② に渡す (列に戻る)
           └ また各文を ③ で降りる … (繰り返し)
```

`no-unused-vars` も同じ「横 × 縦」構造で、違いはアロー関数で**スコープの出入り** (`push`/`leave_scope`) をする点。

---

## 5. no-unused-vars: スコープスタック

節 10 の symbol table を、span 付き・`used` フラグ付きで lint 用に作り直したもの。

```rust
struct Binding { span: Span, used: bool }   // used: 参照されたら true

struct NoUnusedVars {
    scopes: Vec<HashMap<String, Binding>>,   // スコープのスタック
    warnings: Vec<LintWarning>,
}
```

### 4 つのメソッド

- **`declare(name, span)`** — 今いる一番内側のスコープ (`last_mut`) に `used: false` で登録。節 10 の `declare` 相当。
- **`mark_used(name)`** — 内側→外側 (`rev()`) に探し、最初に見つかった binding の `used` を立てる。節 10 の `resolve` (parent を辿る) 相当。「lexical scope で一番近い宣言に解決」。
- **`leave_scope()`** — スコープを `pop` で閉じるとき、`used == false` のまま残った binding を警告に積む。**報告のタイミングはここだけ**。
- **`check_block(statements)`** — 文の列を処理。`const` は **init を先に visit してから declare** (節 10 と同じ。`const x = x;` の自己参照を正しく扱うため)。

### 「集める」と「報告する」を分ける

| パス | やること | メソッド |
|---|---|---|
| 集める | 宣言を登録、参照に印 | `declare` / `mark_used` |
| 報告する | スコープを閉じるとき未使用を回収 | `leave_scope` |

走査中はひたすら `used` を更新するだけで、警告を出すのは **スコープを閉じる瞬間だけ**。「使われたか」はそのスコープを全部見終わるまで分からないので、先に出すと取り消せない。

### トレース: `const f = (x) => { return 1; };`

1. トップレベル `check_block`
2. `const f = ...` → init (アロー) を `visit_expression` → **新スコープ push**
3. param `x` を declare (`used: false`)
4. body `check_block` → `return 1;` … `x` は触られない
5. **`leave_scope`** → `x` が未使用 → ⚠ `'x' is ...`
6. アロー処理後、トップで `f` を declare
7. 最後に `leave_scope` (トップ) → `f` 未使用 → ⚠ `'f' is ...`

→ `x` と `f` の 2 件。

---

## 6. なぜ節 10 の `naming_indexed` を再利用していないか

`no-unused-vars` は symbol table を**自前で再実装**していて、節 10 の `naming_indexed` は使っていない。AST の型が噛み合わないため:

| | 乗っている AST | span | identifier |
|---|---|---|---|
| 節 10 `naming_indexed` | `crate::parse` | **なし** | `Identifier(String)` |
| 節 11 `lint` | `ast_span` | **あり** | `Identifier { name, span }` |

`LintWarning` は span が必須だが、`naming_indexed` の `Symbol` は span を持たない (`crate::parse` AST がそもそも span を持たない)。
なので「未使用 symbol は取れても警告に載せる位置が無い」→ span 付きで作り直すしかなかった。**節 10 の*発想*は流用したが*コード*は流用できていない。**

本筋は oxc の `oxc_semantic` のように「**span 付き AST の上に semantic を 1 個だけ載せ、linter / transformer / LSP が全部それを参照する**」形。
学習用に各節を独立に並べた結果として重複が出ているので、節 14 (LSP) の前段で span 付き semantic に一本化する予定 (next-steps 節 14 参照)。

---

## 読書 TODO

- [ ] [oxc_linter](https://github.com/oxc-project/oxc/tree/main/crates/oxc_linter) — rule ごとに型を分けるスタイル
- [ ] [oxc_semantic](https://github.com/oxc-project/oxc/tree/main/crates/oxc_semantic) — span 付き AST 上の symbol table (一本化の参考)