//! geckodriver の構成要素（`GeckoDriverComponent`）の試験。geckodriver・Firefox なしで流せる範囲だけ。
//!
//! unix 限定（構成要素が `cfg(unix)`）。結果は起動・停止の形の確認で、E2E の合格に数えない（ACPT-7）。
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::time::Duration;

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

#[test]
fn geckodriver_receives_port_as_two_separate_arguments() {
    // P2-c: geckodriver には `--port` と番号が別々の引数で渡る（`--port=N` の形ではない）。
    // 偽のドライバは受けた引数を記録して終了する。本文は固定で、試験用のパスを埋め込まない（A03）。
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("e2e-gecko-args-{}-{nanos}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    // panic でも一時ディレクトリを残さない。
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(dir.clone());
    let script = dir.join("fake-driver");
    let args = dir.join("args");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$#\" \"$@\" > \"$(dirname \"$0\")/args\"\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let mut c = GeckoDriverComponent::new(
        GeckoDriverConfig::new(&script).with_ready_timeout(Duration::from_secs(3)),
    );
    // 書いた直後の実行ファイルは ETXTBSY で spawn に失敗し得るため、記録ができるまでやり直す。
    for _ in 0..5 {
        assert_eq!(c.start().unwrap_err().kind(), E2eErrorKind::StartFailed);
        if args.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let text = fs::read_to_string(&args).expect("fake driver was not executed");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0], "2");
    assert_eq!(lines[1], "--port");
    let port: u32 = lines[2].parse().expect("port number");
    assert!((1..=65535).contains(&port));
}
