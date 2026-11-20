//! DB 構成要素の設定・argv・Debug の試験（TASK-10 (2/6)）。Docker なしで `make test` で走る。

use std::path::Path;
use std::time::Duration;

use fandhe_auth_it_harness::{Database, DatabaseConfig, DatabaseEndpoint, EndpointHandle};
use fandhe_auth_secret::SecretString;

// 試験ごとに設定を差し替えられる。
#[test]
fn config_is_per_test() {
    let default = DatabaseConfig::new();
    let custom = DatabaseConfig::new()
        .with_port(40123)
        .with_wait_timeout(Duration::from_secs(5))
        .with_compose_file("/nonexistent/compose.yaml");
    assert_eq!(default.port(), None);
    assert_eq!(default.wait_timeout(), Duration::from_secs(60));
    assert!(
        default
            .compose_file()
            .ends_with("dev/postgres/compose.yaml")
    );
    assert_eq!(custom.port(), Some(40123));
    assert_eq!(custom.wait_timeout(), Duration::from_secs(5));
    assert_eq!(
        custom.compose_file(),
        Path::new("/nonexistent/compose.yaml")
    );
    // 差し替えは元の設定に影響しない。
    assert_eq!(default.port(), None);
}

// 構成要素ごとに compose の project 名が別で、固定名を使わない。
#[test]
fn project_names_are_unique() {
    let a = Database::new(DatabaseConfig::new());
    let b = Database::new(DatabaseConfig::new());
    assert_ne!(a.project(), b.project());
    for p in [a.project(), b.project()] {
        assert!(p.starts_with("fa-it-"));
        assert!(
            p.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        );
    }
}

// SECR-11・SECR-13: Debug（通常・整形）にパスワードが出ない。
#[test]
fn debug_hides_password() {
    let pw = "fedcba9876543210fedcba9876543210";
    let ep = DatabaseEndpoint {
        host: "127.0.0.1".into(),
        port: 1,
        user: "u".into(),
        dbname: "d".into(),
        sslmode: "require".into(),
        password: SecretString::new(pw),
    };
    assert!(!format!("{ep:?}").contains(pw));
    assert!(!format!("{ep:#?}").contains(pw));
    let db = Database::new(DatabaseConfig::new());
    assert!(!format!("{db:?}").contains(pw));
}

// 起動前は接続情報が無い。
#[test]
fn handle_is_empty_before_start() {
    let db = Database::new(DatabaseConfig::new());
    assert!(db.endpoint_handle().get().is_none());
    assert!(EndpointHandle::default().get().is_none());
}
