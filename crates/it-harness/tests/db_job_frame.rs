//! CI の DB ジョブの枠の試験（TASK-6 5/10。#52）。Docker が要るので `#[ignore]`。
//!
//! 役割: CI の DB 付きジョブ（`make it-db-ci`）が最低 1 件流す「空の DB 結合試験」。DB を使うケースの本体は
//! まだ無く、ここで確かめるのは枠の成立（DB が起動して接続情報が渡り、停止で戻ること）だけである。
//! 呼び出し元は `scripts/it-db-ci.sh` だけで、`make test` からは分ける。Docker の有無で自動に飛ばす
//! 分岐・feature・環境変数は作らない（DPOL-30〜32）。
//!
//! 方針:
//! - 一致する試験が 0 件の素通りは入口の script が失敗にする（AUDB-6 の向き。期待を緩めない）。
//! - パスワード・接続文字列を出力しない（SECR-6・SECR-23）。DB の起動は [`Database`] が行う。
//! - 合格は実物の Hub の DB の合格に数えない（ACPT-7）。AI のレビューの合格を安全の確定にしない（CG-2）。

use fandhe_auth_it_harness::{Database, DatabaseConfig, Harness, HarnessConfig};

#[test]
#[ignore = "requires docker (dev/postgres); run via make it-db-ci"]
fn db_job_frame_runs_empty_case() {
    let db = Database::new(DatabaseConfig::new());
    let handle = db.endpoint_handle();
    let running = Harness::start(HarnessConfig::new().with(Box::new(db))).expect("harness starts");
    assert!(handle.get().is_some(), "endpoint after start");

    // TODO(TASK-584): DB を使う結合試験のケースをここへ足す。
    // TODO(TASK-797): 同上（ケースの追加先）。
    // TODO(TASK-799): 同上（ケースの追加先）。

    running.stop().expect("stops");
    assert!(handle.get().is_none(), "endpoint cleared after stop");
}
