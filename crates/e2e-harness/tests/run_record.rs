//! 実行の記録の形式の試験（TASK-11 5/6）。ブラウザは要らず、`make test` で流す。
//!
//! 偽のドライバを使う単体の試験は記録をファイルに書かない。E2E の実行回数には数えない。
//! 結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。

use fandhe_auth_e2e_harness::{
    AuthenticatorKind, Browser, BrowserVersion, E2eErrorKind, E2eRunRecord, MAX_RUNS,
    PasskeyOperation, RECORD_FORMAT, RecordError, RunResult, WebAuthnErrorName,
};

fn rec(browser: Browser) -> E2eRunRecord {
    E2eRunRecord::new(
        browser,
        BrowserVersion::Unknown,
        AuthenticatorKind::WebDriverVirtual,
        PasskeyOperation::Authentication,
    )
}

#[test]
fn every_browser_record_says_virtual_and_not_real_or_safari() {
    for b in [Browser::Chrome, Browser::Firefox] {
        let text = rec(b).to_string();
        assert!(text.starts_with(&format!("format: {RECORD_FORMAT}\n")));
        assert!(text.contains("authenticator: WebDriverVirtual\n"));
        assert!(text.contains("evidence: Virtual\n"));
        assert!(text.contains("counts_as_real_device: false\n"));
        assert!(text.contains("counts_as_safari: false\n"));
        assert!(text.contains("(ACPT-7, ACPT-8)"));
        assert!(text.contains("browser_version_major: unknown\n"));
    }
}

#[test]
fn counts_appear_for_both_operations_even_when_zero() {
    let mut r = rec(Browser::Chrome);
    r.record(PasskeyOperation::Registration, RunResult::Succeeded)
        .unwrap();
    r.record(
        PasskeyOperation::Authentication,
        RunResult::Rejected(WebAuthnErrorName::NotAllowed),
    )
    .unwrap();
    r.record(
        PasskeyOperation::Authentication,
        RunResult::HarnessError(E2eErrorKind::CommandFailed),
    )
    .unwrap();
    r.record(PasskeyOperation::Authentication, RunResult::CheckFailed)
        .unwrap();
    r.record(PasskeyOperation::Authentication, RunResult::PreflightFailed)
        .unwrap();
    r.record(PasskeyOperation::Authentication, RunResult::HarnessTimedOut)
        .unwrap();
    let t = r.to_string();
    assert!(t.contains("run: 5 Authentication preflight_failed\n"));
    assert!(t.contains("run: 6 Authentication harness_timeout\n"));
    assert!(t.contains("run: 1 Registration succeeded\n"));
    assert!(t.contains("run: 2 Authentication rejected:NotAllowed\n"));
    assert!(t.contains("run: 3 Authentication harness_error:CommandFailed\n"));
    assert!(t.contains("run: 4 Authentication check_failed\n"));
    assert!(t.contains(
        "count: Registration runs=1 succeeded=1 rejected=0 check_failed=0 harness_error=0 preflight_failed=0 harness_timeout=0\n"
    ));
    assert!(t.contains(
        "count: Authentication runs=5 succeeded=0 rejected=1 check_failed=1 harness_error=1 preflight_failed=1 harness_timeout=1\n"
    ));
    let empty = rec(Browser::Firefox).to_string();
    assert!(empty.contains(
        "count: Registration runs=0 succeeded=0 rejected=0 check_failed=0 harness_error=0 preflight_failed=0 harness_timeout=0\n"
    ));
    assert!(empty.contains(
        "count: Authentication runs=0 succeeded=0 rejected=0 check_failed=0 harness_error=0 preflight_failed=0 harness_timeout=0\n"
    ));
}

#[test]
fn completed_defaults_to_false_and_can_be_marked() {
    let mut r = rec(Browser::Chrome);
    assert!(r.to_string().contains("completed: false\n"));
    r.mark_completed();
    assert!(r.to_string().contains("completed: true\n"));
}

#[test]
fn run_limit_is_enforced() {
    let mut r = rec(Browser::Chrome);
    for _ in 0..MAX_RUNS {
        r.record(PasskeyOperation::Registration, RunResult::Succeeded)
            .unwrap();
    }
    assert_eq!(
        r.record(PasskeyOperation::Registration, RunResult::Succeeded),
        Err(RecordError::TooManyRuns)
    );
    assert_eq!(r.run_count(), MAX_RUNS);
}

#[test]
fn write_new_never_overwrites_and_errors_carry_no_path() {
    // プロセスごとに独立したディレクトリを使い、並行実行の他のプロセスと共有しない。
    let base = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("run-record-{}", std::process::id()));
    let dir = base.join("out");
    let r = rec(Browser::Chrome);
    let a = r.write_new(&dir).unwrap();
    let b = r.write_new(&dir).unwrap();
    assert_ne!(a, b);
    assert_eq!(std::fs::read_to_string(&a).unwrap(), r.to_string());
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);

    // 同じ最終パスへの 2 回目は衝突して失敗し、既存のファイルの内容は変わらない。
    let fixed = dir.join("collide.txt");
    std::fs::write(&fixed, b"existing").unwrap();
    assert_eq!(r.write_new_at(&fixed), Err(RecordError::WriteFailed));
    assert_eq!(std::fs::read(&fixed).unwrap(), b"existing");
    let p1 = dir.join("once.txt");
    assert_eq!(r.write_new_at(&p1).unwrap(), p1);
    let mut r2 = rec(Browser::Firefox);
    r2.mark_completed();
    assert_eq!(r2.write_new_at(&p1), Err(RecordError::WriteFailed));
    assert_eq!(std::fs::read_to_string(&p1).unwrap(), r.to_string());
    let _ = std::fs::remove_file(&fixed);
    let _ = std::fs::remove_file(&p1);

    // 出力先がファイルのときは失敗し、誤りの表示にパスが出ない。
    let file = base.join("not-a-dir");
    std::fs::write(&file, b"x").unwrap();
    let e = r.write_new(&file).unwrap_err();
    assert_eq!(e, RecordError::WriteFailed);
    let shown = format!("{e} {e:?}");
    assert!(!shown.contains("run-record"));
    assert!(!shown.contains('/'));
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn record_text_has_no_path_url_or_address_markers() {
    let mut r = rec(Browser::Chrome);
    r.record(PasskeyOperation::Registration, RunResult::Succeeded)
        .unwrap();
    let t = r.to_string();
    for bad in [
        "http",
        "127.0.0.1",
        "localhost",
        "/home",
        "sessionId",
        "session",
    ] {
        assert!(!t.contains(bad), "{bad}");
    }
}
