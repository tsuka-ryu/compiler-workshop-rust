# tsc を読みながら「健全性とは何か」を理解する読書計画

これは読書計画。読んで分かったことや実測値は、別のメモに書く。材料は、Effective TypeScript (穴の具体例)、tsz の Sound Mode、TAPL。

## 目的

1. **「健全性とは何か」を、tsc のコードを読みながら理解する。**
   - 背景: 最近、Bun や scriptc など、TypeScript を AOT コンパイルに使う流れがある。そこで「健全性が大事」と言われる
   - 問い: 型検査が通ったプログラムに期待してよいことは何か (AOT で何が変わるかは今回の対象外)
2. **tsz より強い健全化を、tsrs のパッチで試す。** tsz は実用の線で止めている。その線の先 (配列の共変、any、フロー解析など) を、自分で塞いで、実コードでの壊れ方を測る
3. **tsc (tsgo) のコードを読むきっかけにする。** checker.go は巨大なので、健全性の穴を入口に、関係判定 (relater.go) を中心に、checker.go と フロー解析 (flow.go) を必要な分だけ読む

## 読む対象と進め方

- 対象: `microsoft/TypeScript` の `tsc/internal/checker/` (Go 移植版 = tsgo)
- 読んだ時点の rev: `f7973164a` (2026-11-08 時点の main)。行番号は rev に依存するので、ずれたら関数名で探す
- 副読本 (同じ関数を Rust で読む。**tsrs は tsgo の Rust 移植なので、関数名が `snake_case` になるだけで対応する**):
  - `oxc-project/tsrs` (rev `fc384313`、0.23.0 の準備コミット): `crates/tsrs_checker/`。対応表は `docs/CHECKER.md`、移植方針は `docs/PORTING.md`。Go の `checker.go` (約 32,600 行) は `checker_01.rs` … `checker_15.rs` に Go の行範囲で分割、`relater.go` は `relater_1.rs` / `relater_2.rs`、`flow.go` は `flow.rs`。Rust の練習を兼ねて読むなら、こちらを並べて読む
  - `oxc-project/oxc` (rev `9f5297f781`): **型チェッカーはない** (`crates` に checker 系の crate なし)。健全性の穴を直接読む対象ではなく、「型チェッカーなしで型を扱う」側の参考。読むのは `oxc_cfg` (制御フローグラフ) と `oxc_semantic` (スコープ・シンボル)。tsgo の binder (`tsrs_binder` に対応) が作るフローグラフと、穴 7 のフロー解析の比較対象にする。`oxc_cfg` と `oxc_semantic` の中身は未読 (対応関係は想定で、読んで確かめる)
- 型理論の本は TAPL だけ。穴ごとの計画の「理論」に章を振ってある。読まないもの: 難しそうなので外した Rustonomicon の variance の章、RustBelt、Oxide、Wadler の線形型論文。今回は関係が薄い HM 型推論、型クラス、依存型、row polymorphism、Typechecker Zoo、NLL / Polonius のブログ。保留: Dunfield & Krishnaswami の双方向型検査
- 設計意図の履歴は移植後の git blame では追えない。PR / issue を読む (番号は記憶ベース、読む時に実在を確認する)
  - strictFunctionTypes: #18654
  - noUncheckedIndexedAccess: #39560
  - CFA のトレードオフ: #9998
- tsz の資料 (他実装の健全モード。穴ごとの計画に発想を混ぜてある):
  - https://tsz.dev/sound-mode/ (2026-11-08 取得)
  - https://github.com/tsz-org/tsz/blob/main/docs/plan/SOUND_MODE.md (同日取得。`Last Updated: April 2026`、Status: Experimental / Partially Implemented)。約 95,000 文字のうち末尾の約 16,000 文字 (Phase 1 後半〜Phase 5、非目標、トレードオフ) は要約経由でしか読めていない。tsz のコードとの突き合わせはしていない

1 つの穴あたりの進め方:

1. 5 行程度の再現コードを 1 つ書く
2. 関係判定の穴は `isTypeRelatedTo` にブレークポイントを置いて追う (通読はしない)。インデックスアクセスとフロー解析は、それぞれの入口から追う
3. tsz・Rust・Haskell の発想を参考に tsrs にパッチを当て、実プロジェクトでどれだけ壊れるかを測る (「なぜ健全にしなかったか」を量で実感する)。穴 1 はパッチ不要で、既存フラグを ON にするだけ

順番は下の「穴ごとの計画」の番号順 (測定手順が楽な順。最後のフロー解析が一番重い)。

## 健全性とは何か (読む前の理解)

- **TAPL (型理論) の定義**: 型安全性 (safety) = 進行 (progress) + 保存 (preservation)。型が付いた式は行き詰まらず (進行)、評価しても型が保たれる (保存)。TAPL Ch 8 の 8.3 (章番号は記憶ベース、要確認)
- **Effective TypeScript の見方**: 型を値の集合 (ドメイン) として考える (項目 7)。代入可能・`extends`・サブタイプは、すべて部分集合。項目 7 の本文は健全性の定義を直接与えていない (読んで確認済み)。「実行時の値が静的型のドメインの中にあること」を健全性とみなすのは、この見方を TAPL の「保存」に結びつけた読み手の解釈
- 健全さは精度ではなく正確さ。`Math.random()` は `[0, 1)` でなく `number` でよい
- TypeScript は健全性を設計目標にしていない。表現力・健全性・利便性のトレードオフで、利便性と既存 JS ライブラリのモデリングを優先している。調整用のつまみが `strictNullChecks`、`noUncheckedIndexedAccess` など
- 本のメッセージ: 関数のパラメータを変更しない (`readonly` / `Readonly`)。親子クラスのメソッド宣言を一致させる
- Rust の線引き: 安全な部分と `unsafe` の部分を、**言語の構文で分ける**。安全な部分だけなら、未定義動作は起きないと主張し、`unsafe` の中だけは書いた人が責任を負う。標準ライブラリの `unsafe` は、安全な API に包んで隠す。FFI は `unsafe` で、信頼境界がコード上に見える。RustBelt が、核となる部分と標準ライブラリの一部について、健全性を証明している (「型は検証条件」という考え方で、`unsafe` を含むライブラリも検証できる)。ただし言語全体の公式な形式仕様はなく、コンパイラの健全性バグは、既知の課題として管理されている。穴ごとの対応は、各節の「他実装の発想」。記述は記憶ベースで未確認
- Haskell の線引き: 安全でない操作は、名前 (`unsafeCoerce`、`unsafePerformIO`) で明示する。Safe Haskell という拡張で、モジュールを「安全」「信頼」「危険」に分けて、危険なものの利用を制限できる。型付きの中間言語 (Core) を持ち、最適化の各段階の後で型を再検査して、コンパイラ自身のバグを検出する。値が既定で不変なので、別名と変更の問題が起きにくい。部分関数や `undefined` は残っている。記述は記憶ベースで未確認
- tsz の線引き: 「数学的な証明ではない」し、サードパーティの `.d.ts` が正しくなるわけでもない。第一版で約束するのは小さなリストだけ。宣言ファイル (`.d.ts`、デフォルト lib、`node_modules`) は「信頼境界」で検査しない。第一版の束 (Layer A) は、tsc の既存フラグ `useUnknownInCatchVariables`、`noUncheckedIndexedAccess`、`exactOptionalPropertyTypes` を暗黙にオンにし、「明示的 any 禁止」と「メソッド bivariance 無効」を足したもの。**健全モードの大半は tsc がすでに持つフラグの束ね**で、新規はメソッド bivariance と any だけ (ただし `noUncheckedIndexedAccess` の扱いは tsz のドキュメント内で不整合。穴 1 を参照)

チェッカーの外の穴 (読んでも何も出ない): 不正確な型定義 (lib.d.ts / @types)。

## 実験の共通のやり方

穴 1 つにつき 1 つのパッチ。「なぜ健全にしなかったのか」を、実プロジェクトでの壊れ方の量で実感するのが目的。

- 対象: tsrs (rev `fc384313`)。tsgo を読んで意味を確認し、tsrs で書き換える
- **目指すのは tsz より強い健全化。** tsz・Rust・Haskell は参考で、なぞることが目的ではない。ただし一気に最強まで締めず、弱い版から段階的に締めて、各段階でエラー数を測る (どこで急に壊れるかを見る)
- **既定の挙動は変えない。** パッチごとにスイッチ (環境変数かコンパイラオプション) を付ける。tsrs は tsgo と同じ診断を目標にしているので、既定を変えると自前のテストも壊れる
- 実プロジェクト: `bench/projects.json` に定義されたもの (vscode、mikro-orm、mui-docs、supabase-studio、webpack、nuxt など)。取得と実行の手順は `bench/README.md` にあるはずだが未読 → **最初にここを読む**
- 実行: `tsrs -p <project> --noEmit --incremental false --pretty false`。最初は `--fast` なし (追加キャッシュが結果に影響しうるため)
- 測るもの (結果は別のメモに書く): パッチなしとありでの、プロジェクトごとのエラー数の差。増えたエラーを何件か手で読み、真の不具合か誤検出かに分ける
- ベースラインのエラー数が 0 のプロジェクトを優先する。増えた分がそのままパッチの影響になる
- 関係判定の結果はキャッシュされる。スイッチが変わるとキャッシュにも反映が必要 (tsz も `RelationCacheKey` などで同じ点を課題にしていた)。tsc でも関係ごとにキャッシュが別なので、パッチごとに確認する
- tsz は、特例を止めるフラグを関係判定に渡す形で、Sound Mode を実現している。tsrs でも、特例を判定する箇所にフラグの分岐を足す形が基本になる (`strictFunctionTypes` による `strictVariance` の切り替えと同じ形)

## 穴ごとの計画 (進める順)

各節の構成: 問題 → 理論 (基礎的な理論があれば。TAPL) → tsrs パッチ (入口 → 仮説 (ある穴のみ) → 他実装の発想 (tsz / Rust / Haskell) → パッチ計画)。「理論」は、章番号が記憶ベースで、TAPL の内容から結びつけた解釈も含む。読んで確かめる。

### 1. インデックスアクセス (最初。パッチ不要で測定手順を作る)

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

**理論** (TAPL Ch 8 の 8.3、章番号は記憶ベース): 型安全性 = 進行 + 保存。`undefined.toFixed(1)` は評価が行き詰まる式で、「型が付いたのに行き詰まる」= 進行が壊れている例。型が `string` なのに実行時の値が `undefined` なのは、保存が壊れている言い方でもある。TS では「範囲外アクセスは型が付く」という選択で、健全性を手放している。

**tsrs パッチ**

**入口**: tsgo checker.go `getIndexedAccessType` (27703)、tsrs checker_13.rs:2354 (`no_unchecked_indexed_access` は 2590 付近)。ON のとき `undefined` を足す分岐を読む。設計意図は PR #39560。

**他実装の発想**

- **tsz**: TSZ5001 (未実装)。`sound: true` で `noUncheckedIndexedAccess` を強制する案。ただしドキュメント内で不整合 (Layer A では強制、ステータス表では「自動では有効にならない」)。「tsc の既存フラグで足りるかも」とも書いている。
- **Rust**: 範囲外のインデックスは、型ではなく**実行時の境界チェック**で止める (`v[i]` は panic)。型で扱いたいときは `v.get(i)` が `Option` を返す。`noUncheckedIndexedAccess` は、すべての `v[i]` を `v.get(i)` にするのに近い。TS は型が消えるので、実行時の境界チェックは足せない。型で `undefined` を足す方法しかない。
- **Haskell**: Haskell は、範囲外や空を扱う関数を、API で分けている (`Data.Map.lookup` は `Maybe` を返す。`head` や `!!` は部分関数で、失敗すると例外)。部分関数が既知の弱点で、新しい GHC は `head` / `tail` に警告を出す (要確認)。「安全な版を標準にして、危ない版は名前や警告で目立たせる」やり方。

**パッチ計画**: パッチ不要。既存の `noUncheckedIndexedAccess` を ON にして、実プロジェクトで測るだけ。ここで測定の手順 (取得、実行、エラー数の集計) を確立する。

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

**理論** (TAPL Ch 15 の部分型、章番号は記憶ベース): 関数型の部分型付けは、引数について反変、戻り値について共変 (S-Arrow)。`(number | string) => void` は `number => void` の部分型で、逆ではない。`strictFunctionTypes` は、この規則どおりにチェックする。メソッドの bivariance は、この規則を引数について片側だけ緩めている (どちら向きでも許す)。

**tsrs パッチ**

**入口**: tsgo relater.go `compareSignaturesRelated` の `strictVariance` (1535 行付近)、tsrs relater_1.rs:3071 / `strict_variance` 3143。条件は `KindMethodDeclaration` / `KindMethodSignature` のほかに `KindConstructor` も除外している (「メソッドだけ」より少し広い)。あわせて読む: コールバック引数の再帰 (tsgo 1604 行付近) の `StrictCallback` / `BivariantCallback`。設計意図は PR #18654。

**他実装の発想**

- **tsz**: 動いている。`RelationPolicy` の `strict_subtype_checking` が `disable_method_bivariance = true` にする (`SubtypeChecker`)。エラーコードは標準のまま。直し方の案として、サブクラスの引数を広げる / 関数プロパティ構文にする / イベントマップのパターンにはオーバーロードを使う、を挙げている。`@tsz-bivariant` という例外の注釈は計画のみ。宣言ファイル側のメソッドを関数プロパティに機械的に書き換える (`foo(x: T): R` → `foo: (x: T) => R`) overlay の案もある。
- **Rust**: Rust は、関数の引数の型について**反変を正しく計算する** (`fn(T)` は `T` について反変)。メソッドだからといって例外にしない。構造的な部分型が無く、サブタイプ関係は主にライフタイムに限られるので、緩める動機がそもそも少ない。TS は既存コードとの互換のために、例外を残した。
- **Haskell**: Haskell は、クラスベースの部分型付けを持たないので、関数引数の変性の問題が、そもそもほとんど出ない (多相で代替する)。変性が出るのは `coerce` のときで、「ロール」(nominal / representational / phantom) で型引数ごとに許される変換を指定する。`newtype` の自動導出が不健全になった問題を、ロールで直した。

**パッチ計画**: `strict_variance` の条件から、メソッド / メソッドシグネチャ / コンストラクタの除外を外す (スイッチ付き)。差分は小さいが、lib の宣言 (配列の `push` など) にも効くので、エラーは相当増えそう (仮説)。増えたエラーのうち、lib 由来とユーザーコード由来を分けて数える。

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

**理論** (TAPL Ch 13 の参照、Ch 15 の 15.5 あたり。章番号は記憶ベース): 読み取り専用の配列は共変で安全。**可変な参照 (Ref) は不変**でなければならない。`Ref Hen` を `Ref Animal` として使うと、`Animal` を書き込めてしまい、保存が壊れる。TS の `Animal[]` は可変なので、本来は不変にすべきところを、共変にしている。`readonly Animal[]` なら共変でよい、というのが本の対処の理論的な裏付け。

**tsrs パッチ**

**入口**: tsgo relater.go `getVariances` (1320) / checker.go `arrayVariances` (977、`{Covariant}` の決め打ち)、tsrs relater_1.rs:2832 / checker.rs:1687。`getVariances` が配列に `arrayVariances` を返す分岐の条件 (タプルも含むか) を読む。

**仮説**: 本は直接の原因を「`push` がメソッドで bivariant」とする。しかしコードには `arrayVariances` の決め打ちもある。穴 2 のパッチだけで `Hen[]` → `Animal[]` が弾かれるのか、`arrayVariances` で素通りするのかは未確認。

**他実装の発想**

- **tsz**: TSZ2001。検出コード (`SoundLawyer.check_array_covariance()`) は書かれているが**どこからも呼ばれていない (dead code)**。live の `SubtypeChecker` は sound mode でも配列に対して共変のまま、と明記 = **メソッド bivariance を切っても配列は塞がらない** (仮説の傍証)。原則は「可変なジェネリックコンテナは、証明できない限り不変」。直し方: 読み取り専用の API は `readonly` で受ける、生産側は `readonly` を返す、変更が要るならコピーする。
- **Rust**: `&mut T` は `T` について**不変**、`&T` は共変。TAPL の Ref の話そのまま。`&mut Vec<Hen>` を `&mut Vec<Animal>` として渡せないので、`push(Fox)` の穴は起きない。対応づけると、可変な `Animal[]` ≈ `&mut Vec<Animal>` (不変にすべき)、`readonly Animal[]` ≈ `&[Animal]` (共変でよい)。
- **Haskell**: Haskell は、値が既定で不変なので、配列の書き込みの穴は、可変配列 (`IOArray`、`STArray`) に限られる。可変な型は、型引数について不変 (部分型がないので自明に不変)。`ST` モナドは、`runST` のランク 2 多相で、可変な状態が外に漏れないことを型で保証する。「可変なものを型の仕組みで閉じ込める」やり方。

**パッチ計画**:
- まず測る: 穴 2 のパッチだけで `Hen[]` → `Animal[]` が通るかを確認する (仮説の検証)。通るなら、穴 2 だけでは塞がらないことが確定する
- 次に締める: `arrayVariances` を不変にする。`readonly` 配列だけ共変を残す案も試す。タプルも同じ扱いにするかを決める
- あわせて見る: `Map` / `Set` / `Promise` などの他の可変コンテナ。これらの変性は、メソッドの引数から計算されるはずなので、穴 2 のパッチで自然に不変になるのではないか (仮説)。配列だけ決め打ちなので別扱い
- 測るもの: 穴 2 のパッチ単独、穴 2 + 穴 3 の両方、の 2 通りのエラー数。穴 3 の分が、純粋に配列の共変を締めた影響になる

### 4. 型アサーション

**問題**

```ts
function logNumber(x: number) { console.log(x.toFixed(1)); }
const hour = new Date().getHours() || null; // number | null
logNumber(hour);            // Error
logNumber(hour as number);  // OK だが実行時に失敗しうる
```

- 本の対処: 条件文で絞り込む (`if (hour !== null)`)
- 本のまとめには `is` (型述語) も不健全性の原因として挙がっているが、本文に例がない。別の入口か要確認

**理論** (TAPL Ch 15 の 15.5 のアスクリプションとキャスト、章番号は記憶ベース): 上向きのキャスト (拡大、upcast) は常に安全。下向きのキャスト (絞り込み、downcast) は、型システムだけでは安全にならず、実行時のチェックが要る。TAPL の言語では下向きのキャストに実行時チェックがあるが、TS の `as` は**実行時チェックがない**。つまり `as` の絞り込み方向は、型システムの外の約束。tsz の案 (拡大のみ許可) は、この区別に沿っている。

**tsrs パッチ**

**入口**: tsgo checker.go `checkAssertionDeferred` (12577) + comparable relation、tsrs checker_06.rs:2779。comparable relation は assignable より緩い。どこが緩いかを読む。

**他実装の発想**

- **tsz**: TSZ1011 (未実装)。規則案: 横方向のキャスト (`cat as unknown as Dog`) は `unknown` / `any` 経由でも禁止。許可は拡大のみ (ソースがターゲットに代入可能)。絞り込み方向 (`string as "hello"`) は、認識されたリファインメント (`instanceof`、`in`、判別式、`asserts` 関数、スキーマパーサー) か、理由付きの suppression が必要。suppression は `// @tsz-unsound TSZ1011: 理由` の形で、コード必須・理由必須、対応するエラーがない suppression 自体がエラー (`@ts-expect-error` と同じ思想)。
- **Rust**: `as` は、数値の変換など安全なものだけを許す。型の解釈を変える危険な操作 (`transmute`) は `unsafe` で、書いた人が責任を負う。型を絞り込みたいときは、`Any::downcast_ref` のように、**実行時に検査して `Option` を返す**。TS に当てはめると、`as` の絞り込み方向は `transmute` に近い (検査なしで型だけ変える)。`instanceof` や型ガードは `downcast_ref` に近い。
- **Haskell**: Haskell は、型の強制的な変更を `unsafeCoerce` という名前で明示する。安全な絞り込みは `Typeable` の `cast` で、実行時に検査して `Maybe` を返す。Rust の `downcast_ref` と同じ形。

**パッチ計画**: comparable を締めて、横方向のキャスト (どちらにも代入できない型へのキャスト、`unknown` / `any` 経由を含む) だけを弾く。拡大と、絞り込み方向は通す (最初は suppression なし)。実コードの `as` は多いので増え方が大きそう。増えたエラーの内訳 (どの形の `as` が多いか) が見どころ。

### 5. any

**問題**

```ts
function logNumber(x: number) { console.log(x.toFixed(1)); }
const num: any = 'forty two';
logNumber(num); // エラーなし。実行時に例外
```

- 本の対処: any のスコープを限定する、`unknown` を使う

**理論** (TAPL Ch 15 の 15.4 の Top と Bottom、章番号は記憶ベース): Top はすべての型の上位 (`unknown`)、Bottom はすべての型の下位 (`never`)。`any` は、Top のようにすべての型の値を受け取り、Bottom のようにすべての型に代入できる。**Top と Bottom を兼ねる型は、部分型の推移性を壊し、どんな代入も通す**。tsz の「`any` を bottom として扱わない (any は any / unknown にしか代入できない)」という規則は、この区別そのもの。

**tsrs パッチ**

**入口**: tsgo relater.go `isSimpleTypeRelatedTo` (206)、tsrs relater_1.rs:282。any が素通りする条件 (assignable / comparable のとき) を読む。`unknown` 側がどう弾かれるかも並べて見る。

**他実装の発想**

- **tsz**: 動いているが一部。`strict_any_propagation` → `AnyPropagationMode::TopLevelOnly` で、深さ 1 以上の any だけを制限し、トップレベルの any はまだ素通り。目標の規則は、ユーザーコードで明示的な `any` を禁止 / any を bottom として扱わない (any は any / unknown にしか代入できない) / 宣言ファイル由来の any は許容 / any へのメンバアクセス・呼び出しは専用エラー。反省点として、`RelationPolicy::from_flags()` が `FLAG_STRICT_FUNCTION_TYPES` から「strict any」を推測しており、tsz 自身が「間違った抽象化」と書いている。

  宣言境界の射影 (プロトタイプ、`--soundDeclarationProjection`、issue #8533): 宣言ファイル由来の any を、使う側から見て安全にする案。型を**極性つきで**射影し、読み取り位置の any は `unknown` に、書き込み位置は寛容なままにする。
  - `parse(text: string): any` → `(text: string) => unknown`
  - コールバック引数は二重反転で読み取り位置: `onValue(cb: (value: any) => void)` → `(cb: (value: unknown) => void) => void`
  - mutable な配列・タプルと index signature は射影しない (共変だけの書き換えだとそこが不健全になるため。穴 3 とつながる)
  - 条件型や mapped 型などは辿らないので、DT 流のメタプログラミング経由の any は漏れる
  - 本の「不正確な型定義」を、チェッカー側から塞ぐ試み
- **Rust**: any に近いのは `dyn Any` で、使うには**ダウンキャストを明示し、実行時に検査される**。`any` のように黙ってどの型にもなる値は存在しない。TS の `unknown` + 型ガードが `dyn Any` + `downcast_ref` に近い。`any` は、`transmute` と同じく「検査なしで型を変える」ものとして、`unsafe` 側に分類するのが Rust 流。
- **Haskell**: Haskell には、`any` に当たる型がない。動的な値は `Dynamic` で、`fromDynamic` が `Maybe` を返す。ただし `undefined` (bottom) がどの型にも入る穴があり、部分関数と同じく許容されている。「どの型にも黙ってなる値」は、bottom の形でだけ残っている。

**パッチ計画**:
- まず弱い版: 深さ 1 以上の any だけを制限する (`TopLevelOnly` 相当)。影響が広すぎる可能性があるので、まず「ユーザーコード由来の any が絡む代入だけ」に絞り、宣言ファイル由来は除外する
- 次に強い版: トップレベルの any も締める (any は any / unknown にしか代入できない)。any へのメンバアクセスと呼び出しを専用エラーにする案も試す
- どこまで締めるかは「any 問題」(下の「読書計画の外の穴」) と合わせて決める。宣言境界の射影は、ここでは扱わず、同じ節の .d.ts の問題として残す

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
- 型がシールされていない (余分なプロパティを許す) ことが前提

**理論** (TAPL Ch 15 の 15.2 のレコードの幅の部分型、Ch 16-17 のアルゴリズム的な部分型付け。章番号は記憶ベース): レコードは、フィールドが多い方が部分型 (幅の部分型付け)。`{name, age}` は `{name}` の部分型なので、`Person` に代入できる。ここまでは健全。問題は、`Person` に代入した後で、**捨てた `age` の型について何も言えなくなる**のに、`PossiblyAgedPerson` (`age?: number`) に再び代入できること。幅の部分型付けでは、「余分なフィールドは何でもよい」ので、`age` が `string` でも、型としては `Person` のままで矛盾しない。構造的部分型で、オプションプロパティを足した別の型に戻せる、という組み合わせが穴を作る。

**tsrs パッチ**

**入口**: 代入可能性判定のプロパティ比較 (tsgo relater.go の `propertiesRelatedTo` あたりを grep、未調査)。freshness を落とす処理は `getRegularTypeOfObjectLiteral` (tsgo: checker.go:28959、tsrs: checker_14.rs:645) と `ObjectFlagsFreshLiteral` (types.go:699)。関数・フラグの存在は確認済み。どこで呼ばれて freshness が切れるかは未読 (呼び出し元を grep する)。

**他実装の発想**

- **tsz**: sticky freshness (動いている。一時的)。`widen_freshness()` の呼び出しを、変数チェック、呼び出し結果の計算、for ループの要素推論、状態の保存、識別子の計算、型エイリアス/変数エイリアスの解析など複数のチェッカー内の値フローの箇所で**スキップ**して、`FRESH_LITERAL` を保持する。tsz 自身が「`--sound` で有効なのは一時的なミスマッチ」と書いており、設計上は pedantic 層 (Layer B) の扱い。もう一案は `Exact<T>` にだけ効かせる案 (freshness は `Exact<T>` への代入のときだけ意味を持つ)。別の関連として `exactOptionalPropertyTypes` (TSZ3008、Layer A で暗黙オン) もある。
- **Rust**: 構造体は**公称型**で、幅の部分型付けがない。余分なフィールドを持つ値が別の型として渡ることが、そもそも起きない。TS は構造的型付けを核にしているので、この穴は設計上避けにくい。塞ぐなら freshness のような補助的な印で、局所的に防ぐしかない。
- **Haskell**: Haskell も、構造体 (レコード) は公称型で、幅の部分型付けがない。余分なフィールドを持つ値が別の型で渡ることが、起きない。拡張可能レコードはライブラリで別に扱う。

**パッチ計画**: freshness を落とす呼び出しの一部をスキップして、変数に代入した後も fresh を保つ (tsz の発想)。どの呼び出し元を残すかの判断が要るので、まず呼び出し元を全部洗い出す。あわせて `exactOptionalPropertyTypes` を ON にして測る (パッチ不要)。

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
- 本の理由: 「ほとんどの関数はパラメータを変更しないから、また JS でよくあるパターンだから」絞り込みを無効化しない (issue #9998 と照合する)

**理論**: TAPL に、フロー依存の型付け (絞り込み) を扱う章はなかったはず (記憶ベース、要確認)。近いのは Ch 13 の参照の話で、**別名 (エイリアス) と変更**の組み合わせが、型の保存を壊す、という点。`fact` が `processor` にも渡され、`processor` が `fact.author` を変更しうる、という別名と変更の問題。理論的に健全にするには、「呼び出しで変更されうるか」の情報 (副作用や所有権) が型に要る。TS はそれを持たず、「ほとんどの関数はパラメータを変更しない」という前提で絞り込みを残している。

**tsrs パッチ**

**入口**: tsgo flow.go `getFlowTypeOfReference` (77)、tsrs flow.rs:112。フローノードに「呼び出し」で絞り込みを捨てる種類があるか (たぶんない) を確認する。比較対象として oxc の `oxc_cfg` / `oxc_semantic` を読む。

**他実装の発想**

- **tsz**: **触らない**。「Sound Mode が変えないもの」に、制御フローによる絞り込みと `let` の widening が明記されている。理由は書かれていない (tsc の #9998 と同じ理由、が自然な推測)。tsz の一覧にも出てこない。
- **Rust**: 別名と変更の組み合わせを、**借用検査で禁止する** (可変な参照は同時に 1 つだけ、可変な参照がある間は他の参照を使えない)。「絞り込みの後で、別名経由で値が変わる」ことが、そもそも起きない。TS には所有権も借用もないので、同じ手は使えない。近い折衷は、パラメータが `Readonly` かどうかを、`&T` と `&mut T` の違いのように読むこと (案 B)。`Readonly` なら呼び出しで変更されない、そうでなければ変更されうる、という前提で絞り込みを捨てるかを決める。
- **Haskell**: Haskell は、値が既定で不変なので、パターンマッチで型が絞り込まれたあとに、その値が変わることがない。絞り込みが壊れない理由は、不変性。変更は `IORef` や `ST` など、型に現れる仕組みに閉じ込める。TS の `readonly` / `Readonly` は、これを慣習で近似するもの。案 B の「`Readonly` なら絞り込みを保つ」は、この考え方に沿っている。

**パッチ計画**: 一番重い。調査から始める。
- 調査: フローノードに「呼び出し」で絞り込みを捨てる種類を足せるか、既存の仕組みで足りるかを、flow.go / flow.rs を読んで決める
- 案 A: 関数呼び出しの引数に渡したオブジェクトについて、プロパティの絞り込み (`fact.author`) を、呼び出しの後で捨てる。最も強く、壊れ方も大きい
- 案 B: 引数のパラメータ型が `readonly` / `Readonly` でないときだけ捨てる。本の対処 (変更しないなら `Readonly`) と整合し、`Readonly` で書かれたコードは壊れない。型から判断できる分、案 A より弱いが、実用的
- 案 C: 関数呼び出し全般で、プロパティの絞り込みを捨てる。案 A より広い。上限を測る用
- 測るもの: 案 A / B / C のエラー数。「絞り込みがどれだけ消えると、実コードが壊れるか」を量で見る (issue #9998 の議論との答え合わせ)

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
| `reduce` の初期値なし | 空配列で実行時エラー。空でないと証明された型 (`[T, ...T[]]`) を要求する案。`T \| undefined` は誤った型付けとして退けている | tsz TSZ5003 |
| タプルの長さの変更 | 長さを変えるメソッドが使える | tsz TSZ5004 |
| enum と number | 暗黙に相互変換できる (TS 5.0 以降で一部は直っている) | tsz TSZ6001 |
| プリミティブのボクシング | プリミティブを `Object` や `{}` に代入できる | tsz TSZ6002 |
| 条件型の分配 | 変えると実コードが壊れるので、tsz はデフォルトを維持し、診断だけを足す案 | tsz TSZ7001 |
| 交差型 | 不可能な交差型を `never` に潰す案。TS が意図的に残しているものもある | tsz TSZ7002 |
| `switch` の網羅性 | 網羅性を型の保証として使えるか | tsz TSZ8001 |
| `this` の共変 | `this` が反変の位置にあるクラスが共変になる | tsz TSZ2003 |
| 非 null アサーション `!` | 型アサーション (穴 4) の別形。要確認 | tsz TSZ1021 |
| 明確な代入のアサーション `!:` | クラスフィールドの初期化を保証しない | tsz TSZ1022 |
| catch 変数 | `useUnknownInCatchVariables` で `unknown` にする (tsc の既存フラグ。測るだけ) | tsz TSZ1031 |
| 型述語 `is` | 型述語が不健全性の原因として本のまとめに挙がっている。例なし | 本 |

### any 問題: どこまで締めるか

穴 5 のパッチを決めるための論点。実コードでの any の量を、先に測る (どの形の any が多いか)。

- **any の入り口は複数ある**: 明示的な `any` の注釈 / 暗黙の any (推論やライブラリ由来、`JSON.parse` の戻り値など) / 宣言ファイルから流れ込む any / `any[]` や `Promise<any>` の中の any
- **締め方の選択肢**:
  - 禁止: 自分のコードに書かせない (tsz の第一版の目標)
  - 汚染 (taint): any は any / unknown にしか代入できないようにして、any の値が他の型に紛れ込まないようにする (TAPL の Top / Bottom の話)
  - 境界で `unknown` にする: 入ってくる位置 (戻り値、読み取り位置) だけ変換する (tsz の射影)
- **決めること**: 禁止と汚染のどちらを基本にするか。明示的な any と、宣言ファイル由来の any を、同じ扱いにするか分けるか。逃げ道 (`unknown` + 型ガード、検証ライブラリ) を許すか
- **Rust / Haskell の発想**: Rust の `dyn Any` も Haskell の `Dynamic` も、使うには実行時に検査して `Option` / `Maybe` を受け取る。`any` を `unsafe` 側に分類する、という割り切り方もある
- **見るもの**: tsz のドキュメントにある `TSZ1001` (明示 any) / `TSZ1002` (宣言由来の any、later) / `TSZ1003` (汚染された値の危険な操作) の分け方

### .d.ts の信頼問題: 宣言ファイルをどこまで信じるか

穴 2・5 のパッチの影響にも関わる。健全性の保証は、「`.d.ts` が実行時の挙動を正しく表している」という仮定の下でしか成り立たない。

- **仮定の確認**: TAPL の保存は、閉じた体系の話で、外部の宣言 (FFI) は扱わないはず (解釈、要確認)。TS の `.d.ts` は、外部の JS を信じて書いた型。この境界の外側の誤りは、チェッカーでは見つけられない
- **選択肢**:
  - 信頼する: そのまま使う (tsz の第一版。宣言ファイルは検査しない)
  - 極性つきの射影: 読み取り位置の `any` を `unknown` にして、書き込み位置はそのままにする (tsz のプロトタイプ。mutable な配列とタプル、index signature は射影しない)
  - overlay: 個別の宣言を直す (`JSON.parse(): unknown`、`Response.json(): Promise<unknown>`、メソッドを関数プロパティに書き換えるなど)
  - 宣言ファイル自体を検査する: 自分のプロジェクトが出す `.d.ts` だけ (tsz の `soundCheckDeclarations` 案)
  - 境界でランタイム検証: Zod のようなスキーマで、入ってくる値を検証する (型の外の話)
- **穴 2 との関係**: メソッド bivariance を締めると、lib の宣言 (配列の `push` など) にも効く。穴 2 の測定で、lib 由来のエラーとユーザーコード由来のエラーを分けて数える。lib 由来を除外するかどうかは、ここで決める
- **Rust / Haskell の発想**: Rust の FFI は `unsafe` で、信頼境界がコード上に見える。Haskell も `foreign import` と Safe Haskell で、信頼する部分を明示する。外部を信じる部分と、安全な部分が、構文や拡張で分かれている。TS の `.d.ts` は、境界が見えない。tsz の suppression や「宣言ファイルは信頼境界」は、この見える化の試みと読める
- **決めること**: どこまでを信頼境界として認めるか。境界の外にあるもの (`node_modules` の型、DOM の lib) を、チェッカー側で補正するか、ランタイム検証に任せるか
