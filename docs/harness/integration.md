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
| `Database` | 2/6（#91） | `dev/postgres`（`TASK-5`） | 外部プロセス |
| `Host` | 2/6（#91） | 最小の組込み先（`TASK-716`。完成後に置く） | in-process |
| `HubStub` | 3/6（#92） | `crates/hub-stub`（`TASK-7`） | in-process |
| `ConsoleStub` | 4/6（#93） | `crates/console-stub`（`TASK-8`） | in-process |

実体は `fandhe_auth_it_harness::Component` を実装して置く。種類はこの 4 つだけで、増やさない。

## 起動と停止

- 入口は Rust の API: `Harness::start(HarnessConfig) -> RunningHarness`、`RunningHarness::stop()`。
- 起動順は Database → Host → HubStub → ConsoleStub、停止はその逆。渡した順には依らない。
- 同じ種類の重複は起動前にエラー（複数インスタンスを作らない）。
- 起動の途中で失敗したら、起動済みを逆順に停止してから誤りを返す。`Drop` でも停止を試みる。
- 誤りは種類と構成要素の種類だけを持つ。秘密値・接続文字列を載せない（`SECR-11`・`SECR-23`）。

入口を make ターゲットにしない理由: DB のパスワードを生成して呼び出し元へ渡せないため
（`SECR-6`。`dev/postgres/README.md` の方針）。

## CI との関係

CI の runner に DB は無い。DB を使う試験を `make test` からどう分けるかは 2/6 で決める（未定）。
検証を外す feature・環境変数の分岐は作らない前提で選ぶ（`DPOL-30`〜`DPOL-32`）。

## stub の扱い

stub の合格を実物の合格に数えない（`A-2026-10-28-9`・`ACPT-7`）。記録の形への反映は 3/6〜5/6。

## 実行の記録の形式

未定。5/6（#94）で版・構成・日時を決めて追記する。

## 使い方

未定。6/6（#95）で追記する。

## 未決・人の確認待ち

- crate の追加・名前 `fandhe-auth-it-harness`・置き場 `crates/it-harness`・許可表の行
- `LIB-14`・`LIB-15` との関係（段階 1 のリリースのメンバーに含めない扱い）
