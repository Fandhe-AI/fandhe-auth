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
3. 要件の値・初期値は、承認一覧の置き場・形・承認の手順が決まるまで記録に載せない（9/10 の実装は常に拒否する。TASK-12・#121）。決まった後は出典 ID が必須で、出典の無い値は誤りにする。黙って「未決」に落とさず、「未決」は呼び出し元が区分として明示で渡す（規則 2）。
4. 測定値を要件の値・初期値に上げない。区分を変えるのは人である（`ACPT-7`。`THRT-29` の「測って決める」の扱い）。
5. PoC の数値を既定値・比べる値として置かない。
6. 判定の欄は空けておき、人が記入する。

区分の欄名・表し方は下の「結果の記録の形式」の 2 で定める。

## 測定の条件

機材・構成・並行数は記録の別の欄にする。具体の欄は下の「結果の記録の形式」の 5 で定め、自動での取得は 7/10 で扱う（下の「機材と構成の記録（7/10）」）。
機材の名前・ホスト名・内部のアドレス・ホームのパスは書かない。機材は種類・CPU の数・メモリ量などの抽象的な属性で表す（`SECR-11`・`SECR-23`、04-behavior/README.md の (d)）。

## 結果の記録の形式

`TASK-12` の 2/10（#113）。この節は形式の定義で、記録のファイル全体の出力は 9/10 で置いた（下の「記録の出力と試し実行（9/10）」。記録時点の文は残し、追記で示す）。
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

- `requirement`・`initial`: 承認一覧の置き場・形・承認の手順が決まるまで（TASK-12。追跡は #121）、記録を作る時点で常に誤り（`ClassNotAllowed`。fail-closed）にし、黙って `undecided` に落とさない。呼び出し元が `undecided` を明示で渡す。決まった後は、出典と整数の値を必須にし、承認一覧の行との完全一致を求める（規則 4）。
- `poc_parameter`: 比べる値として置かない（規則 5。`ClassNotAllowed`）。
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
  - 値の行の `measured`・`test_condition`（型で塞げないもの）: `ComparisonValue::new` は、この 2 区分では整数の値が有ること・出典が空であることだけを検査する。
    渡された数値が `RunEvidence` の実測から出たものか（別の実行・定数・外部の計測でないか）は、型では区別できない（値の行は `RunEvidence` と結び付けず、呼び出し元が渡す。区別する由来の欄も持たない）。
    区別できるのは、`RunSession::run` が作る段階の結果の行（`run:`・`metric:`・`worst_run:` 等）で、時計を引数で受けない経路を通ったものだけである。
    ただし段階の行にも、下の信頼の境界の (1)〜(4)（閉包と申告の一致・測定の条件・実行の記録との結び付け・機材の条件）がかかる。
    値の行の `class=measured` は「呼び出し元が測定値として渡した」という印で、真正性の証明ではない。記録が本物か・値の行の由来が正しいかの判定は人が行う（`ACPT-7`）。
    記録を repo に commit する場所と、人のレビューの手順は未定（人が決める）。
    ハーネスの中に、測定値を要件の値・初期値へ上げる経路は無い（規則 4）。要件の値・初期値は承認の手順が決まるまで常に拒否する（TASK-12・#121）。
  - 記録の出力先（`SECR-23`・`AAUD-30`）: `write_new` の出力先は呼び出し元が渡す追跡ツリーの外で、`make secret-scan`（追跡ファイルの走査）の対象に入らない。
    そのため、書く側で中身を許可した文字・長さに限る（`LoadRecord::new` と `write_new` が検査する）: 通すのは ASCII の英数字と ` _-.:=,/`・LF だけで、1 行 1024 バイト・1 語（空白・`=`・`,` 区切り）64 バイト・全体 1 MiB まで。
    それ以外は `Format` で拒否し、ファイルを作らない。上限の根拠: 1 行の最大は値の行（名前 64 + 出典 8 件）で 700 バイト前後。全体は段階の数 × 繰り返し × 区間・失敗の種類の行の合計で、`StageLimits` と区間の宣言の数で決まる。超える計画は `Format` で拒否する（fail-closed）。
    この網が止めるのは、許可文字の外（記号・非 ASCII・制御文字）と、区切りを含まず 64 バイトを超える語、長すぎる行・全体の回帰だけである。
    次は通る: `key=<64 バイト以下>`、区切りで連ねた値、`+` を含まない base64、ホスト名・`IP:port`・パス、libpq の `key=value` 形式、全体が 64 バイト以下の語になる JWT の形（`.` は区切りでないため、通常の長さの JWT は語の上限で止まる）。
    64 バイト以下の英数字・base64url・16 進の秘密値は形では止められない。各欄の語彙・列挙・整数の検査が止めるのは、渡された語彙の外の名前と、列挙・整数の外の値である。
    ただし区間名（`IntervalName`）・`ValueName`・`RefId` の語彙と信頼済みの一覧は、記録に書く名前と同じ呼び出し元が渡す（`StagePlan::new` の `allowed_intervals`・`ValueName::new` の語彙・`RefId::new` の信頼済みの一覧）。
    呼び出し元が秘密値を名前と語彙の両方に入れれば、語彙の検査も形の網も通り、記録に出る（陽性対照 `matcher_detects_registered_hex_written_through_write_new` はこの経路で書き出し、照合が見つけることを確かめている）。
    そのため名前・出典の欄は、呼び出し元が設定由来の値を名前にしないことに頼っている（ハーネスは止めない）。語彙を入力から独立させる件は #6038（TASK-12）で追跡する。
    漏れは照合の試験（`tests/trial_run.rs`）で確かめる。実行時に作った登録簿（乱数の値と、stub の設定の 3 欄に入れた文字列の全文）の各値を、生・base64・base64url・16 進の形で負荷の記録と it-run の記録の全文に掛け、一致と JWT の形の語が 0 件であること、混ぜた場合に混ぜた形そのものが形ごとに検出されること（合計の件数では見ない）と、`write_new` で書き出した場合に検出されること（陽性対照）、符号化が RFC 4648 の例と一致することを見る。
    これは `AAUD-30`・`AAUD-31` の方法をハーネスの出力に当てはめたもので、`AAUD-30`・`AAUD-31` を満たしたものではない。実環境の走査は Hub（`AOPS-73`）が持つ。
    試験が見る範囲は次のとおり。(1) stub の設定の 3 欄から負荷の記録へ値が流れないことの回帰の検出。(2) 設定が it-run の記録に入る口（`with_hub_stub_record`）を通して書いた it-run の記録に、登録簿の値が現れないことの確認。
    it-run の記録に入るのは欄名（`&'static str`）と印（`ValueMark`）だけで、設定の値が届く経路は無いため、it-run の記録側の陽性対照は置かない（照合の検出力は負荷の記録側の陽性対照で見る）。
    JWT の形は `eyJ….eyJ….` の部分列を、語の頭に限らず本文のどこでも数える。
    署名鍵（`AssertionSigningKey`）の秘密値は登録簿に入れておらず対象外で、未確認のまま持ち越し。
    追跡先: 秘密値を登録簿に入れて走査するのは TASK-500（#1391。`AAUD-30`・`AAUD-31`）、出力の面の秘密値の扱いは TASK-518（#1407。`SECR-23`）。
    出力先を追跡ツリーの中に置いて commit するなら、通常の `secret-scan` の対象になる。
- 誤りは種類だけを持ち、値・パス・`io::Error` の文言を載せない。誤りの文言は英語（`D-2026-11-20-13`）。
- 設定の値・接続情報・ポート・パス・ホスト名・パスワードは記録に入れない（`SECR-11`・`SECR-23`）。環境変数は読まず、設定ファイルを自分では探さない（`DPOL-32`）。
  例外として、`ApprovedValues::load(path)` は呼び出し元が渡したファイル（承認一覧）を形式の検査つきで読む（現在は照合に使わない。TASK-12・#121）。
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

## 計測の部品（3/10）

`TASK-12` の 3/10（#114）。時間を測り、標本と件数を集める部品を `crates/load-harness`（`fandhe-auth-load-harness`。仮称）に置いた。
通常の依存は 0（std だけ）。試験（dev）の依存は 5/10 の節に書く。試験専用で、どの crate からも依存されない（`LIB-2`・`LIB-4`）。

- 時計: trait `MonotonicClock` が原点からの経過を整数ナノ秒（`u64`）で返す。本番は `StdMonotonicClock`（`Instant`）。試験は偽の時計を手で進める。
- 名前: `IntervalName`・`FailureKind` は ASCII の `[a-z0-9_]`・1〜64 文字を作る時点で検査する（上の「差し込み対策」）。
- `RunMeter`（1 回の「回」）: 区間名を宣言して作る（0 件・重複・宣言外は誤り）。`begin_trial` が通し番号（1 始まり）を振り、同時実行の数とその最大を更新する。
- `Trial`: `start`・`end`・`measure` で区間を測る。終点が始点より前・二重の開始・始めていない区間の終了は誤り（黙って 0 にしない）。
  `finish(Outcome)` で `Success`・`Failure(種類)`・`Timeout`・`Excluded` のいずれかで締める。
- 取り出し: `snapshot`・`finish` が `RunSamples` を返す。区間ごとの成功の標本・失敗の標本（通し番号つきのナノ秒）、件数、失敗の種類ごとの件数、実測の最大同時実行数を持つ。
  試行が残っていれば誤り。`executed = success + failure + timeout`・`failure = Σ 種類ごとの件数` を検査してから返す。
- 4/10 に渡すもの: `RunSamples`。百分位・最悪の回の計算と `fandhe-auth-load-record/1` の出力は 4/10 以降。この段では合否を付けず、値の区分も持たない（記録の出力の段で付ける）。

仮置き（人の確認待ち）:

- 締めずに捨てた試行（開いた区間を残した締めの失敗を含む）は、失敗の種類 `unfinished` として数える。不変条件を崩さず、黙って落とさないため。
- 失敗の試行の時間は成功とは別の標本として持つ。時間切れは件数だけ、除外は件数だけで標本を持たない。
- 実測の最大同時実行数は、開いている試行の数の最大。閉ループ・開ループのどちらにも依らない（5/10 の決定を縛らない）。
- 成功で締めるには、宣言した区間をすべて測り終えている必要がある。

## 集計の処理（4/10）

`TASK-12` の 4/10（#115）。`RunSamples` から百分位などを作る純粋な処理を `crates/load-harness` の `aggregate` に置いた。依存は 0。時計・ファイル・環境変数を読まない。

- `summarize_interval`: 区間の標本から `n`・p50・p95・p99・最大・`max_trial` を作る。最近順位法で、k = ⌈p·n/100⌉ を整数演算（`u128`）で求める。入力の並びに依らない。`n = 0` のときはすべて「なし」。
- `summarize_run`: 宣言した区間を呼び出し元から受け、成功の分布と失敗の分布を別々に作る。標本の無い区間は `n = 0` の行にする。宣言が 0 件・重複は `InvalidIntervals`、宣言外の区間は `UndeclaredInterval`。
  件数と実測の最大同時実行数はそのまま写す。
- 不変条件（満たさなければ `InvariantViolated`）: `executed = success + failure + timeout`、`failure = Σ 種類ごとの件数`、各区間の標本数 ≤ 成功（失敗）の件数、p50 ≤ p95 ≤ p99 ≤ 最大。溢れは `CounterOverflow`。
- `worst_run`: 比べる指標（`MetricKey`。既定なし）と区間を明示して、成功の分布が最も悪い回の通し番号（1 始まり）を返す。標本の無い回は候補にしない。全回が標本無しなら `None`、回が空なら `NoRuns`。
- 範囲外: `fandhe-auth-load-record/1` の出力・`class=` の付与、合否（`ACPT-7`）、段階・回を回す処理（8/10）。

仮置き（人の確認待ち）:

- `max_trial` が同値のときは通し番号の最も小さい標本。最悪の回の同値の規則と向きを揃えた。
- 最悪の回は成功の分布だけで比べる。全回が `n = 0` のときは `None`、回が空のときは誤りにする。
- 区間の宣言を呼び出し元から受ける API の形（`RunSamples` を変えないため）。
- 記録の出力を担当する段が 5/10〜10/10 の題から見当たらない。担当の段を人が決める。

## 負荷の生成（5/10）

`TASK-12` の 5/10（#116）。`crates/load-harness` の `generate` に、呼び出し元が指定した並行数のワーカーで対象を繰り返し呼ぶ部品を置いた。
名前・語・値はすべて仮置きで、人が確認する。対象の経路を呼ぶ部分（6/10）・機材と構成の取得（7/10）・段階を変える実行（8/10）は含まない。
合否は付けない（`ACPT-7`）。

- API: `LoadConfig::new(concurrency, trials_planned, warmup)`（既定値なし。並行数は 1〜1024）、`run_closed_loop(meter, &config, target)`、結果 `LoadRun`、誤り `LoadError`（6 種）。
- 閉ループを仮置きで採る（`LoopModel::Closed`。記録の `condition_loop` の材料は `closed`）。各ワーカーは 1 度に 1 試行だけ持つ。
- 暖機の段（`warmup > 0` のときだけ）を回してから本計測の段を回す。2 つの段は重ならない。暖機の試行は結果にかかわらず `Excluded` で締める。
  暖機の試行も通し番号を消費するため、本計測の標本の番号は `warmup + 1` から始まる。
- 本計測で対象が `Excluded` を返したら `InvalidOutcome`（除外は生成器だけが付ける）。
- 停止の規則: 誤りの記録と停止は枠の取得と同じロックで 1 つの代入として確定する（#5984）。記録の後に新しく取得できる枠は 0 件（保証は枠の取得だけ）。記録より前に取得済みの枠の試行は止めない。記録時点で未完了の試行は各ワーカー最大 1 件（設定の並行数が上限）。対象の閉包の panic はワーカー内の `catch_unwind` が捕捉してその場で記録する。join で初めて記録されるのは、ワーカー内で捕捉されずにスレッドを終了させた panic（対象の閉包の外）だけで、検出・記録までに他のワーカーが新しく取得して進める件数の累計は上限を保証しない。立てたワーカーを join してから最初の誤りを返す。panic の中身・`io::Error` の文言は載せない。
- 不変条件: 実測の最大同時実行数 <= 設定の並行数（2/10 の 4 の候補を閉ループで採る）、`executed == trials_planned`、`excluded == warmup`（違えば `CountMismatch`）。
- ハーネスの上で動くこと: 試験（dev）だけ `it-harness`・`hub-stub` に依存する（`tests/on_it_harness.rs`。Docker・ネットワークなし）。許可表の行は人の確認待ち。
- 記録の出力（`fandhe-auth-load-record/1`）を担当する段は未決のまま持ち越す。

## 負荷の対象の呼び出し（6/10）

`TASK-12` の 6/10（#117）。`crates/load-harness` の `target` に、対象の経路を名前・区間と組で呼び、結果を記録の構造体に残す部品を置いた。
名前・語・値はすべて仮置きで、人が確認する。機材と構成の取得（7/10）・段階を変える実行（8/10）は含まない。合否は付けない（`ACPT-7`）。

- API: `TargetName::new`（規則は区間名と同じ）、`TargetSpec::new(name, interval)`、`run_target(clock, &spec, &config, call)`、結果 `TargetRun`（経路・区間・`LoadRun`・`RunSummary`）、誤り `TargetError`（`Load`・`Aggregate`・`Meter`）。
- 呼び出しの結果 `CallResult` から `Outcome` への写し（固定。1 か所）:

  | `CallResult` | `Outcome` |
  | --- | --- |
  | `Ok` | `Success` |
  | `Rejected` | `Failure(rejected)` |
  | `Unavailable` | `Failure(unavailable)` |
  | `TimedOut` | `Timeout` |

  対象から `Excluded` を返す道は作らない（除外は生成器だけが付ける）。
- 対象の中の計測の誤りは、最初の誤りを残し、戻ったら `TargetError::Meter`（元の種類を保つ）で返す。panic・`unwrap` は使わない。
  - 試行の始めに誤りの記録を見た試行は、対象を呼ばずに `Excluded` を返す。記録より前にこの確認を通った試行（並行数が 2 以上のとき）は、記録の後に対象を呼び得る。
  - 停止は生成器の fail-closed に依る。暖機の段は結果を問わず除外で締めるため止まらず、残りの暖機は対象を呼ばずに回って本計測の段へ進む。
    本計測の段では `Excluded` が生成器の `InvalidOutcome` で拒否されて新しい試行が止まる。拒否された試行は締められず `unfinished` に数えられ、ワーカーごとに 1 件起こり得る。
  - 生成器は誤りの記録と停止を枠の取得と同じロックで確定する（#5984。順序は `run_closed_loop` の doc）。
- 結果の記録: `TargetRun` に残すところまで。試行番号と時間の標本が残るのは成功と失敗（種類を問わない）だけで、失敗の種類の内訳・時間切れ・暖機の除外は件数だけが残り、個々の試行には対応付けられない。`fandhe-auth-load-record/1` のファイルへの出力は未提供で、担当の段は未決。
- 対象は stub だけ（`TODO(TASK-716)`。実物の経路は最小の組込み先の完成後）。stub の合格を実物の合格に数えない（`A-2026-10-28-9`・`ACPT-7`）。
  `tests/on_it_harness.rs` で it-harness の上の Hub の stub の JWKS の配信を呼ぶ。stub は Mutex でワーカーを直列化するため、測った時間に意味は無く、試験は件数だけを見る。
- 依存の辺は変えない（通常の依存は 0。試験の依存は 5/10 のまま）。

## 機材と構成の記録（7/10）

`TASK-12` の 7/10（#118）。`crates/load-harness` の `condition` に、測定の条件を記録の行に出す部品を置いた。
名前・語・値はすべて仮置きで、人が確認する。合否は付けない（`ACPT-7`）。

- 自動で取得するのは機材の属性（6 属性）だけ。構成と負荷の型は取得せず、呼び出し元が渡す入力である。
  - 構成: 起動した構成要素の（種類, stub の印）の組を `ComponentConditions::new` に渡す。組は呼び出し元が it-harness の起動済みの構成
    （`RunningHarness::composition_record()` の各 entry の種類と stub の印）から写して作る。写しは自動ではない。
    渡さなかった種類は `not_started` で出る（起動していないのか渡し忘れかは出力から区別できない）。
  - 負荷の型: 呼び出し元が `LoopModel` を渡す（`MeasurementConditions::detect(components, loop_model)`）。

- API: `MeasurementConditions`（機材・構成・負荷の型。`detect` が機材を自動で取得）、`MachineCondition`（`detect_with`・`detect`・`new`）、
  `MachineProbe`・`StdMachineProbe`、`ComponentConditions`（`new` が重複と stub の印の食い違いを誤りにする）、`ConditionComponent`、`ConditionError`。
  `Display` は形式の 5 の行（`condition_machine` 6 行・`condition_component` 4 行・stub があるときだけ `note:`・`condition_loop`）を出す。
- 取得元: `cpu_count` は `available_parallelism`（使える並列数。物理 CPU 数ではない）、`memory_mib` は Linux の `/proc/meminfo` の `MemTotal`
  （固定パス・64 KiB の上限・整数だけ。上限で途切れたときは改行まで読めた行だけを解析する。コンテナではホストの総量）、`arch`・`os_family` は `std::env::consts`（std が特定できない `unknown` は取得不能の `None` にして `unknown` と出す。`arch` の許可語彙は `arm64ec`〔Windows ARM64EC〕を含む）、`build_profile` は確かめる手段が無いため常に `unknown`（`debug_assertions` はプロファイルで上書きされ誤記録になる。確かな値は `new` で明示）。
  `virtualization` は判別の方法が未決のため常に `unknown`。
- 取れない（`None`）値は `unknown`。`arch`・`os_family` は固定の許可語彙だけを受け、語彙の外の値は `detect_with`・`detect`・`new` のどれでも `InvalidMachineValue` の誤りで返す（`unknown` に置き換えない）。ホスト名・機材の名前・パスは取得も記録もしない。
- 条件は記録の単位で 1 つ持ち、`TargetRun`・`LoadRun` には入れない（8/10 の段階を変える実行で重複しないため）。構成は呼び出し元が起動したものを明示で渡し、渡さない種類は `not_started`。
- 7 の見本との差: `condition_loop` は 5/10 の閉ループの仮置きで `closed` を出す（見本は `undecided`）。
- 範囲外: `fandhe-auth-load-record/1` のファイル全体の出力、`stage` 行（並行数は 8/10 とファイル出力の段）、仮想化の判別。
  Host の実体は `TODO(TASK-716)` まで起動しない。依存の辺は変えない（通常の依存は 0）。

## 段階を変える実行（8/10）

`TASK-12` の 8/10（#119）。`crates/load-harness` の `stage` に、並行数・件数・暖機・繰り返しの違う段階を順に回す部品を置いた。
名前・語・値はすべて仮置きで、人が確認する。合否は付けない（`ACPT-7`）。

- API: `StageConfig`（`LoadConfig` と繰り返しの数）、`StagePlan`（段階の並びと、最悪の回を比べる指標。指標は明示で既定なし）、
  `StageLimits`（段階数・1 段階の繰り返し・1 回の試行数・計画全体の試行数の上限。呼び出し元が明示し、既定値なし）、
  `run_stages`（回ごとに時計を作る関数〔`Result<_, MeterError>` を返す〕・`TargetSpec`・計画・呼び出しを受ける）、`StageResult`・`StagedRun`（結果。段階ごとに分けて持つ）、`StageError`。
- 段階と回は計画の順に 1 つずつ回し、重ならない。各回は `run_target` で回す。段階の全回が終わったら `worst_run` で段階の最悪の回を求める。
- 誤り（fail-closed・仮置き）: 最初の誤りで以後の回・段階を始めず、終わった段階の結果も返さない（部分の結果を正しい結果に見せないため）。
  誤りは種類と整数の番号だけを持つ（`SECR-11`・`SECR-23`）。
- 時計の生成の誤り（仮置き）: 生成関数の `Err` は `StageError::Clock` とし、段階・回の番号付きで返す。回を始める前の失敗で、その回の対象は呼ばない。
  誤りは中身を持たない `MeterError` の種類だけで、対象の名前を運ばない（`SECR-23`）。
- 生成関数の panic: 捕捉せず、そのまま呼び出し元へ伝わる。生成関数・時計の破棄の panic も同じ。これらの panic のために panic hook を差し替え・抑止しない
  （hook はプロセス全体の共有状態で、ハーネスが握り続けることを保証できないため）。
  アンワインド中に生成関数や時計の破棄が続けて panic すると、二重 panic でプロセスが中断し、何も伝わらない。
  対象の閉包の panic の文言の抑止は、`run_closed_loop` の既存の `install_quiet_hook` に依る。これも panic hook に依るので同じ限界を持つ
  （呼び出し元が後から hook を差し替えた場合など）。その扱いは #6017 の範囲外で、ここでは変えない。対象の名前を伏せることをハーネスが保証するのは
  `Result` の誤りの経路（`StageError` の `Display`・`Debug`）だけで、panic の文言に対象の名前を入れないのは生成関数の書き手の責任である。
  伏せたい失敗は panic ではなく `Err` で返す。
- 結果は非公開フィールド+読み取りだけのアクセサーで持ち、回ごとに残すのは集計のみ（生標本は回の終了時に捨てる）。回の削除・並べ替えで `Display` の番号と `worst_run` がずれない。
- 記録の行: `StagedRun` の `Display` が、段階ごとに `stage`・（回ごとに）`run`・`failure_kind`・`metric`・`failure_metric`、最後に `worst_run` を出す（形式の 3・4）。
  `n=0` のときは百分位・最大・`max_trial` を `none` で出す。出るのは検査済みの名前・固定の語・整数だけ（形式の 6）。
- 利用ガイド: `StagePlan::new(stages, by, limits, allowed_intervals)`。`limits`（`StageLimits::new(max_stages, max_repetitions, max_trials_per_run, max_total_trials)`）は
  呼び出し元が実行環境に合わせて明示する（固定の上限は置かない。`DPOL-32`）。暖機と本計測の合計が `max_trials_per_run` を超える段階を持つ計画は拒否する
  （回の終了まで生標本を保持するため）。段階数・繰り返しの超過、全体の試行数の溢れ、0 の上限も計画の誤りにする。
  計画全体の試行数（各段階の (暖機 + `trials_planned`) × 繰り返し の合計）が `max_total_trials` を超えると `StagePlan::new` は `InvalidPlan` で拒否し、
  上限ちょうどは受理する（全回の集計を保持するため。対象を呼ぶ前に検査する）。
- `Debug`: `StagePlan`・`StageResult`・`StagedRun` の `Debug` は区間名を出さない固定文字列にする。
  公開アクセサーの戻り値も同様で、`IntervalName`・`FailureKind`・`TargetName` の `Debug` は名前の中身を出さない固定文字列にした
  （`RunSummary` のマップのキーを含む。`SECR-23`）。
- 区間名の語彙: `allowed_intervals` に記録へ出してよい区間名（承認済みの語彙。既定なし・空は誤り）を渡す。`run_stages` は `TargetSpec` の区間名が語彙に無ければ、
  最初の回の前に `InvalidPlan` で拒否する（`metric`・`failure_metric`・`worst_run` に語彙外の名前を出さない。`SECR-23`）。
- 範囲外: `format:`・`run_record_file:`・`judgment:` などファイル全体の出力と書き出し（担当の段は未決）、開ループ、合否、実物の経路（`TODO(TASK-716)`）。
  機材と構成（7/10）は記録の単位で 1 つ持ち、`StagedRun` には入れない。依存の辺は変えない（通常の依存は 0）。

## 記録の出力と試し実行（9/10）

`TASK-12` の 9/10（#120）。`crates/load-harness` の `record` に、`fandhe-auth-load-record/1` のファイル全体を組んで新規ファイルへ書き出す部品を置いた。
名前・語・上限はすべて仮置きで、人が確認する。合否は付けない（`ACPT-7`）。

- API: `RunSession::new`（実行の記録のファイル名・起動時刻・機材と構成を受け、作る時点で整合を検査する）、`RunSession::run`（呼び出す構成要素を呼ぶ前に申告させて検査し、その場で `run_stages` を回して `RunEvidence` を返す。申告そのものは記録に残さない。時計は中で標準の単調な時計を作り、引数で受けない。文脈は消費する）、`LoadRecord::new`（`RunEvidence`・記録時刻・比べる値を受ける）、`LoadRecord::write_new(dir)`、
  `ComparisonValue`・`ValueClass`・`ValueUnit`・`ValueName`・`RefId`・`RunRecordFile`・`ApprovedValues`・`LoadRecordError`。`RunEvidence` を段階の結果から作る公開の口は無い（下の「信頼の境界」）。
- 行の順: `format:` → `run_record_file:` → `run_started_at:` → `recorded_at:` → 条件 → 段階 → `value:` → `judgment:`（形式の 7 の見本どおり）。`judgment:` は常に空で、ハーネスは書かない。
- 区分の規則は作る時点で検査する（fail-closed）。`requirement`・`initial` は、承認一覧の置き場・形・承認の手順が決まるまで（TASK-12。追跡は #121）常に `ClassNotAllowed` で拒否する（出典・数値・`ApprovedValues` の照合より先で、値を通さない。引数の `ApprovedValues` は使わない）。`ApprovedValues::load(path)` は承認一覧のファイル〔1 行 `項目|区分|数値|単位|出典,出典`。通常のファイルだけ・64 KiB まで〕を形式の検査つきで読む口として残す。フィールドは非公開で、作る口はこの `load` と空の一覧〔`Default`〕だけ、本文の文字列から作る公開の口はない〔書式の解析は非公開〕。読めなければ `ApprovedFile`。出典 ID は `RefId::new` と同じ形・予約語の検査を読み込み時に通し、満たさない行は `Format`。照合を戻した後は、1 件と完全一致しなければ `NotInApprovedList` で拒否する（規則 4）。
  `poc_parameter` は比べる値として置かない（規則 5）ため `ClassNotAllowed` で拒否する、`undecided` は値なし、`external` は値なし・出典必須、
  `test_condition`・`measured` は整数の値が必須で出典を受けず、行には固定の印（`harness_config`・`measured`）を出す。出典は 1 行 8 件まで・重複は誤り。
  ハーネスの中に測定値・既定値を要件の値へ上げる経路は作らない（要件の値・初期値は常に拒否する）。
- 書き出し: `load-run-<起動時刻の秒>-<pid>-<通し番号>.txt`。無ければ `dir` を作る。隠し名の一時ファイルへ書ききってから、上書きなしの `hard_link` で最終名へ公開する
  （途中の失敗で不完全なファイルを完成品の名前で残さない。一時ファイルは作れたときだけ消しにいくが、削除の失敗は無視するため、失敗時は残りうる。名前が衝突して作れなかったときは既存のファイルを消さない）。乱数・環境変数・設定ファイルを使わない。出力先のファイルシステムはハードリンクに対応している必要がある（非対応だと `WriteFailed`）。
- 入力の検査（仮置き）: `ValueName::new` は呼び出し元が渡す承認済みの語彙に含まれる名前だけを受ける。`RefId::new` は呼び出し元が渡す信頼済みの出典の一覧（spec の ID 等）に完全一致するものだけを受け、予約語（`none`・`harness_config`・`measured`）を拒否する。
  語彙・一覧は名前と同じ呼び出し元が渡すため、呼び出し元が同じ値を両方に入れれば通る（6 の記録の出力先の段落。入力から独立した語彙にする件は #6038・TASK-12）。
  `p99_target` は `undecided` 以外の区分を `ClassNotAllowed` で拒否する（`AUDP-29`）。`RunRecordFile` は生成元と同じ整数型で解析し、再整形した名前と一致しない名前（先頭ゼロ等）を拒否する。
  `RunRecordFile` の起動秒と `run_started_at` が違えば `RunSession::new` が `RunStartMismatch`。申告した構成要素が空、または測定の条件で起動していなければ、`RunSession::run` が対象を 1 回も呼ばずに `InvalidRunEvidence`（stub を申告した実行に stub の起動を載せない構成が付く組を作らない。stub の `note:` は条件の `Display`）。段階の実行の誤りは `Stage`。時刻は 9999-12-31T23:59:59Z（RFC 3339 の 4 桁年）まで、超えると `TimeOutOfRange`。
  値の行の違反の理由は固定の列挙 `InvalidValueReason` で持つ（`&'static str` を公開しない）。
- 時刻の RFC 3339 の整形は、通常の依存を 0 に保つため it-harness の `UtcTimestamp` と同じ変換を `record` に重複して持つ。
- 承認一覧の読み込みは止まらない: Unix では `O_NONBLOCK` を付けて開き（値は libc 0.2.191 の定義と突き合わせた対象だけに置く。libc には依存しない。CI で実地に確かめているのは Linux x86_64 の値だけで、他の対象は定義との突き合わせだけ）、書き手のいない FIFO でも `open` で待たずに、開いたハンドルの種別と大きさの検査で `ApprovedFile` にする。
  値を置いていない対象では開く前にパスの種別も見る（検査と `open` の間の差し替えは防げない）。
- 信頼の境界（ライブラリの中で証明できないもの。`ACPT-7` は検討中で、ここでは規則として扱う）:
  - 承認一覧（現在は照合に使わない。要件の値・初期値は常に拒否する。TASK-12・#121）: `ApprovedValues` が検査するのは形式（ファイルの種類・大きさ・UTF-8・書式、候補との完全一致）だけで、行を人が承認したこと（真正性）は検査しない。
    ライブラリの中では、ファイルを書いたのが人か、記録の候補を持つ同じ呼び出し元かを区別できない（照合を戻した後は、呼び出し元が候補から書いたファイルを渡せば、その値は `requirement`・`initial` として通る。TASK-12・#121 までは常に拒否するため通らない）。
    真正性は、承認一覧を repo に commit し、人がレビューで承認することで担保する（値の決定・承認は人が行う。`ACPT-7`・規則 4）。一覧のファイルの置き場と形は未定で、人が決める（下の「未決・人の確認待ち」）。
    試験（`tests/record.rs`）が書く一時ファイルは書式の検査の入力で、承認ではない。
  - 段階の結果と実行の結び付け（型で塞いだもの）: 段階の結果は `RunSession::run` の中でだけ作られ、同じ文脈（実行の記録・測定の条件）とまとめて `RunEvidence` になる。
    別の実行の段階の結果を組み合わせる口は無い。呼ぶ構成要素は回す前に申告させ、起動していなければ対象を呼ばずに拒否する（申告を検査してから回す。申告そのものは記録に残さない）。
    時計は `RunSession::run` の中で標準の単調な時計を作り、引数で受けない（偽の時計で `class=measured` の時間を作る経路を作らない。決定的な時計が要る試験は `run_stages` を直接使い、その結果からは `RunEvidence` を作れない）。stub の種類は `ComponentConditions::new` が実物として載せられない（`StubMismatch`）ため、stub を申告した実行の記録には stub の起動と `note:` が必ず出る（`A-2026-10-28-9`）。
  - 段階の結果と実行の結び付け（塞げないもの。呼び出し側の契約）: (1) 閉包が実際に呼ぶ構成要素と申告の一致（閉包の中身は見えない）、(2) 測定の条件の構成が実際に起動した構成要素と一致すること（呼び出し元が it-harness の起動済みの構成から写す）、
    (3) `run_record_file` が同じ実行の記録であること（見るのは起動秒の一致だけで、名前の文字列は誰でも作れるため結び付けの証明ではない）、
    (4) 機材の条件が実際の機材と一致すること（`MachineCondition::new` や独自の `MachineProbe` で `virtualization`・`build_profile` 等を呼び出し元が自由に申告できる）。
    `load-harness` は通常の依存で it-harness を持たないため、(1)〜(3) を確かめられない。(4) は呼び出し元が明示する値を受ける設計で、申告の正しさは確かめない。
    stub の結果を実物の結果として申告しないことは、呼び出し元の試験とレビューで担保する（〔仮想〕〔モック〕の合格を〔実機〕扱いしない。`ACPT-7`）。試し実行は stub 側の呼び出しの件数で (1) を確かめている。
- 誤りは種類だけを持ち、値・パス・`io::Error` の文言を載せない。`Debug` は中身を出さない固定文字列にした（`SECR-23`）。
- 試し実行（`tests/trial_run.rs`。Docker・ネットワークなし。`make test` で流れる）: it-harness の上で Hub の stub に 2 段階（並行数 1・2）の負荷をかけ、
  実行の記録（`it-run-…`）と負荷の記録を同じ出力先へ書く。見た点は、記録のファイルが出ること・名前の形・`run_record_file` と `run_started_at` が実行の記録と結ばれること・
  行の数と件数・全試行数に一致する stub 側の呼び出し・`class=requirement`/`initial`/`poc_parameter` が 1 行も無いこと・値の行が `undecided` の 1 行だけであること・
  メタデータ以外の全行が `class=` と `refs=` を持つこと・設定の目印と出力先のパスが出ないこと・再度の書き出しが別名の新しいファイルになること。時間の値は判定しない。
  stub の合格を実物の合格に数えない（`A-2026-10-28-9`）。
- 範囲外: 開ループ、実物の経路（`TODO(TASK-716)`）、仮想化の判別、人向けの表示の単位と手順書（10/10）、合否の値の導出（`TASK-21`・`AUDP-29`）。

## 未決・人の確認待ち

- 文書名・節立て
- 百分位の方法（最近順位法・補間なし）
- 「最悪の回」の読み（標本の最大か、繰り返しの中の最悪の 1 回か）と、比べる指標の既定
- 失敗・時間切れを時間の分布から分ける扱い
- 区分の追加分（試験の条件・測定値・未決・他所有者の値）
- 閉ループか開ループか（5/10）: 閉ループ（固定数のワーカー）を仮置きで採った。開ループは作っていない。人が確認する
- 5/10 の仮置き: 並行数の上限 1024、暖機の段と本計測の段を分けること、本計測での `Excluded` の拒否、停止の順序（記録より前に枠を取り終えた試行は走り切る。#5984）、API の名前（`generate`・`run_closed_loop`・`LoadConfig`・`LoadRun`・`LoadError`）
- 許可表の行: `fandhe-auth-load-harness` → `fandhe-auth-it-harness`・`fandhe-auth-hub-stub` の試験（dev）の辺（5/10）
- 記録への偽の行の差し込みへの対策（下の「結果の記録の形式」の 6 で扱った。内容は人が確認する）
- 結果の記録の形式の型と出力の実装の置き場。3/10（#114）で別の crate `crates/load-harness`（仮称）に計測の部品を置いた。
  `it-harness` の crate doc が `TASK-12` を別のハーネスとしているため、`it-harness` の module にはしない。crate の追加・名前・許可表の行は人が決める
- 記録の形式の識別子・キー名・トークン・ファイル名の形（すべて仮置き）
- 出典の無い `requirement`・`initial` を誤りにする扱い（`undecided` に落とさない）
- `test_condition`・`measured` の出典を固定の印で表すこと
- 失敗の時間の分布（`failure_metric`）の扱いと、同じ値が並んだときの最悪の回の選び方
- 閉ループのときの並行数の不変条件（5/10 の決定に依る）
- 版の情報を実行の記録側に寄せ、`run_record_file` で結ぶこと
- 6/10 の仮置き: `CallResult` の 4 種、失敗の種類名 `rejected`・`unavailable`、`TargetName` の規則、API の名前（`target`・`run_target`・`TargetSpec`・`TargetRun`・`TargetError`）
- 7/10 の仮置き: `os_family` に `consts::OS` を採ること、`cpu_count` を使える並列数とすること、`memory_mib` を `/proc/meminfo` の総量とすること、
  `virtualization` を常に `unknown` とし判別の方法を未決とすること、`build_profile` を標準の取得元では常に `unknown` とし、確かな値は呼び出し元が明示すること、
  自動取得した値の区分を `test_condition`・`refs=harness_config` とすること、「記録」の読み（ファイル出力を含まない）、API の名前（`condition` 以下）
- 「結果が記録される」の読み: `TargetRun` の構造体まで。ファイルへの出力を担当する段は未決（人が決める。受け入れ条件を満たしたかの判定も人）
- 8/10 の仮置き: `StageLimits` の項目と区間名の語彙を呼び出し元の設定で受けること、誤りのときに終わった段階を捨てること、行を出す順、API の名前（`stage`・`run_stages`・`StagePlan`・`StageConfig`・`StageResult`・`StagedRun`・`StageError`）
- 8/10 の時計の生成の誤り（#6017。仮置き）: 生成関数を `Result` で返す形にし、panic は捕捉せず伝播すること（panic hook で抑止する方式は採らない）は利用者の方針に沿う
  （2026-11-21 に利用者が、hook をやめて `Result` で返す方針を承認した。記録は PR #6019 のコメント）。
  生成関数の型を `FnMut() -> Result<Box<dyn MonotonicClock>, MeterError>` にしたこと、誤りの型を中身を持たない `MeterError` に固定したこと
  （`StageError` を `Copy` で種類と整数だけに保つため）、variant 名（`Clock`）は仮置きで、人が決める
- 「段階ごとに記録される」の読み: 構造体と行の `Display` まで。ファイルへの出力を担当する段は未決のまま（人が決める。受け入れ条件を満たしたかの判定も人）
- 9/10 の仮置き: 記録のファイル全体の出力を 9/10 に置いたこと、`requirement`・`initial` を承認の手順が決まるまで常に拒否したこと（#121）、`poc_parameter` を拒否したこと、`test_condition`・`measured` に出典を受けないこと、
  値の行の上限（64）と出典の上限（8）、`load-run-…` の名前を `run_started_at` の秒で作ること、時刻の整形を it-harness と別に持つこと、
  API の名前（`record`・`LoadRecord`・`ComparisonValue`・`ValueClass`・`ValueUnit`・`ValueName`・`RefId`・`RunRecordFile`・`LoadRecordError`）
- 「小さな試し実行が終わり記録が出る」の読み: `make test` の中の試験でファイルが出ること（人が確認する。受け入れ条件を満たしたかの判定も人）
- 承認一覧のファイルの置き場・形・承認の手順（repo に commit し人がレビューで承認する、という担保の具体）: 未定。人が決める（TASK-12。追跡は #121）。決まるまで `requirement`・`initial` は常に拒否する。現時点で repo に承認一覧のファイルは無い（性能の目標値は持ち越し。`AUDP-29`）
- 9/10 の追加の仮置き: `RunSession`（文脈を作ってから段階を回す形）、`InvalidValueReason::NotInApprovedList` の名前、`O_NONBLOCK` を置く対象の範囲と、置かない対象での開く前の種別の検査
