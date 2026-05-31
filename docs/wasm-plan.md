# WebAssembly コード生成の方針

JS版 `solutions/7/wasm.js` を Rust に移植 + 多相関数の monomorphization 対応。

**ステータス: 全 Phase 完了 ✅**

## テスト戦略

- `wasmparser` クレートで生成バイナリの構造検証
- 実行はしない（wasmtime を使う案は見送り、依存軽量化を優先）

## モジュール構成（実装後）

- `src/wasm.rs` — 1 ファイルにまとめた（セクションごとに `===` コメントで区切り）
- `src/typecheck_mono.rs` — typecheck.rs のコピー + wasm 向けの型情報抽出 (`Type`, `FunctionScheme`, `type_check_with_info`)
- `src/monomorphize.rs` — 多相関数の単相化 (specialization 収集、AST 書き換え、エイリアス解決)

## 全体ロードマップ

### 基盤フェーズ（単相の式を wasm に落とす）

| Phase | 内容 | 動作する例 |
|---|---|---|
| 0 | LEB128 (unsigned/signed)、f64 エンコーディング、byte builder | ユニットテスト |
| 1 | 最小 WASM モジュール（空 `main`） | wasmparser でパース成功 |
| 2 | 数値リテラル + 戻り値 | `const x = 42;` |
| 3 | 二項演算（`+` `*`、Number 同士） | `const x = 1 + 2 * 3;` |
| 4 | ローカル変数（const → local） | `const x = 5; const y = x + 1;` |
| 5 | Boolean + 三項演算（IF/ELSE/END） | `const x = true ? 1 : 2;` |

### 関数フェーズ

| Phase | 内容 | 動作する例 |
|---|---|---|
| 6 | 単相関数の定義と呼び出し | `const add = (a: number, b: number) => { return a + b; }; const r = add(1, 2);` |
| 7 | 文字列リテラル（Data section） | `const s = "hi";` |

### 多相フェーズ（monomorphization）

| Phase | 内容 |
|---|---|
| 8 | 型情報付き AST を作る（typecheck の結果を AST に反映） |
| 9 | 多相関数の各呼び出しから `(関数名, 具体的なシグネチャ)` を収集 |
| 10 | 各 (関数名, シグネチャ) ごとに関数を複製、シグネチャに従って型を確定 |
| 11 | Call の callee を具体化された名前に向け直す<br>（同時にエイリアス解決：`const f2 = id;` の `f2` を `id` に置換） |
| 12 | Phase 6 のコードを流用して wasm に落とす（全部単相になってる） |

### サポートするパターン

- 単純なエイリアス（`const f2 = id;` で、右辺が **Identifier だけ**）
  - Phase 11 で「Identifier の正体を辿る」処理を足してエイリアス解決

### スコープ外（やらない）

- `console.log` の import
- 高階関数の monomorphization（`map(f, xs)` のようなパターン）
- 右辺が複雑なエイリアス（`const f2 = make_func();`）
  - コンパイル時に正体を追跡しきれない

## 前提知識

### WASM モジュールの構造

```
magic header (\0asm\01\00\00\00)
+ Type Section     (1)  関数シグネチャ一覧
+ Function Section (3)  関数 i のシグネチャは type j
+ Memory Section   (5)  線形メモリの宣言（文字列用）
+ Export Section   (7)  JS から見える名前
+ Code Section    (10)  関数の本体（命令列）
+ Data Section    (11)  初期メモリ内容（文字列リテラル）
```

各セクション: `[section_id, size_in_LEB128, payload]`

### 主な命令

- `LOCAL_GET (0x20)`, `LOCAL_SET (0x21)` — ローカル変数
- `F64_CONST (0x44)`, `F64_ADD (0xa0)`, `F64_MUL (0xa2)` — 数値
- `I32_CONST (0x41)` — Boolean / 文字列ポインタ
- `IF (0x04)`, `ELSE (0x05)`, `END (0x0b)` — 制御フロー
- `CALL (0x10)`, `RETURN (0x0f)` — 関数呼び出し

### 型の対応

| ソース言語の型 | WASM 型 |
|---|---|
| Number | f64 |
| Boolean | i32 (0/1) |
| String | i32 (linear memory のポインタ) |

### LEB128

可変長整数エンコーディング。WASM の長さフィールドや整数定数に使う。

- ULEB128: 符号なし。下位7bitずつ取り出して continuation bit (MSB) を立てる
- SLEB128: 符号あり。符号拡張に注意

## Monomorphization の設計

JS版に**ない**新規設計。Rust のジェネリクスがやってる単相化のミニチュア版。

### 部品

#### A: 型情報付き AST

現在の `Expression` / `Statement` には型情報がない。typecheck で各ノードの TypeId は分かるが、保持していない。

選択肢：

- **A1**: 既存 enum に `type_id: TypeId` フィールドを足す（侵襲的）
- **A2**: 別 enum `TypedExpression` を作って、typecheck の代わりにこれを返す（綺麗）
- **A3**: `HashMap<*const Expression, TypeId>` で外部から紐付け（不衛生）

**A2 を採用予定**。「typecheck の出力 = 型付き AST」と明確に分けられる。

#### B: 使用ペアの収集

型付き AST を走査して `(関数名, 具体的なシグネチャ)` を集める：

```rust
type ConcreteSignature = (Vec<ConcreteType>, ConcreteType);  // (params, return)
type Specializations = HashMap<String, HashSet<ConcreteSignature>>;
```

例：

```js
const id = (x) => x;
const a = id(5);
const b = id("hi");
const c = id(true);
```

→

```
Specializations = {
    "id" → {
        ([Number], Number),
        ([String], String),
        ([Boolean], Boolean),
    }
}
```

#### C: 単相化された AST

各 (関数名, シグネチャ) ごとに関数定義を複製。命名規則は `id_Number_Number` のような形。元の `id` 定義は削除。

#### D: Call の書き換え

`Call { callee: Identifier("id"), arguments: [5] }` を見たら、引数の型から「これは `id_Number_Number` 版だ」と決定して書き換える。

### 実装の流れ

```
入力: type-checked AST
   ↓
[Phase 9] 使用ペアを集める（HashMap<name, HashSet<signature>>）
   ↓
[Phase 10] 各ペアごとに関数を複製、TypeAnnotation を具体型で埋める
   ↓
[Phase 11] Call を具体化された名前で書き換え、元の多相 const 宣言を削除
   ↓
出力: monomorphized AST（全部単相）
   ↓
[Phase 12] Phase 6 の単相コード生成器に渡す
```

## 規模感

| Phase | 推定難易度 |
|---|---|
| 0-2 | 易 |
| 3-5 | 中 |
| 6-7 | 中の上（関数テーブル、ローカル管理） |
| 8 | 中（typecheck の結果を AST に反映） |
| 9-11 | 難（収集・複製・書き換え） |
| 12 | 易（Phase 6 を流用） |

合計で parse 並みの規模（500〜800行）の見込み。