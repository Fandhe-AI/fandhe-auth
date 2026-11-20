//! geckodriver の構成要素（`GeckoDriverComponent`）の試験。geckodriver・Firefox なしで流せる範囲だけ。
//!
//! unix 限定（構成要素が `cfg(unix)`）。結果は起動・停止の形の確認で、E2E の合格に数えない（ACPT-7）。
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fandhe_auth_e2e_harness::firefox::new_session_request;
use fandhe_auth_e2e_harness::{
    E2eComponent, E2eErrorKind, GeckoDriverComponent, GeckoDriverConfig, Protocol, Transport,
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
fn new_session_request_has_only_headless_arg_and_two_prefs() {
    // DPOL-30: 検証を外す意味の引数・pref を足さない。pref は PoC-3 の値（要件の値ではない。人の確認待ち）。
    let v = new_session_request();
    let opts = &v["capabilities"]["alwaysMatch"]["moz:firefoxOptions"];
    assert_eq!(v["capabilities"]["alwaysMatch"]["browserName"], "firefox");
    assert_eq!(opts["args"], serde_json::json!(["-headless"]));
    assert_eq!(
        opts["prefs"],
        serde_json::json!({
            "security.webauth.webauthn_enable_softtoken": true,
            "security.webauth.webauthn_enable_usbtoken": false
        })
    );
    let text = v.to_string().to_ascii_lowercase();
    for word in [
        "disable",
        "insecure",
        "skip_",
        "bypass",
        "no_csrf",
        "allow_all",
        "unchecked",
    ] {
        assert!(!text.contains(word), "{word}");
    }
}

#[test]
fn start_with_missing_driver_program_fails_and_leaves_nothing() {
    let handle;
    let mut c = {
        let c = GeckoDriverComponent::new(GeckoDriverConfig::new("/nonexistent/e2e-driver"));
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
    let c = GeckoDriverComponent::new(GeckoDriverConfig::new("x"));
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

/// `pid` が zombie 以外の状態で居るか。zombie は止まっている（回収待ちなだけ）とみなす。
/// 引き取り先が回収しない実行環境でも判定が変わらないよう、`ps` の状態で見分ける。
fn alive(pid: &str) -> bool {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .stderr(Stdio::null())
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&out.stdout);
    let stat = stat.trim();
    !stat.is_empty() && !stat.starts_with('Z')
}

#[test]
fn failed_start_kills_the_whole_process_group() {
    // 前提: ready にならない偽のドライバ。配下に孫プロセス（Firefox の代わり）を 1 つ残して待ち続ける。
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

    let mut c = GeckoDriverComponent::new(
        GeckoDriverConfig::new(&script).with_ready_timeout(Duration::from_secs(1)),
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

    let grandchild = fs::read_to_string(&pid_file).unwrap();
    let grandchild = grandchild.trim();
    // SIGKILL の配送を少しだけ待つ。
    let deadline = Instant::now() + Duration::from_secs(3);
    while alive(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(grandchild), "grandchild survived");
    assert!(c.stop().is_ok());
    let _ = fs::remove_dir_all(&dir);
}
