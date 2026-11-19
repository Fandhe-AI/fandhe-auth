# 障害注入のハーネス（TASK-13）

TASK-13 の設計の下書き。名前・形式の識別子・キー名・文言はすべて仮置きで、人が確認する。
記録は事実の出力で、合否の判定でも spec の「確定」でもない（ACPT-7）。stub の合格を実物の合格に数えない（A-2026-10-28-9）。
使い方は 8/8 で本書に足す。

## 置き場

`crates/it-harness` の module `fault`（`crates/it-harness/src/fault/`）。別の crate にしない（書き手の判断で、人が確認する）。
後続の段は `fault/` にファイルを足し、`mod.rs` に `mod` を 1 行足すだけにする。

## 注入の種類（1/8）

| 種類 | 名前 | 対象 | パラメータ | 担当の段 |
| --- | --- | --- | --- | --- |
| DbBlock | `db_block` | database | なし | 2/8 |
| DbDelay | `db_delay` | database | `delay_ms` | 2/8 |
| InstanceSigterm | `instance_sigterm` | instance | なし | 3/8 |
| InstanceSigkill | `instance_sigkill` | instance | なし | 3/8 |
| ExternalStoreBlock | `external_store_block` | external_store | なし | 4/8 |
| HubStubDelay | `hub_stub_delay` | hub_stub | `port`・`delay_ms` | 5/8 |
| HubStubRefusal | `hub_stub_refusal` | hub_stub | `port`・`refusal` | 5/8 |
| ClockSkew | `clock_skew` | instance（仮） | `skew_ms` | 6/8 |

根拠: AVLB-31（時計のずれ）・AVLB-33（`SIGTERM`・`SIGKILL`）・ORST-17（DB の外の保存先）。
パラメータは作るときに検査し、0 の遅れ・0 のずれを拒否する。遅れ・ずれの値は試験用の仮の値で要件の値ではない（THRT-29・A-2026-10-28-1）。
`ComponentKind` には種類を足さない（DB の外の保存先は構成要素ではない）。

## DB の遮断と遅延（2/8）

`crates/it-harness/src/fault/db.rs` の `DbFaultProxy`（仮称）。PostgreSQL の前に置くループバックの TCP 転送器で、
PoC-7 の手順 6 の向き（転送器が応答に遅延を足す・経路を遮断する）に合わせる。外部の crate は足さず `std::net` と `std::thread` だけで書く。

- 待ち受けも上流も `127.0.0.1` 固定。上流はポートだけを受け、任意のホストへは転送しない。TLS は素通しで、中身は読まない。
- 入口: `start_to_port`（試験用）・`start_for(EndpointHandle)`（`Database` の起動前に作れる。値が無い間は接続を閉じる）・
  `proxied_endpoint`（ポートを差し替えた接続情報。Host への結線は TASK-716 待ち）・`block`・`release_block`・`delay`・`release_delay`・`stop`。
- 遮断は開いている接続を切り、新しい接続を直ちに閉じる。解除しても切れた接続は戻らないので、client はつなぎ直す。
- 遅延は応答（上流から client）の向きの塊ごとに足す。開いている接続にも効く。解除は同じ値で `Released` を記録する。
- 記録が成功してから状態を変える。基準線の `FaultTimeline` への注入は記録が拒否し、注入もされない。
  二重の開始は `AlreadyActive`、無い注入の解除は `NotActive`。
- 誤りと `Debug` にポート・アドレスを載せない（SECR-11・SECR-23）。
- 試験: `make test`（`tests/fault_db.rs`。Docker 不要）・`make it-db`（`tests/fault_db_docker.rs`。実際の PostgreSQL に SSLRequest を送る）。
  時間に依る確認は下限だけ。

## 記録の形式（`fandhe-auth-it-fault-record/1`）

`RunRecord`（`fandhe-auth-it-run-record/1`）とは別のファイル。`run_started_at` で結ぶ。UTF-8・LF・1 行 1 項目の `key: value`。
注入した実行（`run_mode: fault`）と基準線（`run_mode: baseline`）で同じキーを出す。基準線は `injection_count: 0`・`unreleased_count: 0`。

```
format: fandhe-auth-it-fault-record/1
run_mode: baseline | fault
run_started_at: <RFC 3339 UTC>
recorded_at: <RFC 3339 UTC>
injection_count: <N>
unreleased_count: <M>
injection: <seq> <applied|released> <category> target=<target> at=<UTC> offset_ms=<n>[ パラメータ]
note: ...
```

- 基準線に事象を積む・Fault の実行で 0 件のまま記録を作る・`Applied` の無い `Released`・`offset_ms` の逆行は誤り（fail-closed）。
- 解除しなかった `Applied` は `unreleased_count` に数える。`SIGTERM`・`SIGKILL` は解除が無い一回きりの事象として数えない。
- 記録は enum・数値・静的な名前だけで組む。ホスト名・ポート・パス・接続情報・実行時の文字列を持たない（SECR-11・SECR-23・SECR-26）。

## 時計

時刻はハーネスのプロセスの基準の時計で押す（壁時計 `at`〔秒〕と、実行開始からの単調時計の `offset_ms`）。
試験対象に注入したずれた時計や stub の引数の `now` は使わない。hub-stub の `InjectionLog.at` との対応づけは 5/8 で決める。

## 未決・人の確認待ち

- 置き場を it-harness の module にしたこと
- 型・種類・形式の識別子・キー名・文言・ファイル名の形（すべて仮置き）
- 記録を `RunRecord` と別のファイルにし、`run_started_at` で結ぶこと
- 時計のずれの対象（instance の時計か DB のホストの時計か）。6/8 で決める
- インスタンスの停止を in-process の Host（TASK-716 待ち）にどう当てるか、停止の後の再起動を `Released` とするか。3/8 で扱う
- DB の外の保存先の実体（ORST-2 の持ち越し）。4/8 で扱う
- console stub の注入（TASK-8）を TASK-13 の一覧に入れるか（spec の列挙には無い）
- 記録のファイルを秘密値の走査の対象に入れること（`secret-values.md`）。本段は canary の試験で代える。出力先は追跡ツリーの外（`target/`）
- DB の遮断の形を「直ちに閉じる」にしたこと。「受けて黙る（blackhole）」を入れるか（2/8）
- 遅延を応答の向きの塊ごとに足す形（2/8）
- 開発の値（accept の刻み 5 ms・待ちの刻み 10 ms・上流への接続の上限 5 秒・遅延の上限 600 秒）（2/8）
- 転送器を `Component` にせず `ComponentKind` を 4 種のままにしたこと。Host への結線は TASK-716 待ち（2/8）
