//! 範囲の型の試験。前提・操作・期待の形。
//! 注意: ここでの合格は型の範囲の確認で、E2E の合格ではない（ACPT-7）。

use fandhe_auth_e2e_harness::{AuthenticatorKind, Browser, Evidence, PasskeyOperation};

/// 範囲は登録と認証の 2 件だけ（TASK-11。画面の操作の変種は無い）。
#[test]
fn scope_is_registration_and_authentication_only() {
    assert_eq!(PasskeyOperation::ALL.len(), 2);
    for op in PasskeyOperation::ALL {
        // 網羅の match。変種を足すとここがコンパイルできない。
        match op {
            PasskeyOperation::Registration | PasskeyOperation::Authentication => {}
        }
    }
}

/// ブラウザは Chrome・Firefox だけで、Safari の変種は無い（TASK-784）。
#[test]
fn browser_has_no_safari_variant() {
    let all = [Browser::Chrome, Browser::Firefox];
    for b in all {
        match b {
            Browser::Chrome | Browser::Firefox => {}
        }
    }
    assert_eq!(all.len(), 2);
}

/// 仮想 authenticator の結果は実機にも Safari にも数えない（ACPT-7・ACPT-8）。
#[test]
fn virtual_evidence_never_counts_as_real_device_or_safari() {
    let kind = AuthenticatorKind::WebDriverVirtual;
    let evidence = kind.evidence();
    assert_eq!(evidence, Evidence::Virtual);
    assert!(!evidence.counts_as_real_device());
    assert!(!evidence.counts_as_safari());
}
