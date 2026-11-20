//! chromedriver の構成要素（`ChromeDriverComponent`）の試験。chromedriver・Chrome なしで流せる範囲だけ。
//!
//! unix 限定（構成要素が `cfg(unix)`）。結果は起動・停止の形の確認で、E2E の合格に数えない（ACPT-7）。
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fandhe_auth_e2e_harness::chrome::new_session_request;
use fandhe_auth_e2e_harness::{
    ChromeDriverComponent, ChromeDriverConfig, E2eComponent, E2eErrorKind, Protocol, Transport,
    VirtualAuthenticatorOptions,
};

fn opts() -> VirtualAuthenticatorOptions {
    // 試験の値。PoC の値（QP-1）であり要件の値ではない。
    VirtualAuthenticatorOptions {
        protocol: Protocol::Ctap2,
        transport: Transport::Internal,
        has_resident_key: true,
        has_user_verification: true,
        is_user_verified: true,
    }
}

#[test]
fn new_session_request_has_only_headless_flag() {
    // DPOL-30: 検証を外す意味の flag を足さない。
    let v = new_session_request();
    let args = &v["capabilities"]["alwaysMatch"]["goog:chromeOptions"]["args"];
    assert_eq!(args, &serde_json::json!(["--headless=new"]));
}

#[test]
fn start_with_missing_driver_program_fails_and_leaves_nothing() {
    let handle;
    let mut c = {
        let c = ChromeDriverComponent::new(ChromeDriverConfig::new("/nonexistent/e2e-driver"));
        handle = c.handle();
        c
    };
    let e = c.start().unwrap_err();
    assert_eq!(e.kind(), E2eErrorKind::StartFailed);
    assert!(!handle.is_running());
    assert!(c.stop().is_ok());
}

#[test]
fn handle_is_not_running_before_start() {
    let c = ChromeDriverComponent::new(ChromeDriverConfig::new("x"));
    let h = c.handle();
    assert_eq!(
        h.navigate("http://localhost/").unwrap_err().kind(),
        E2eErrorKind::NotRunning
    );
    assert_eq!(
        h.browser_version().unwrap_err().kind(),
        E2eErrorKind::NotRunning
    );
    assert_eq!(
        h.add_virtual_authenticator(&opts()).unwrap_err().kind(),
        E2eErrorKind::NotRunning
    );
}

/// 試験ごとの一時ディレクトリ。秘密値は置かない。
fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("e2e-harness-{tag}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[test]
fn failed_start_kills_the_whole_process_group() {
    // 前提: ready にならない偽のドライバ。配下に孫プロセス（Chrome の代わり）を 1 つ残して待ち続ける。
    // 操作: 短い起動待ちで start する。セッション ID は取れない。
    // 期待: StartFailed で、子だけでなく孫もプロセスグループごと止まる。
    let dir = temp_dir("group");
    let pid_file = dir.join("grandchild.pid");
    let script = dir.join("fake-driver");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nexec sleep 30\n",
            pid_file.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();

    let mut c = ChromeDriverComponent::new(
        ChromeDriverConfig::new(&script).with_ready_timeout(Duration::from_secs(1)),
    );
    let started = Instant::now();
    let e = c.start().unwrap_err();
    assert_eq!(e.kind(), E2eErrorKind::StartFailed);
    // 起動待ちの期限（1 秒）を大きく超えて待たない。
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );

    let grandchild = fs::read_to_string(&pid_file).unwrap();
    let grandchild = grandchild.trim();
    // 孫は init に引き取られて回収されるまで zombie で残り得るので、少しだけ待つ。
    let deadline = Instant::now() + Duration::from_secs(3);
    while alive(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(grandchild), "grandchild survived");
    assert!(c.stop().is_ok());
    let _ = fs::remove_dir_all(&dir);
}
