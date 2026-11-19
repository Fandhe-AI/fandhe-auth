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
| `ConsoleStub` | 4/6（#93） | `crates/console-stub`（`TASK-8`） | in-process |

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
- ファイル出力・版・日時は 5/6（#94）。console の stub は 4/6（#93）。

## 実行の記録の形式

未定。5/6（#94）で版・構成・日時を決めて追記する。

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
- `LIB-14`・`LIB-15` との関係（段階 1 のリリースのメンバーに含めない扱い）
