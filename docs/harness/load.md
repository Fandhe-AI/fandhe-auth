# 計測・負荷のハーネス: 指標と値の区別の下書き

`TASK-12`（計測・負荷のハーネス。MS-1）の 1/10（#112）の下書き。ここでは指標の定義と、値の区別の規則だけを置く。
仕様は spec の `TASK-12` で引く（本文は写さない）。この文書は下書きで、文書名・節立て・語・規則は仮置き、人が確定する。
記録は事実の出力であり、合否の判定でも spec の「確定」でもない（`ACPT-7`）。
欄名・キー名・ファイル形式は 2/10（下の「結果の記録の形式」）で定めた。機材と構成の自動での取得は 7/10 で扱い、同じ文書に節を足す。

## 目的と範囲

本体の処理の時間・件数を測り、負荷をかけて記録するハーネス。ハーネスは合格の値を持たず、合否を付けない。

含めないもの:

- 障害注入（`TASK-13`）
- 負荷の見積もり（`TASK-15`）・合格の値の導出（`TASK-21`。`AUDP-29`、`D-2026-10-17-36`）
- 結合試験の起動の枠（`TASK-10`。`./integration.md`）

## 指標の定義

### 標本・区間・時計・単位

- 標本は 1 回の試行（1 要求・1 操作）の所要時間。
- 区間（始点と終点）には名前を付け、指標ごとに区間の名前を併記する。クライアント側の応答の時間とハンドラの中の時間は別の区間にする（PoC-12 の記録の分け方を ID で引く。数値は写さない）。
- 時間の差は単調な時計（`std::time::Instant`）で測る。壁時計は差に使わず、実行の開始・終了の時刻（UTC・RFC 3339）だけに使う。
- 内部の単位は整数のナノ秒。表示の単位は下の「結果の記録の形式」で定める。単位は値と必ず組で書く。

### p95・p99

- 最近順位法（nearest-rank）を使う。昇順に並べた n 個の標本 x(1)…x(n) について、k = ⌈p/100 × n⌉（1 ≤ k ≤ n）とし、pXX = x(k)。補間しない。
- n = 0 のときは値を出さない（「なし」。0 と書かない）。
- n が小さいと pXX が最大と同じになる。百分位の値には必ず n を併記する。
- 既知の入力の例（4/10 の試験の期待に使う）: 1〜100 の 100 個 → p95 = 95・p99 = 99・最大 = 100。1〜10 の 10 個 → p95 = 10・p99 = 10。
- 比べる参考として p50 と最大も同じ方法で出す（書き手の読み）。

### 最悪の回

spec に定義は無く、`TASK-12` の 1 行にだけ出る。次の 2 つの語に分けて定義する。

- 最大: 1 回の実行の中の全標本の最大値。その試行の通し番号を併記する。
- 最悪の回: 同じ条件で繰り返した実行（「回」）のうち、比べる指標が最も悪い 1 回。その回の通し番号と各指標を記録する。比べる指標の既定は仮置きで p99。繰り返しが 1 回のときは、その回が最悪の回になる。

`TASK-12` の「最悪の回」がどちらを指すかは未決（下記）。確定まで両方を記録する。

### 件数

次を別々に数える。

- 予定の試行数
- 実行した試行数
- 成功
- 失敗（種類ごと）
- 時間切れ
- 除外（暖機など）

不変条件: 実行 = 成功 + 失敗 + 時間切れ。除外は実行の外に別に数え、黙って落とさない。
時間の分布は成功の試行だけで集計する（仮置き）。失敗の時間は別の分布にし、時間切れは値を持たないので件数だけで数える。

### 並行数

- 設定の並行数: 同時に処理中にしてよい試行の上限（閉ループのワーカーの数）。
- 実測の最大同時実行数: 実行中に観測した、同時に処理中の試行の最大。
- 上の 2 つは別の値で、両方を記録する。1 秒あたりの送出の件数（開ループの率。`TASK-15` の見積もりの単位）とは別の語。閉ループか開ループかは 5/10 で決める。
- 段階ごとに並行数を変える場合（8/10）は、段階ごとに設定の並行数を持つ。

## 値の区別

spec の用例は ID で引き、数値は写さない。

### 要件の値

`04-behavior/` の項目や決定（`D-…`）で要件として決まった値。性能の目標値は持ち越しで今は無い（`04-behavior/README.md`、`TASK-15`・`TASK-21`、`AUDP-29`）。このため現時点の記録では、要件の値の欄は「未決」になる。

### 初期値

決定で決まったが、初期値と明示され、見直しの予定がある値（用例: `AVLB-1`・`ACPT-13`・`D-2026-10-17-62` の (1)）。

### PoC のパラメータ

PoC・QP で試験の条件として置いた値。要件の値としては検討中のまま（`04-behavior/README.md` の ID 規約 (a)）。

### 上の 3 つに当たらない値（書き手の読み。仮置き）

- 試験の条件: ハーネスの実行の設定（並行数・回数・試行数・暖機の件数）。件数は試験の入力の大きさで、要件の値ではない。
- 測定値: 測った結果。
- 未決: 出典が無い値。
- 他所有者の値（Hub 等）: 写さず、出典 ID だけを持つ（`D-2026-10-15-21`）。

### 規則

1. 記録に載せる値は、すべて区分と出典（ID）を持つ。
2. 区分は呼び出し元が明示で渡す。既定値を置かず、ハーネスが推測しない。
3. 要件の値・初期値は出典 ID が必須。無ければ「未決」にする。
4. 測定値を要件の値・初期値に上げない。区分を変えるのは人である（`ACPT-7`。`THRT-29` の「測って決める」の扱い）。
5. PoC の数値を既定値・比べる値として置かない。
6. 判定の欄は空けておき、人が記入する。

区分の欄名・表し方は下の「結果の記録の形式」の 2 で定める。

## 測定の条件

機材・構成・並行数は記録の別の欄にする。具体の欄は下の「結果の記録の形式」の 5 で定め、自動での取得は 7/10 で扱う。
機材の名前・ホスト名・内部のアドレス・ホームのパスは書かない。機材は種類・CPU の数・メモリ量などの抽象的な属性で表す（`SECR-11`・`SECR-23`、04-behavior/README.md の (d)）。

## 結果の記録の形式

`TASK-12` の 2/10（#113）。この節は形式の定義だけで、型・出力の実装はまだ無い（後段。下の「未決・人の確認待ち」）。
形式の識別子・キー名・トークン・文言はすべて仮置きで、人が確認する。記録は事実の出力で、合否の判定でも spec の「確定」でもない（`ACPT-7`）。

### 1. 形式の基本

- 形式の識別子は `fandhe-auth-load-record/1`。形式を変えたら末尾の版を上げる（`fandhe-auth-it-run-record/1`・`fandhe-auth-it-fault-record/1` と同じ流儀）。
- UTF-8・LF・1 行 1 項目の `key: value`。キーは ASCII 固定で、値の中のトークンは半角空白 1 つで区切る。
- 時刻は UTC・RFC 3339（秒）。実行の記録（`./integration.md` の「実行の記録の形式」）とは、この記録の `run_record_file`（実行の記録のファイル名。`it-run-<起動時刻の秒>-<pid>-<通し番号>.txt` の形で、pid と通し番号を含み同じ秒に起動した複数のハーネスも区別できる）で一意に結ぶ。`run_started_at`（`started_at` と同じ値）は補助で、秒精度のため単独では結ばない。
- 出力先は呼び出し元が渡し、追跡ツリーの外に置く（試験では `CARGO_TARGET_TMPDIR`）。既存のファイルは上書きしない。
  ファイル名の形は `load-run-<起動時刻の秒>-<pid>-<通し番号>.txt`。

### 2. 区分（値の区別）の表し方

区分は値を持つ各行に `class=<トークン>` と `refs=<…>` で明示する。キーから暗黙に決めない（「値の区別」の規則 1）。
適用除外は値を持たないメタデータの行だけ（`format:`・`run_record_file:`・`run_started_at:`・`recorded_at:`・`note:`・`judgment:`）。下の 3〜5 の各行の形式は、すべて `class=`・`refs=` を含む。

| トークン | 区分 |
| --- | --- |
| `requirement` | 要件の値 |
| `initial` | 初期値 |
| `poc_parameter` | PoC のパラメータ |
| `test_condition` | 試験の条件 |
| `measured` | 測定値 |
| `undecided` | 未決 |
| `external` | 他所有者の値 |

出典は `refs=<ID>[,<ID>…]` で書き、無いときは `refs=none` と書く。区分ごとの規則:

- `requirement`・`initial`・`poc_parameter`: 出典が必須。出典の無い `requirement`・`initial` は記録を作る時点で誤り（fail-closed）にし、黙って `undecided` に落とさない。呼び出し元が `undecided` を明示で渡す。
- `undecided`: 値を持たない（`value=none`）。出典は任意。
- `external`: 値を常に持たない（`value=none`）。出典は必須（`D-2026-10-15-21`。他所有者の値を写さない）。
- `test_condition`・`measured`: 出典は固定の印（`refs=harness_config`・`refs=measured`）。

区分に既定値は無く、ハーネスが推測しない。測定値を要件の値・初期値に上げる経路を作らない。PoC の数値を既定値・比べる値として置かない（「値の区別」の規則 2・4・5）。

比べる値の行は `value: <名前> class=<区分> value=<整数|none> unit=<単位> refs=<…>` の形にする。
性能の目標値は持ち越し（`AUDP-29`、`TASK-15`・`TASK-21`）のため、p99 の目標の行は現時点で `class=undecided value=none` になる。

判定の行 `judgment:` は常に出し、値は常に空にする。人が記入する欄で、ハーネスは書かない（規則 6）。

### 3. 単位と指標

- 時間は整数のナノ秒で、単位はキーに入れる（`p95_ns` 等）。小数・単位の換算はしない。人向けの表示の単位は 10/10 の手順書で扱う。
- 単位のトークンは `ns`・`count`・`per_s`。値は整数だけにする。
- 指標の行:
  `metric: <段階>.<回> interval=<区間名> class=measured n=<n> p50_ns=<v|none> p95_ns=<v|none> p99_ns=<v|none> max_ns=<v|none> max_trial=<通し番号|none> refs=measured`
  - 百分位の値には必ず `n` と区間名が並ぶ。`n=0` のときは百分位・最大・`max_trial` をすべて `none` にする（0 と書かない）。
  - 不変条件: `p50 <= p95 <= p99 <= max`、`n <= success`。`metric` の分布は成功の試行だけで作る（成功のみの集計の規則は `metric` に限る）。
  - 失敗の時間の分布は同じ形の `failure_metric:` 行（`class=measured`・`refs=measured` を持つ）で別に出し、標本は失敗の試行だけで作る。不変条件は `n <= failure`（`n <= success` は適用しない）。時間切れは件数だけで数える。
- 最悪の回の行: `worst_run: <段階> by=<比べる指標> interval=<区間名> run=<回の通し番号|none> class=measured refs=measured`
  - 比べる指標は呼び出し元が明示する（既定の仮置きは `p99_ns`）。同じ値が並んだときは通し番号の小さい回を採る。どの回も `n=0` のときは `run=none`。
  - 「最大」（`metric` 行の `max_ns`・`max_trial`）と「最悪の回」の両方を出す。

### 4. 段階・回・区間の配置

- `stage: <段階の通し番号> class=test_condition refs=harness_config concurrency_configured=<n> repetitions=<r> trials_planned=<k> warmup=<w>`
- `run: <段階>.<回> class=measured refs=measured concurrency_observed_max=<n> executed=<e> success=<s> failure=<f> timeout=<t> excluded=<x>`
- `failure_kind: <段階>.<回> class=measured refs=measured kind=<種類名> count=<c>`
- 件数の不変条件: `executed = success + failure + timeout`、`failure = Σ failure_kind の count`。除外は `executed` の外に別に数える。
- 設定の並行数（`stage` 行）と実測の最大同時実行数（`run` 行）は別の欄にする。閉ループのときは `concurrency_observed_max <= concurrency_configured` を不変条件の候補とする（5/10 の決定に依る）。
- 段階が 1 つでも `stage: 1 …` を出す。段階を変える実行（8/10）は同じ行の並びを繰り返す。

### 5. 測定の条件（機材・構成・並行数）

- 機材は抽象的な属性だけで、`condition_machine: <属性> <値|unknown> class=test_condition refs=harness_config` の形にする。属性は 6 つで、すべて常に出し、取れないものは `unknown` と明示する。
  - `cpu_count`・`memory_mib`・`arch`・`os_family`
  - `virtualization`（`none`・`vm`・`container`・`unknown`）
  - `build_profile`（`debug`・`release`・`unknown`）
- 自動での取得は 7/10 で、この段は欄だけを定める。
- ホスト名・機材の名前・内部のアドレス・ホームのパス・利用者名の欄は作らない（`SECR-11`・`SECR-23`、04-behavior/README.md の (d)）。
- 構成は `condition_component: <種類> started real|started stub|not_started class=test_condition refs=harness_config` の形にし、語彙と 4 種は実行の記録の `component` 行と同じにする。
  stub を含むときは、stub の合格を実物の合格に数えない旨の `note:` を出す（`A-2026-10-28-9`・`ACPT-7`）。
- `condition_loop: closed|open|undecided class=test_condition refs=harness_config`（5/10 が決めるまでは `undecided` を許す）。並行数は 4 の `stage` 行に書く。
- 版の情報（revision・rustc の版）は実行の記録側にあり、`run_record_file` で結ぶ。この記録には重複して持たない。

### 6. 差し込み対策（出してはいけないものと守り方）

- 許可する文字と語彙はフィールドの種類ごとに分ける。
  - 名前（区間名・値の名前・キー）: ASCII の `[a-z0-9_]`・1〜64 文字。
  - 構成要素名（`condition_component` の種類名）: 実行の記録の `component` 行と同じ固定語彙（`Database`・`Host`・`HubStub`・`ConsoleStub`）だけ。大文字を含み、上の名前の規則は適用しない。
  - 固定列挙値（`unknown`・`none`・`vm`・`release`・`closed`・`open`・`undecided`・`started`・`not_started`・`real`・`stub` 等）: フィールドごとに定めた語彙の完全一致だけ。語彙外は誤りにする。
  - 数値フィールド（`value=`・`*_ns`・`n=` 等）: 整数だけ。値を持たないときの `none` と、未取得の `unknown` は、定めたフィールドにだけ許す。
  - 利用者指定の名前: 作らない（自由文の欄を作らない）。必要になったら 1 つずつ人が語彙と文字を決める。
  - `run_record_file`: `it-run-<秒>-<pid>-<通し番号>.txt` の形（ASCII の英数字・`-`・`.` だけ・1〜64 文字）。
- 出典の ID は ASCII の英数字・`-`・`_`だけ・1〜64 文字。1 行に並べる数の上限を置く（仮置きで 8）。
- 自由文の欄を作らない（値はすべて上のフィールド別の規則で検証する）。
- 改行・制御文字・行区切り（U+2028・U+2029）を含む入力は、記録を作る時点で誤りにする（fail-closed）。黙って省かない。
- 誤りは種類だけを持ち、値・パス・`io::Error` の文言を載せない。誤りの文言は英語（`D-2026-11-20-13`）。
- 設定の値・接続情報・ポート・パス・ホスト名・パスワードは記録に入れない（`SECR-11`・`SECR-23`・`SECR-26`）。環境変数・設定ファイルは読まない（`DPOL-32`）。
- 後段の試験では、目印（canary）の文字列が記録と `Debug` に出ないことを確かめる。

### 7. 見本の記録

1/10 の既知の入力（1〜100 の 100 個を ns として扱う）の合成の値だけを使う。時刻は epoch 付近の合成の値で、PoC の数値・要件の値は置かない。

```text
format: fandhe-auth-load-record/1
run_record_file: it-run-0-1-1.txt
run_started_at: 1970-01-01T00:00:00Z
recorded_at: 1970-01-01T00:01:00Z
condition_machine: cpu_count unknown class=test_condition refs=harness_config
condition_machine: memory_mib unknown class=test_condition refs=harness_config
condition_machine: arch unknown class=test_condition refs=harness_config
condition_machine: os_family unknown class=test_condition refs=harness_config
condition_machine: virtualization unknown class=test_condition refs=harness_config
condition_machine: build_profile unknown class=test_condition refs=harness_config
condition_component: Database not_started class=test_condition refs=harness_config
condition_component: Host not_started class=test_condition refs=harness_config
condition_component: HubStub started stub class=test_condition refs=harness_config
condition_component: ConsoleStub not_started class=test_condition refs=harness_config
note: stub results are not counted as real results
condition_loop: undecided class=test_condition refs=harness_config
stage: 1 class=test_condition refs=harness_config concurrency_configured=1 repetitions=1 trials_planned=100 warmup=0
run: 1.1 class=measured refs=measured concurrency_observed_max=1 executed=100 success=100 failure=0 timeout=0 excluded=0
metric: 1.1 interval=client_response class=measured n=100 p50_ns=50 p95_ns=95 p99_ns=99 max_ns=100 max_trial=100 refs=measured
worst_run: 1 by=p99_ns interval=client_response run=1 class=measured refs=measured
value: p99_target class=undecided value=none unit=ns refs=none
judgment:
```

`condition_component` の種類名は実行の記録の `component` 行と同じ語彙（`Database`・`Host`・`HubStub`・`ConsoleStub`）で、見本の起動状態は合成の値である。

### 8. 1/10 の記述との関係

この節で、1/10 が 2/10 に回した欄名・キー名・ファイル形式・表示の単位・区分の表し方・機材と構成の欄・差し込み対策を定めた。

## 未決・人の確認待ち

- 文書名・節立て
- 百分位の方法（最近順位法・補間なし）
- 「最悪の回」の読み（標本の最大か、繰り返しの中の最悪の 1 回か）と、比べる指標の既定
- 失敗・時間切れを時間の分布から分ける扱い
- 区分の追加分（試験の条件・測定値・未決・他所有者の値）
- 閉ループか開ループか（5/10）
- 記録への偽の行の差し込みへの対策（下の「結果の記録の形式」の 6 で扱った。内容は人が確認する）
- 結果の記録の形式の型と出力の実装の置き場。提案は 3/10（#114）で、別の crate（例: `crates/load-harness`。仮称）。
  `it-harness` の crate doc が `TASK-12` を別のハーネスとしているため、`it-harness` の module にはしない。crate の追加・名前・許可表の行は人が決める
- 記録の形式の識別子・キー名・トークン・ファイル名の形（すべて仮置き）
- 出典の無い `requirement`・`initial` を誤りにする扱い（`undecided` に落とさない）
- `test_condition`・`measured` の出典を固定の印で表すこと
- 失敗の時間の分布（`failure_metric`）の扱いと、同じ値が並んだときの最悪の回の選び方
- 閉ループのときの並行数の不変条件（5/10 の決定に依る）
- 版の情報を実行の記録側に寄せ、`run_record_file` で結ぶこと
