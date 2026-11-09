# tsc を読みながら「健全性とは何か」を理解する読書計画

これは読書計画。読んで分かったことは [reading-tsc-soundness-notes.md](reading-tsc-soundness-notes.md) に書く。

## 目的

**tsc (tsgo) のコードを読む。** 健全性の穴を入口にして、関係判定 (relater.go) を中心に、checker.go とフロー解析 (flow.go) を必要な分だけ読む。checker.go は巨大なので、通読はせず、穴ごとに入口を決めて追う。

- 背景: 最近、Bun や scriptc など、TypeScript を AOT コンパイルに使う流れがある。そこで「健全性が大事」と言われる- 問い: 型検査が通ったプログラムに期待してよいことは何か (AOT で何が変わるかは今回の対象外)
- 面白そうな穴だけ、tsrs にパッチを当てて、実コードでの壊れ方を測る。全部の穴でやるとは決めない。やるかどうかは、その穴のコードを読んでから決める

## 読む対象と進め方

- 対象: `microsoft/TypeScript` の `tsc/internal/checker/` (Go 移植版 = tsgo)
- 読んだ時点の rev: `f7973164a` (2026-11-08 時点の main)。行番号は rev に依存するので、ずれたら関数名で探す
- 副読本 (同じ関数を Rust で読む。**tsrs は tsgo の Rust 移植なので、関数名が `snake_case` になるだけで対応する**):
  - `oxc-project/tsrs` (rev `fc384313`、0.23.0 の準備コミット): `crates/tsrs_checker/`。対応表は `docs/CHECKER.md`、移植方針は `docs/PORTING.md`。Go の `checker.go` (約 32,600 行) は `checker_01.rs` … `checker_15.rs` に Go の行範囲で分割、`relater.go` は `relater_1.rs` / `relater_2.rs`、`flow.go` は `flow.rs`。Rust の練習を兼ねて読むなら、こちらを並べて読む。下の tsrs 側の行番号は計画作成時のもので、この rev での確認はしていない
  - `oxc-project/oxc` (rev `9f5297f781`): **型チェッカーはない**。穴 7 のフロー解析の比較対象として、`oxc_cfg` (制御フローグラフ) と `oxc_semantic` (スコープ・シンボル) を読む。中身は未読 (対応関係は想定で、読んで確かめる)
- 設計意図の履歴は移植後の git blame では追えない。PR / issue を読む (番号と題名は確認済み)
  - strictFunctionTypes: #18654 (Strict function types、Anders Hejlsberg、2017-10 マージ)
  - noUncheckedIndexedAccess: #39560 (Ryan Cavanaugh、2020-09 マージ)
  - CFA のトレードオフ: #9998 (open)
  - プロパティの読み書きで部分型を分ける提案: #18770 (Ryan が穴 2・3 の議論で何度も参照している。未読)
- 他実装の資料 (健全モード。穴ごとの「他実装の発想」に混ぜてある):
  - tsz: https://tsz.dev/sound-mode/ (2026-11-08 取得)、https://github.com/tsz-org/tsz/blob/main/docs/plan/SOUND_MODE.md (同日取得。`Last Updated: April 2026`、Status: Experimental / Partially Implemented)。約 95,000 文字のうち末尾の約 16,000 文字は要約経由でしか読めていない。tsz のコードとの突き合わせはしていない
  - Rust / Haskell の記述は記憶ベースで未確認

1 つの穴あたりの進め方:

1. 5 行程度の再現コードを 1 つ書く
2. 関係判定の穴は `isTypeRelatedTo` (relater.go:170) にブレークポイントを置いて追う (通読はしない)。インデックスアクセスとフロー解析は、それぞれの入口から追う
3. 読んだ結果を、読書メモに書く。「どちらが壊れているか」(下の `TAPL Ch 8 の見方`) も書く
4. (任意) 面白ければ、tsrs にパッチを当てて測る。「パッチと測定をやるとき」を参照

順番は下の番号順 (測定が楽な順。最後のフロー解析が一番重い)。

### 読み始めの足場

TAPL の T-If が tsgo のどこに当たるかを、先に 1 回見ておく。`checkConditionalExpression` (checker.go:11193) は、部分式を `checkExpressionEx` で再帰的に検査して、全体の型を作る。T-If は `then` と `else` が同じ型を要求するが、tsgo は 2 つの型のユニオンにする。ノードの種類ごとの振り分けは `checkExpressionWorker` (checker.go:7951)。

## 健全性とは何か (読んで分かったこと)

詳しくは読書メモの TAPL Ch 8 の節と Effective TypeScript 項目 7 の節を参照。

- **TAPL の定義**: 型安全性 (safety、健全性と同義) = 進行 (progress) + 保存 (preservation)。型が付いた項は、値であるか 1 歩進める (進行)。1 歩進んでも同じ型が付く (保存)。TAPL Ch 8 の 8.3 で確認済み
- **性質の持ち主は型システム**。個々の項ではなく、型付け規則と評価規則の組が、型の付いたすべての項について 2 つの定理を満たすかを問う
- **Effective TypeScript の見方**: 型を値の集合 (ドメイン) として考える (項目 7)。代入可能・`extends`・サブタイプは、すべて部分集合。本文は健全性の定義を直接与えていない。「実行時の値が静的型のドメインの中にあること」を健全性とみなすのは、この見方を保存に結びつけた読み手の解釈
- **TS は健全性を設計目標にしていない** (#9825)。理由は、健全性・使いやすさ・複雑さのトレードオフ、JS の実行時の挙動が健全化に敵対的、証明可能な健全性は「正しいプログラムを書く」ことの代用指標、の 3 つ。設計は実コードで試して決めた。型システムの振る舞いを変えるフラグ (モード) は避けてきた
- 健全さは精度ではなく正確さ。`Math.random()` は `[0, 1)` でなく `number` でよい
- 調整用のつまみが `strictNullChecks`、`noUncheckedIndexedAccess` など

チェッカーの外の穴 (読んでも何も出ない): 不正確な型定義 (lib.d.ts / @types)。宣言ファイルが実際の挙動と食い違う例として、commander.js#2683 の `alias()` がある。

## 穴ごとの計画 (進める順)

各節の構成: 問題 (再現コード) → 入口 → 読んで答えたいこと → TAPL Ch 8 の見方 (どちらが壊れているか。私の解釈で、読んで確かめる) → 他実装の発想 (任意) → パッチ (任意)。

### 1. インデックスアクセス

**問題**

```ts
type IdToName = { [id: string]: string };
const ids: IdToName = { '007': 'James Bond' };
const agent = ids['008']; // string (実行時は undefined)

const xs = [1, 2, 3];
xs[3].toFixed(1); // noUncheckedIndexedAccess を ON にすると Error
```

- 本の対処: `noUncheckedIndexedAccess`、または型に `undefined` を明示
- 本が言う「`for (const x of xs)` や `xs.map` は ON でも OK」は、要素アクセスを経由しないから

**入口**: tsgo checker.go `getIndexedAccessType` (27703)、tsrs checker_13.rs:2354 (`no_unchecked_indexed_access` は 2590 付近)。設計意図は PR #39560。

**読んで答えたいこと**: ON のとき `undefined` を足す分岐はどこか。どんなアクセスで足されて、どんなアクセスでは足されないか (`for...of` や `map` は足されない理由)。

**TAPL Ch 8 の見方**: `string` 型なのに値が `undefined`。標準形補題が壊れ、`agent.length` で進行も壊れる。

**他実装の発想**: tsz は TSZ5001 (未実装)。`sound: true` で `noUncheckedIndexedAccess` を強制する案だが、ドキュメント内で不整合 (Layer A では強制、ステータス表では「自動では有効にならない」)。Rust は範囲外を実行時の境界チェックで止め、型で扱うなら `v.get(i)` が `Option` を返す。Haskell は `Data.Map.lookup` が `Maybe` を返し、`head` や `!!` は部分関数として目立たせる。

**パッチ**: 不要。既存の `noUncheckedIndexedAccess` を ON にして測るだけ。測定の手順 (取得、実行、エラー数の集計) を作る練習になる。

### 2. メソッドの bivariance

**問題**

```ts
class Parent {
  foo(x: number | string) {}
  bar(x: number) {}
}
class Child extends Parent {
  foo(x: number) {}          // 本来は反変違反。メソッドなので OK になる
  bar(x: number | string) {} // こちらは健全
}
const p: Parent = new Child();
p.foo('string'); // 実行時にクラッシュするが型エラーなし

// 関数プロパティにすると strictFunctionTypes で弾かれる
class P2 { foo = (x: number | string) => {} }
class C2 extends P2 { foo = (x: number) => {} } // Error
```

- 本の対処: メソッドでなく関数プロパティにする。eslint の `method-signature-style`
- 本の訳者補記: メソッドが bivariant のまま残ったのは、厳密にすると既存の代入可能性ルールが広範囲に壊れる (`Hen[]` → `Animal[]` や `Element` → `EventTarget` のサブタイプなど) から。strictFunctionTypes は 2017 年の TS 2.6

**入口**: tsgo relater.go `compareSignaturesRelated` (1494)。`strictVariance` (1535) は、`strictFunctionTypes` が ON で、ターゲットが `KindMethodDeclaration` / `KindMethodSignature` / `KindConstructor` のどれでもないときだけ真。`strictVariance` が偽のときは、ソースからターゲットを先に試し (1606)、だめならターゲットからソースを試す (1609)。この「両方向を試す」のが bivariance の正体。あわせて、コールバック引数の再帰 (1604、`StrictCallback` / `BivariantCallback`)。tsrs relater_1.rs:3071 / `strict_variance` 3143。設計意図は PR #18654。

**読んで答えたいこと**: 条件がメソッドと一緒にコンストラクタも除外するのはなぜか。コールバック引数の再帰は、どう効くか。

**TAPL Ch 8 の見方**: `p.foo('string')` は、`number` を受ける関数に `string` を渡す。標準形補題が壊れ、進行が壊れる。

**他実装の発想**: tsz は動いている。`RelationPolicy` の `strict_subtype_checking` が `disable_method_bivariance = true` にする。`@tsz-bivariant` という例外の注釈は計画のみ。Rust は関数の引数の型について反変を正しく計算し、メソッドだからといって例外にしない。Haskell は変性が問題になる場面が少なく、`coerce` ではロール (nominal / representational / phantom) で許される変換を指定する。

**パッチ (任意)**: `strict_variance` の条件から、メソッド / メソッドシグネチャ / コンストラクタの除外を外す (スイッチ付き)。#57783 で Ryan が試した結果は、コアの lib が壊れる、メソッドの引数も反変にすると配列が不変になり `const arr: number[] = [];` すら書けなくなる (右辺が `never[]` になるため)、だった。やるなら、lib 由来とユーザーコード由来を分けて数える。

### 3. 配列の共変

**問題**

```ts
class Animal {}
class Hen extends Animal {}
class Fox extends Animal {}
function addFoxOrHen(animals: Animal[]) {
  animals.push(Math.random() > 0.5 ? new Fox() : new Hen());
}
const henhouse: Hen[] = [new Hen()];
addFoxOrHen(henhouse); // henhouse に Fox が入る

// 対処: readonly にすると push が存在しないのでエラー
function f(animals: readonly Animal[]) { animals.push(new Hen()); } // Error
```

- 本の対処: `readonly Animal[]` で受ける、`Animal` を返す関数にする

**入口**: tsgo relater.go `getVariances` (1320)、checker.go:977 の `arrayVariances = []VarianceFlags{VarianceFlagsCovariant}` (決め打ち)。tsrs relater_1.rs:2832 / checker.rs:1687。`getVariances` が配列に `arrayVariances` を返す分岐の条件 (タプルも含むか) を読む。

**仮説**: 本は直接の原因を「`push` がメソッドで bivariant」とする。しかしコードには `arrayVariances` の決め打ちもある。穴 2 を締めただけで `Hen[]` → `Animal[]` が弾かれるのか、`arrayVariances` で素通りするのかは未確認。tsz の資料は「メソッド bivariance を切っても配列は塞がらない」と書いていて、これは仮説の傍証。

**読んで答えたいこと**: 仮説の検証。`Map` / `Set` / `Promise` などの他の可変コンテナの変性は、メソッドの引数から計算されるのか。

**TAPL Ch 8 の見方**: `Hen[]` と型が付いた配列に `Fox` が入る。値が型のドメインの外に出る。保存が壊れる。

**他実装の発想**: tsz は TSZ2001。検出コード (`SoundLawyer.check_array_covariance()`) は書かれているが、どこからも呼ばれていない。原則は「可変なジェネリックコンテナは、証明できない限り不変」。Rust は `&mut T` が `T` について不変、`&T` が共変で、`push(Fox)` の穴は起きない。可変な `Animal[]` ≈ `&mut Vec<Animal>`、`readonly Animal[]` ≈ `&[Animal]`。Haskell は値が既定で不変で、可変配列 (`IOArray`、`STArray`) だけが問題になり、`ST` モナドが可変な状態の漏れを型で防ぐ。

**パッチ (任意)**: `arrayVariances` を不変にする。`readonly` 配列だけ共変を残す案も。typescript-go#3576 で Ryan が「不変な配列は大量の問題を持ち、過去に何度も検討した」と答え、#18770 を参照している。やるなら #18770 を先に読む。

### 4. 型アサーション

**問題**

```ts
function logNumber(x: number) { console.log(x.toFixed(1)); }
const hour = new Date().getHours() || null; // number | null
logNumber(hour);            // Error
logNumber(hour as number);  // OK だが実行時に失敗しうる
```

- 本の対処: 条件文で絞り込む (`if (hour !== null)`)

**入口**: tsgo checker.go `checkAssertionDeferred` (12577) と comparable relation、tsrs checker_06.rs:2779。comparable relation は assignable より緩い。どこが緩いかを読む。

**読んで答えたいこと**: comparable は assignable と何が違うか。`as unknown as T` が通る理由。

**TAPL Ch 8 の見方**: `as` は実行時チェックなしに、型だけを書き換える。型は `number`、値は `null` になりうる。標準形補題が壊れる。

**他実装の発想**: tsz は TSZ1011 (未実装)。許可は拡大のみ、絞り込み方向は認識されたリファインメントか理由付きの suppression が必要、という案。Rust は `as` を安全な変換に限り、危険な `transmute` は `unsafe`。絞り込みは `Any::downcast_ref` のように実行時に検査して `Option` を返す。Haskell は `unsafeCoerce` と、実行時に検査する `Typeable` の `cast`。

**パッチ (任意)**: comparable を締めて、横方向のキャスト (`unknown` / `any` 経由を含む) だけを弾く。増えたエラーの内訳 (どの形の `as` が多いか) が見どころ。

### 5. any

**問題**

```ts
function logNumber(x: number) { console.log(x.toFixed(1)); }
const num: any = 'forty two';
logNumber(num); // エラーなし。実行時に例外
```

- 本の対処: any のスコープを限定する、`unknown` を使う

**入口**: tsgo relater.go `isSimpleTypeRelatedTo` (206)、tsrs relater_1.rs:282。any が素通りする条件 (assignable / comparable のとき) を読む。`unknown` 側がどう弾かれるかも並べて見る。

**読んで答えたいこと**: any が、ソースにもターゲットにもなれるのはどの分岐か。

**TAPL Ch 8 の見方**: `any` は、すべての型の値を受け取り、すべての型に代入できる。どんな代入も通すので、型と値の食い違いを防げない。

**他実装の発想**: tsz は動いているが一部。`strict_any_propagation` で深さ 1 以上の any だけを制限し、トップレベルの any は素通り。反省点として、`RelationPolicy::from_flags()` が `FLAG_STRICT_FUNCTION_TYPES` から「strict any」を推測していて、tsz 自身が「間違った抽象化」と書いている。宣言境界の射影 (プロトタイプ、issue #8533) は、読み取り位置の any を `unknown` にする案。Rust は `dyn Any` で、使うにはダウンキャストを明示する。Haskell は `Dynamic` と `fromDynamic` (`Maybe`)。

**パッチ (任意)**: 弱い版 (深さ 1 以上の any だけ) から、強い版 (トップレベルも締める) へ段階的に。

### 6. オプションプロパティ / freshness (構造的型付け)

**問題**

```ts
interface Person { name: string }
interface PossiblyAgedPerson extends Person { age?: number }
const p1 = { name: 'Serena', age: '42 years' };
const p2: Person = p1;              // OK (余剰プロパティチェックは対象外)
const p3: PossiblyAgedPerson = p2;  // OK だが age の実体は string
console.log(p3.age?.toFixed(1));    // 実行時エラー
```

- 本の対処: プロパティ名を具体的にする (`ageInYears`)。コードでなく命名

**入口**: 代入可能性判定のプロパティ比較 (tsgo relater.go の `propertiesRelatedTo` あたりを grep、未調査)。freshness を落とす処理は `getRegularTypeOfObjectLiteral` (checker.go:28959、tsrs checker_14.rs:645) と `ObjectFlagsFreshLiteral` (types.go:699、この rev では未確認)。呼び出し元を grep して、どこで freshness が切れるかを読む。

**読んで答えたいこと**: freshness は、どの時点で、何のために落とされるか。

**TAPL Ch 8 の見方**: `Person` に代入した時点で、`age` の型は型から消える。再び `PossiblyAgedPerson` に代入すると、`age` が `string` なのに `number | undefined` の型が付く。幅の部分型付けの組み合わせで、保存が壊れる。

**他実装の発想**: tsz は sticky freshness (動いている。一時的)。`widen_freshness()` の呼び出しを複数の箇所でスキップする。tsz 自身が「`--sound` で有効なのは一時的なミスマッチ」と書いている。Rust と Haskell は構造体が公称型で、幅の部分型付けがない。

**パッチ (任意)**: freshness を落とす呼び出しの一部をスキップして、変数に代入した後も fresh を保つ。あわせて `exactOptionalPropertyTypes` を ON にして測る (パッチ不要)。

### 7. フロー解析 (関数呼び出しで絞り込みが消えない。最後)

**問題**

```ts
interface FunFact { fact: string; author?: string }
function processFact(fact: FunFact, processor: (fact: FunFact) => void) {
  if (fact.author) {
    processor(fact);
    console.log(fact.author.blink()); // OK だが processor が delete すると実行時エラー
  }
}
processFact({ fact: '...', author: 'Botanists' }, f => delete f.author);
```

- 本の対処: 引数を `Readonly` にして変更しない
- 本の理由: 「ほとんどの関数はパラメータを変更しないから、また JS でよくあるパターンだから」絞り込みを無効化しない

**入口**: tsgo flow.go `getFlowTypeOfReference` (77)、tsrs flow.rs:112。フローノードに「呼び出し」で絞り込みを捨てる種類があるか (たぶんない) を確認する。設計意図は #9998 (open、コメント多数)。比較対象として oxc の `oxc_cfg` / `oxc_semantic` を読む。

**読んで答えたいこと**: 絞り込みがどのフローノードで作られ、どの時点で捨てられるか。関数呼び出しが、フローグラフにどう現れるか。

**TAPL Ch 8 の見方**: 別名と変更の組み合わせが、絞り込んだ型を壊す。`fact.author` は `string` のはずが、`processor` が `delete` して `undefined` になる。保存が壊れる。TAPL Ch 8 の範囲には、フロー依存の型付けはない。

**他実装の発想**: tsz は触らない (「Sound Mode が変えないもの」に、制御フローによる絞り込みと `let` の widening が明記)。Rust は別名と変更の組み合わせを借用検査で禁止する。Haskell は値が既定で不変なので、パターンマッチ後に値が変わらない。

**パッチ (任意)**: 一番重い。案 A (関数呼び出しの引数に渡したオブジェクトの、プロパティの絞り込みを捨てる)、案 B (パラメータ型が `readonly` / `Readonly` でないときだけ捨てる)、案 C (関数呼び出し全般で捨てる。上限を測る用)。

## パッチと測定をやるとき

面白そうな穴だけ。やるかどうかは、その穴のコードを読んでから決める。穴 1 はパッチ不要。

- 対象: tsrs (rev `fc384313`)。tsgo を読んで意味を確認し、tsrs で書き換える
- **既定の挙動は変えない。** パッチごとにスイッチ (環境変数かコンパイラオプション) を付ける。tsrs は tsgo と同じ診断を目標にしているので、既定を変えると自前のテストも壊れる。特例を判定する箇所にフラグの分岐を足す形が基本 (`strictFunctionTypes` による `strictVariance` の切り替えと同じ形)
- 弱い版から段階的に締めて、各段階でエラー数を測る (どこで急に壊れるかを見る)
- 実プロジェクト: `bench/projects.json` に定義されたもの (vscode、mikro-orm、mui-docs、supabase-studio、webpack、nuxt など)。取得と実行の手順は `bench/README.md` にあるはずだが未読 → **最初にここを読む**
- 実行: `tsrs -p <project> --noEmit --incremental false --pretty false`。最初は `--fast` なし (追加キャッシュが結果に影響しうるため)
- 測るもの: パッチなしとありでの、プロジェクトごとのエラー数の差。増えたエラーを何件か手で読み、真の不具合か誤検出かに分ける。ベースラインのエラー数が 0 のプロジェクトを優先する
- 関係判定の結果はキャッシュされる。スイッチが変わるとキャッシュにも反映が必要 (tsz も `RelationCacheKey` などで同じ点を課題にしていた)。tsc でも関係ごとにキャッシュが別なので、パッチごとに確認する
- TS 側の過去の試行 (#57783、typescript-go#3576) の結果が公開されている穴は、それを読んでから、測る範囲を決める

## 読書計画の外の穴 (メモ)

穴 1〜7 を片付けた後の拡張候補。出典は tsz の SOUND_MODE.md の候補一覧と、本のまとめ。未調査で、tsgo / tsrs のどこに当たるかも分かっていない。

| 穴 | 内容 | 出典 |
|---|---|---|
| readonly の別名経由の変更 | `readonly` プロパティを持つ型を、mutable なプロパティを持つ型に代入できる (参照レベルでしか守れない限界もある) | tsz TSZ3003 |
| 分割アクセサ | getter / setter で型が違うオブジェクトを、インターフェースに代入したとき、setter が受ける型を確認しない | tsz TSZ3004 |
| `Object.assign` | 上書きするキーの型を、プロパティごとに確認しない | tsz TSZ3005 |
| index signature の暗黙の推論 | TS が緩く推論する index signature を明示させる | tsz TSZ3007 |
| rest 引数の bivariance | `(...args: any[]) => void` を、具体的なシグネチャの上位型として受ける | tsz TSZ4001 |
| `void` 戻り値の例外 | `() => void` のコールバックに、戻り値のある関数を渡せる | tsz TSZ4002 |
| `Function` 型 | 具体的な関数型を、不透明な `Function` に代入できる | tsz TSZ4003 |
| `includes` / `has` | 要素と無関係な型で呼べてしまう (`unknown` を受ける形にする案) | tsz TSZ5002 |
| `reduce` の初期値なし | 空配列で実行時エラー。空でないと証明された型 (`[T, ...T[]]`) を要求する案 | tsz TSZ5003 |
| タプルの長さの変更 | 長さを変えるメソッドが使える | tsz TSZ5004 |
| enum と number | 暗黙に相互変換できる (TS 5.0 以降で一部は直っている) | tsz TSZ6001 |
| プリミティブのボクシング | プリミティブを `Object` や `{}` に代入できる | tsz TSZ6002 |
| 条件型の分配 | 変えると実コードが壊れるので、tsz はデフォルトを維持し、診断だけを足す案 | tsz TSZ7001 |
| 交差型 | 不可能な交差型を `never` に潰す案。TS が意図的に残しているものもある | tsz TSZ7002 |
| `switch` の網羅性 | 網羅性を型の保証として使えるか | tsz TSZ8001 |
| `this` の共変 | `this` が反変の位置にあるクラスが共変になる | tsz TSZ2003 |
| 非 null アサーション `!` | 型アサーション (穴 4) の別形 | tsz TSZ1021 |
| 明確な代入のアサーション `!:` | クラスフィールドの初期化を保証しない | tsz TSZ1022 |
| catch 変数 | `useUnknownInCatchVariables` で `unknown` にする (tsc の既存フラグ) | tsz TSZ1031 |
| 型述語 `is` | 型述語が不健全性の原因として本のまとめに挙がっている。例なし | 本 |

### 最近報告された個別の不健全性 (microsoft/TypeScript の issue、2026-11-09 時点)

2026 年に報告された不健全性の issue。`gh search issues` で件名と状態を取り、本文は冒頭だけ読んだ。**中身の正確な読みは未確認**。読むときは issue を開いて確かめる。

| issue | 状態 | 内容 | 読書との関係 |
|---|---|---|---|
| #63454 | closed (Design Limitation、2026-05) | 再帰型を使うと `1 \| 2 \| 3` を `1 \| 2` に代入できる | 関係判定 (relater) の穴。深さ制限などの打ち切りが原因かは未確認 |
| #63708 | closed (Possible Improvement、2026-09) | ユニオンの分配で、ジェネリクスの制約を破れる | 条件型の分配 (tsz TSZ7001) に近い |
| #63709 | open (2026-08 起票) | オプションプロパティが `undefined` を含むため、文字列インデックスシグネチャで制約された型パラメータの制約を破れる | 穴 1 に近い |
| #64390 | closed (2026-09-22) | TypeScript 7 (ネイティブ版) が、関係判定の深さ制限を超えると、型の不一致を黙って受け入れる。TS 5.9 では `TS2321` が出る | tsgo の `relater` の話で、読む対象そのもの。tsrs でも同じ挙動か確かめる価値がある |
| #64495 | open | `Record<K1, T> & Record<K2, U>` が `Record<K1 \| K2, T & U>` に代入できる | 交差型と関係判定 |

あわせて、動いている議論: #9998 (穴 7 の入口)、#202 (非構造的な型の一致、穴 6 と関係)。
