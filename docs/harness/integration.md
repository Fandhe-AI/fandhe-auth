# 結合試験のハーネス: 構成の下書き

`TASK-10`（結合試験のハーネス。MS-1）の構成の下書き。1/6（#90）で枠と起動・停止の入口を置いた。
仕様は spec の `TASK-10` で引く（本文は写さない）。この文書は下書きで、crate の名前・置き場は仮称、人が確定する。

## 目的と範囲

最小の組込み先（`TASK-716`）・DB・Hub の stub・console の stub をつなぐ、本体の結合試験の最小のハーネス。

含めないもの:

- 複数インスタンス・前段のリバースプロキシ・TLS の終端を含む統合の試験（Hub の `ARUN` の担当）
- 本番の前段を通した構成（`TASK-97`）
- E2E（`TASK-11`）・計測と負荷（`TASK-12`）

## 構成要素

| 種類（`ComponentKind`） | 担当の段 | 実体 | 形 |
| --- | --- | --- | --- |
| `Database` | 2/6（#91。実体あり: `database::Database`） | `dev/postgres`（`TASK-5`） | 外部プロセス（`docker compose`） |
| `Host` | `TASK-716` の完了待ち（未設置） | 最小の組込み先（`TASK-716`。2/6 では代用を作らない） | in-process |
| `HubStub` | 3/6（#92。実体あり: `hub_stub::HubStubComponent`） | `crates/hub-stub`（`TASK-7`） | in-process（sans-IO。ソケットなし） |
| `ConsoleStub` | 4/6（#93。実体あり: `console_stub::ConsoleStubComponent`） | `crates/console-stub`（`TASK-8`） | in-process（sans-IO。ソケットなし） |

実体は `fandhe_auth_it_harness::Component` を実装して置く。種類はこの 4 つだけで、増やさない。

## 起動と停止

- 入口は Rust の API: `Harness::start(HarnessConfig) -> RunningHarness`、`RunningHarness::stop()`。
- 起動順は Database → Host → HubStub → ConsoleStub、停止はその逆。渡した順には依らない。
- 同じ種類の重複は起動前にエラー（複数インスタンスを作らない）。
- 起動の途中で失敗したら、起動済みを逆順に停止してから誤りを返す。`Drop` でも停止を試みる。
- 誤りは種類と構成要素の種類だけを持つ。秘密値・接続文字列を載せない（`SECR-11`・`SECR-23`）。

Database の起動: 試験ごとに固有の compose project 名・空きポート（既定）で `dev/postgres/compose.yaml` を
`docker compose up -d --wait` し、ループバックだけの公開・ホストからの到達・`sslmode=require` の接続を確かめる。
どれかが満たされなければ起動の失敗にする（fail-closed）。停止は `down --volumes`。

起動の入口を make ターゲットにしない理由: パスワードを make から渡せないため（`SECR-6`）。
Docker を使う試験の入口 `make it-db` は、パスワードを試験プロセスの中で生成するので、この理由に当たらない。

Hub の stub の起動: `HubStubComponent::new(HubStubConfig)` を作り、`Box` に入れる前に `handle()` で
`HubStubHandle` を取る。起動は設定から模擬一式（JWKS・生存確認・`fandhe-auth` の JWKS 取得・変更のフィード）を
組んでハンドルに置くことで、ソケット・スレッド・子プロセスは作らない。試験は `handle.with(|s| ..)` で起動中の模擬を
呼ぶ。起動前と停止後は `None`。鍵・期待値の要る受け口（本人トークン・Logout Token・assertion）はセッションに
含めず、`session.config()` から試験が組む。障害注入は `TASK-13` の担当。

console の stub の起動: `ConsoleStubComponent::new(ConsoleStubConfig, AgentCredentialConfig)` を作り（両方必須。
既定値なし）、`Box` に入れる前に `handle()` で `ConsoleStubHandle` を取る。起動は設定から模擬一式（introspection・
配送の時点の確認・資格情報の発行と失効の呼び出し）を組んでハンドルに置くことで、ソケット・スレッド・子プロセスは作らない。
相手側の口の偽物（`Scripted*Ports`）はセッションに含めず、試験が渡す。console の stub と Hub の stub の結線は作らない
（Hub の stub に `ADLG-31` ③ の口が無い）。

## DB の接続情報の受け方

- 設定は `DatabaseConfig`（引数の型）で試験ごとに差し替える。環境変数・ファイルは読まない（`DPOL-32`）。
- パスワードは `openssl rand -hex 16` を試験プロセスの中で呼んで生成し、`SecretString` で持つ。子プロセスへは
  環境変数だけで渡し、argv・誤り・`Debug`・記録に載せない（`SECR-6`・`SECR-11`・`SECR-23`・`SECR-30`）。
  `docker compose config`・`docker inspect` は展開した値を出すので呼ばない。
- 接続情報は `EndpointHandle`（共有のハンドル）で渡す。試験が `Database::endpoint_handle()` を先に取り、Host へ
  構築時に渡す。起動前と停止後は `None`。URL の形（userinfo 付き）の接続文字列は作らない。

## CI との関係

CI の runner に DB は無い。Docker を使う試験は `#[ignore]` にして `make test` から分け（「ignored」と数えられ、
黙って合格にはならない）、`make it-db` で明示に流す。Docker の有無で自動に飛ばす分岐・feature は作らない
（`DPOL-30`〜`DPOL-32`）。DB を使う試験を CI に載せるかは `TASK-6` の範囲。

## stub の扱い

stub の合格を実物の合格に数えない（`A-2026-10-28-9`・`ACPT-7`）。

- `ComponentKind::is_stub()`: `HubStub`・`ConsoleStub` が `true`（網羅の `match`）。
- `RunningHarness::composition_record()`: 起動順の `CompositionRecord`。stub は「実物ではない」の印で残る。
  種類と印だけを持ち、設定の値・接続情報・鍵を載せない。
- `HubStubHandle::provisional_record()`: 設定の欄ごとの「仮置き」の印（`HubStubConfig::provisional_record`）。
- `ConsoleStubHandle::contract_record()`: 契約が仮定である旨の記録（`A-2026-10-28-12`）。値・URL を載せない。
- 記録への取り込み: `RunRecord::with_hub_stub_record`・`with_console_contract` で、欄名・項目名・印・根拠 ID だけを
  実行の記録に写す（次の節）。

## 実行の記録の形式

`crates/it-harness/src/record.rs` の `RunRecord`（5/6。#94）。形式・キー名・文言は仮置きで、人が確認する。
記録は実行の事実の出力で、合否の判定でも spec の「確定」でもない（`ACPT-7`）。

### 作り方と出力先

- `RunningHarness::run_record(VersionInfo)` で作り、必要なら stub の記録を取り込み、`RunRecord::write_new(dir)` で書く。
- 出力先 `dir` は呼び出し元が渡す（無ければ作る）。ファイル名は `it-run-<起動時刻の秒>-<pid>-<通し番号>.txt`。
  既存のファイルは上書きしない（`create_new`）。試験では `CARGO_TARGET_TMPDIR`（`target/` の下）を使い、追跡ツリーに書かない。
- 環境変数・設定ファイルは読まない（`DPOL-32`）。

### 形式

UTF-8・LF の 1 行 1 項目 `key: value`。キーは ASCII 固定。

| キー | 中身 | 出どころ |
| --- | --- | --- |
| `format` | 形式の識別子 `fandhe-auth-it-run-record/1` | 定数。形式を変えたら末尾の版を上げる |
| `started_at`・`recorded_at` | UTC・RFC 3339（秒） | ハーネスの起動時刻・記録を作った時刻 |
| `harness_version` | crate の版 | `CARGO_PKG_VERSION`（コンパイル時） |
| `source_revision` | 試験したソースの git revision（16 進 40 または 64 文字） | `git rev-parse HEAD` |
| `source_dirty` | 作業ツリーに変更があるか（`true`/`false`） | `git status --porcelain` が空かだけ。ファイル名は載せない |
| `spec_revision` | pin している spec の revision | `git rev-parse HEAD:docs/spec`（submodule を取得していなくても取れる） |
| `rustc` | コンパイラの版 | `rustc --version` |
| `component` | 種類ごとに `started real`・`started stub`・`not_started` | 構成の記録。4 種すべてを出す |
| `note` | stub を含むときだけ。stub の合格を実物の合格に数えない旨 | `A-2026-10-28-9`・`ACPT-7` |
| `hub_stub_field` | Hub の stub の欄名と「仮置き」の印 | `ProvisionalRecord`（取り込んだとき） |
| `console_contract` | console の stub の項目名・印・根拠 ID | `ContractRecord`（取り込んだとき） |

未起動の種類も `not_started` と出すのは、Host（`TASK-716` 待ち）を試したと読まれないようにするため。

### 出してはいけないものと守り方

設定の値・接続情報・ポート・compose project 名・パス・ホスト名・パスワードを記録に入れない（`SECR-11`・`SECR-23`・`SECR-26`）。

- 記録の型は値を持たない部品（種類・印・静的な欄名・数値の時刻）と検査済みの版の文字列だけで組む。設定の値が入る経路が無い。
- 実行時に外から入る文字列（revision・`rustc` の版）は文字種と長さで検査し、外れたら誤りにする（黙って省かない）。
  改行・制御文字を通さないので、偽の行を差し込めない。
- 誤り（`RecordError`）は種類だけを持ち、パス・コマンドの出力・`io::Error` の文言を載せない。
- 試験 `tests/run_record.rs` が、設定に入れた目印の文字列と crate のパスが `Display`・`Debug` に出ないことを確かめる。

## 使い方

未定。6/6（#95）で追記する。

## 未決・人の確認待ち

- crate の追加・名前 `fandhe-auth-it-harness`・置き場 `crates/it-harness`・許可表の行
- 2/6 の追加: `fandhe-auth-it-harness` → `fandhe-auth-secret` の辺（許可表の行）、make ターゲット名 `it-db`、
  Database の待ち時間の既定 60 秒（開発の値で要件の値ではない）
- 2/6 の未達: Host（最小の組込み先）の起動。`TASK-716` の完了待ちで、試験内の Fake Host を達成に数えない（`ACPT-7`）
- 3/6 の追加: `fandhe-auth-it-harness` → `fandhe-auth-hub-stub` の辺（許可表の行。`Cargo.toml`・`coding-rust.md` の記述も）、
  構成の記録の文言（仮置き）
- 3/6 の未達: Host（最小の組込み先）から Hub の stub への接続。`TASK-716` の完了待ちで、試験内の Fake Host を達成に数えない（`ACPT-7`）
- 3/6 でセッションに含めない受け口: 本人トークンの提示・失効の通知・assertion（鍵・期待値が要るため）
- 4/6 の追加: `fandhe-auth-it-harness` → `fandhe-auth-console-stub` の辺（許可表の行。`Cargo.toml`・`coding-rust.md` の記述も）、
  構成の記録・契約の記録の文言（仮置き）、`ConsoleStubComponent::new` が `AgentCredentialConfig` も必須で受ける設計（書き手の読み）
- 4/6 の未達: Host（最小の組込み先）から console の stub への接続。`TASK-716` の完了待ちで、試験内の Fake Host・Scripted の口を達成に数えない（`ACPT-7`）
- 4/6 でセッションに含めないもの: 相手側の口の偽物（`Scripted*Ports`・`Recording*`）・障害注入（`TASK-13`）
- 5/6 の追加（すべて仮置き。人が確認する）: 形式の識別子・キー名・文言・ファイル名の形、テキストの `key: value` にした選択
  （JSON は依存の辺が増えるので採らなかった）
- 5/6 の読み: 「版」の中身（crate の版・git の revision・作業ツリーの汚れの有無・spec の pin・`rustc` の版）は spec に規定が見当たらず、
  書き手の読み。DB のイメージの版は入れていない（`docker inspect` は展開した値を出すので呼ばない方針）
- 5/6 の設計: `git`・`rustc` を子プロセスで呼ぶことが `DPOL-32` の読みに沿うか
- 5/6 の未対応: 記録の出力先を秘密値の走査の対象に入れる扱い（本段は目印の文字列の試験で代える）。
  Docker 越しの記録（Database を含む）の試験は未追加で、Docker を使える環境で `make it-db` の対象に足す。Host の行は `TASK-716` まで常に `not_started`
- `LIB-14`・`LIB-15` との関係（段階 1 のリリースのメンバーに含めない扱い）
