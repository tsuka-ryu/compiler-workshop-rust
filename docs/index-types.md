# Index 型 (`SymbolId` / `ScopeId` / `ReferenceId`)

[next-steps.md](./next-steps.md) 節 10 の実装メモ。
既存 [naming.rs](../src/naming.rs) は `Vec<HashSet<String>>` のスタックで scope を管理し、
スコープを抜けると `pop` で**捨てて**いた。これを oxc の [`oxc_semantic`](https://github.com/oxc-project/oxc/tree/main/crates/oxc_semantic) 流に、
symbol / scope / reference を **`Vec` に捨てずに溜め、`u32` の添字で間接参照する**形へ書き直す。

対応ファイル:
- [src/naming_indexed.rs](../src/naming_indexed.rs) — index 型版 (節 5 の `naming.rs` から派生、並置スタイル)

---

## 1. arena (節 8/9) との違い — `Bump` は使わない

節 8/9 の arena は「ポインタ (`&'a T`) で間接参照」だったが、index 型は **同じ「集約して間接参照する」問題への別解**。
`Vec<Symbol>` は std の普通の `Vec`(bumpalo ではない)で、`SymbolId(u32)` はその添字。

| | 間接参照の手段 | ライフタイム | 主な用途 |
|---|---|---|---|
| 節 8/9 arena | `&'a T`(ポインタ) | `'a` が全体に伝染 | AST 本体(作って捨てる) |
| 節 10 index | `SymbolId(u32)`(添字) | **不要** | symbol/scope グラフ(相互参照する) |

旨味:

- **`'a` が消える** — `u32` なので持ち回り不要。「このシンボルはこのスコープの子」のような
  **相互参照(グラフ)** を `&'a` / `Rc` で絡めず、ただの `u32` で表せる。
- **`Copy`** で軽く、シリアライズも容易(ポインタと違い `u32` はそのまま保存できる)。
- **キャッシュ局所性** — `Vec` に連続配置される。

節 8/9 で `'a` に散々苦しんだ後に「**ポインタの代わりに添字を使えば `'a` から解放される**」と気づく回。

---

## 2. 型

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);     // symbols: Vec<Symbol> の添字
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);      // scopes: Vec<Scope> の添字
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReferenceId(pub u32);  // references: Vec<Reference> の添字

pub struct Symbol {
    pub name: String,
    pub scope_id: ScopeId,   // どのスコープで宣言されたか
}

pub struct Scope {
    pub parent: Option<ScopeId>,             // 親スコープ。root だけ None
    pub bindings: HashMap<String, SymbolId>, // この階層の宣言だけ (親は含まない)
}

pub struct Reference {
    pub name: String,
    pub scope_id: ScopeId,           // どのスコープから参照したか
    pub resolved: Option<SymbolId>,  // 解決先。None なら未宣言エラー
}
```

旧版に存在しなかったのが `Symbol` / `Reference` の **実体**。旧版は名前を `HashSet` に入れるだけで、
「宣言済みか」の bool しか持たなかった。

`u32` を使うのは oxc と同じ理由で、**id を小さく保つ**ため(`Copy` とキャッシュ局所性が旨味なので、`usize` でなく 32bit)。
1 ファイルの宣言数が 42 億を超えることはまずないので実用上無限。

---

## 3. スタック → `current_scope` 1 個

`SemanticBuilder` は旧版の `Resolver` 相当だが、**スコープのスタックを持たない**。
代わりに「いまどのスコープか」を `current_scope: ScopeId` で 1 個だけ持ち回る。

```rust
pub struct SemanticBuilder {
    symbols: Vec<Symbol>,
    scopes: Vec<Scope>,
    references: Vec<Reference>,
    current_scope: ScopeId,  // 旧版の「スタックの一番後ろ」に相当
    errors: Vec<NamingError>,
}
```

### `push`/`pop` の化け方

旧版の scope 操作が、id の付け替えに化けるのが本節の核。

```rust
// 旧 naming.rs
self.scopes.push(HashSet::new());  // 入る
self.scopes.pop();                 // 抜ける = 実体を捨てる

// index 版
fn enter_scope(&mut self) -> ScopeId {
    let new_id = ScopeId(self.scopes.len() as u32); // push 前の len が新添字
    self.scopes.push(Scope { parent: Some(self.current_scope), bindings: HashMap::new() });
    let saved = self.current_scope;                 // 付け替え前に退避
    self.current_scope = new_id;
    saved                                           // 旧 id を返す
}

fn leave_scope(&mut self, saved: ScopeId) {
    self.current_scope = saved;  // 戻すだけ。push した実体は Vec に残す
}
```

| | naming.rs | naming_indexed.rs |
|---|---|---|
| 入る | `scopes.push(HashSet)` | `enter_scope`(実体を積む + 付け替え、旧 id を返す) |
| 抜ける | `scopes.pop()`(**実体を捨てる**) | `leave_scope`(id を戻すだけ・**実体は残す**) |
| 親子 | スタック順序が暗黙 | `Scope.parent` で明示 |

呼び出し側 (アロー関数) は、旧 id を受け渡して入れ子を表現する:

```rust
Expression::ArrowFunction { params, body, .. } => {
    let saved = self.enter_scope();
    for param in params { self.declare(&param.name); }
    for stmt in body { self.visit_statement(stmt); }
    self.leave_scope(saved);
}
```

---

## 4. `is_declared` → `resolve`

旧版の `is_declared`(全スコープを舐めて bool)は、`current_scope` から **`parent` チェーンを上る** 形になる。
返すのも bool ではなく `SymbolId`。

```rust
fn resolve(&self, name: &str) -> Option<SymbolId> {
    let mut current = Some(self.current_scope);
    while let Some(scope_id) = current {
        let scope = &self.scopes[scope_id.0 as usize];
        if let Some(&symbol_id) = scope.bindings.get(name) {
            return Some(symbol_id);
        }
        current = scope.parent;  // 見つからなければ親へ
    }
    None
}
```

旧版の `scopes.iter().any(...)` との違いは「**今の枝の祖先だけ**を見る」点。
スタックではなく木を上るので、別の枝のスコープは見えない(正しい lexical scope)。

参照は捨てずに `Reference` として残す:

```rust
Expression::Identifier(name) => {
    let resolved = self.resolve(name);
    if resolved.is_none() {
        self.report(format!("Reference to undeclared variable: {name}"));
    }
    self.references.push(Reference {
        name: name.clone(),
        scope_id: self.current_scope,
        resolved,  // ← 解決先を残す。旧版は bool 判定して捨てていた
    });
}
```

---

## 5. 借用チェッカの罠 (`declare`)

`declare` で「重複チェック → エラー報告」を書くとき、scope を変数に束ねたまま `self.report()` を呼ぶと
不変借用と可変借用が衝突する。対策は **scope 変数を作らず、その都度 `self.scopes[idx].bindings...` と書く**
(借用を式の中で完結させ、持ち越さない)。節 9 の「`intern`(&mut) と `alloc`(&) を別文に分ける」と同じ話。

```rust
fn declare(&mut self, name: &str) -> Option<SymbolId> {
    let idx = self.current_scope.0 as usize;
    if self.scopes[idx].bindings.contains_key(name) {   // 借用を式内で完結
        self.report(format!("Duplicate declaration of variable: {name}"));
        return None;
    }
    let symbol_id = SymbolId(self.symbols.len() as u32);
    self.symbols.push(Symbol { name: name.to_string(), scope_id: self.current_scope });
    self.scopes[idx].bindings.insert(name.to_string(), symbol_id);
    Some(symbol_id)
}
```

---

## 6. 「溜める」設計でメモリは大丈夫か

スコープを捨てないので push し続けるが、破綻しない理由は 3 つ:

1. **量はソースサイズで頭打ち** — symbols は宣言数、references は使用箇所数、scopes は関数/ブロック数。
   いずれも 1 ファイルの中身に比例するだけ(巨大ファイルでも数万オーダー)。
2. **寿命が per-file** — 現状の `name_check` は `errors` だけ返し、`builder`(symbols/scopes/references 全部)は
   関数を抜けた瞬間に drop。arena と同じ「作る→使う→ファイルごと丸ごと捨てる」。
3. **`u32` の上限** — 約 42 億。1 ファイルでは到達しない。

oxc の `oxc_semantic` は逆にこれを `Semantic` 構造体として**返して保持**する。後段(linter / transformer / LSP)が使うため。
- CLI: 1 ファイル処理 → 使う → 次のファイルへ(前のは drop)。常駐しても 1 ファイル分。
- LSP: 開いているファイル分だけ保持し、編集のたび作り直して古いのを drop。

「溜める」のは設計通りで、解放のタイミングが per-file に設計されているから破綻しない。
節 11(linter の未使用変数検出)/ 節 14(LSP の go-to-definition)で、この `builder` を捨てずに返す形に変えると実感できる。

---

## 7. なぜ後段が楽になるか

解析後に symbols / scopes / references が `Vec` に残るので、index で何度でも引ける:

- **未使用変数 (節 11 linter)**: 各 `Symbol` が一度も `Reference.resolved` に現れないかを数えるだけ。
- **go-to-definition (節 14 LSP)**: カーソル位置の `Reference` → `resolved: SymbolId` → `Symbol` の宣言位置、と添字で辿る。
- **find-references**: ある `SymbolId` を `resolved` に持つ `Reference` を集めるだけ。

これが「実用 semantic API のシグネチャがなぜ index 型を返すのか」の答え。
`&'a` で返すと `'a` が API 全体に伝染するが、`u32` なら呼び出し側に何も強制しない。

---

## 読書 TODO

- [ ] [oxc_semantic](https://github.com/oxc-project/oxc/tree/main/crates/oxc_semantic) — Symbol / Scope / Reference の管理
- [ ] [oxc_index](https://github.com/oxc-project/oxc/tree/main/crates/oxc_index) — index 型の基盤 (`IndexVec` など)