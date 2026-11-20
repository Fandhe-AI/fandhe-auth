//! 起動失敗時のプロセスグループ停止の回帰試験（Chrome・Firefox の共通部 `driver_process` に対する 1 か所の試験）。
//!
//! 共通の停止処理はブラウザに依らないため、ここだけで確かめる。ブラウザ固有の試験（`chrome_component.rs`・
//! `firefox_component.rs`）は起動引数とセッション要求の違いだけを見る。unix 限定。結果は E2E の合格に数えない（ACPT-7）。
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fandhe_auth_e2e_harness::{
    ChromeDriverComponent, ChromeDriverConfig, E2eComponent, E2eErrorKind,
};

/// 試験ごとの一時ディレクトリ。秘密値は置かない。
fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("e2e-harness-{tag}-{}-{nanos}", std::process::id()));
    // 排他的に作る（既存なら失敗。他者のディレクトリを使わない）。所有者だけが入れる権限（0700）で作る。
    fs::DirBuilder::new().mode(0o700).create(&d).unwrap();
    d
}

/// PID ファイルの内容を正の整数として検証して返す。空・非数・0 は試験の失敗（孫を確かめないまま通さない）。
fn parse_pid(text: &str) -> String {
    let pid: u32 = text.trim().parse().expect("PID ファイルが正の整数でない");
    assert!(pid > 0, "PID が 0");
    pid.to_string()
}

/// `pid` が zombie 以外の状態で居るか。zombie は止まっている（回収待ちなだけ）とみなす。
/// 引き取り先が回収しない実行環境でも判定が変わらないよう、`ps` の状態で見分ける。
/// プロセス不在（終了コード 1・出力なし）と照会の失敗は区別し、照会できなければ試験を失敗させる。
fn alive(pid: &str) -> bool {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .stderr(Stdio::piped())
        .output()
        .expect("ps を起動できない");
    let stat = String::from_utf8_lossy(&out.stdout);
    let stat = stat.trim();
    if out.status.success() {
        assert!(!stat.is_empty(), "ps が成功したのに状態が空");
        return !stat.starts_with('Z');
    }
    // 対象不在と確認できるのは、終了コード 1・標準出力も標準エラーも空のときだけ。
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.code() == Some(1) && stat.is_empty() && err.trim().is_empty(),
        "ps の照会に失敗: {:?} stderr={:?}",
        out.status,
        err.trim()
    );
    false
}

#[test]
fn failed_start_kills_the_whole_process_group() {
    // 前提: ready にならない偽のドライバ。配下に孫プロセス（ブラウザの代わり）を 1 つ残して待ち続ける。
    // 操作: 短い起動待ちで start する。セッション ID は取れない。
    // 期待: StartFailed で、子だけでなく孫もプロセスグループごと止まる。
    let dir = temp_dir("group");
    let pid_file = dir.join("grandchild.pid");
    let script = dir.join("fake-driver");
    // スクリプトは固定の文字列で、試験用のパスを本文に埋め込まない（A03）。pid の置き場は
    // スクリプト自身の場所（`$0`）からの相対で、引用して参照する。
    fs::write(
        &script,
        "#!/bin/sh\nsleep 30 &\necho $! > \"$(dirname \"$0\")/grandchild.pid\"\nexec sleep 30\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();

    let mut c = ChromeDriverComponent::new(
        ChromeDriverConfig::new(&script).with_ready_timeout(Duration::from_secs(1)),
    );
    // 書いた直後の実行ファイルは、他の試験スレッドの fork と競合して spawn が失敗する（ETXTBSY）ことがある。
    // スクリプトが走った（pid ファイルがある）ことを確かめてから停止を検査し、走っていなければやり直す。
    let mut started = Instant::now();
    for _ in 0..5 {
        started = Instant::now();
        let e = c.start().unwrap_err();
        assert_eq!(e.kind(), E2eErrorKind::StartFailed);
        if pid_file.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(pid_file.exists(), "偽のドライバが実行されなかった");
    // 起動待ちの期限（1 秒）を大きく超えて待たない。
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );

    let grandchild = parse_pid(&fs::read_to_string(&pid_file).unwrap());
    let grandchild = grandchild.as_str();
    // SIGKILL の配送を少しだけ待つ。
    let deadline = Instant::now() + Duration::from_secs(3);
    while alive(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(grandchild), "grandchild survived");
    assert!(c.stop().is_ok());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn parent_exit_before_ready_still_kills_descendants() {
    // 前提: 配下に孫プロセスを残して、ready を返す前に親（偽のドライバ）が終了する。
    // 操作: start する。期待: StartFailed で、孫もグループごと止まる。
    let dir = temp_dir("parent-exit");
    let pid_file = dir.join("grandchild.pid");
    let script = dir.join("fake-driver");
    fs::write(
        &script,
        "#!/bin/sh\nsleep 30 &\necho $! > \"$(dirname \"$0\")/grandchild.pid\"\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();

    let mut c = ChromeDriverComponent::new(
        ChromeDriverConfig::new(&script).with_ready_timeout(Duration::from_secs(3)),
    );
    // 書いた直後の実行ファイルは、他の試験スレッドの fork と競合して spawn が失敗する（ETXTBSY）ことがある。
    // そのときはスクリプトが走っていない（pid ファイルが無い）ので、試験の対象外として数回やり直す。
    for _ in 0..5 {
        let e = c.start().unwrap_err();
        assert_eq!(e.kind(), E2eErrorKind::StartFailed);
        if pid_file.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let grandchild = parse_pid(&fs::read_to_string(&pid_file).unwrap());
    let grandchild = grandchild.as_str();
    let deadline = Instant::now() + Duration::from_secs(3);
    while alive(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(grandchild), "grandchild survived");
    let _ = fs::remove_dir_all(&dir);
}
