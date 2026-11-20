//! TASK-10 (6/6): 使い方の文書（`docs/harness/integration.md` の「使い方」）の手順どおりに動くサンプルの試験。
//!
//! 試験の書き手が最初に写す見本で、文書の手順の番号とコメントの番号が対応する。呼び出し元は試験ビルドだけ。
//! Docker・ネットワークなし（`make test` で必ず流れる。`#[ignore]` を付けない）。値は試験の中だけの仮の値
//! （A-2026-10-28-1・A-2026-10-28-14）。Host は TASK-716 の完了まで起動しない。Fake の Host は置かず、達成に数えない（ACPT-7）。
//! stub の合格を実物の合格に数えない（A-2026-10-28-9）。複数インスタンスは作らない（Hub の `ARUN` の担当）。

use std::path::Path;
use std::time::Duration;

use fandhe_auth_console_stub::config::{AgentCredentialConfig, ConsoleStubConfig};
use fandhe_auth_console_stub::delivery::{
    AgentCredentialScript, DeliveryContext, DeliveryDecision, DeliveryScript,
    ScriptedDeliveryPorts, ValidityAnswer, ValidityTarget,
};
use fandhe_auth_console_stub::introspection::AuthorityRequirement;
use fandhe_auth_hub_stub::assertion::AssertionSigningKey;
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_it_harness::{
    ComponentKind, ConsoleStubComponent, Harness, HarnessConfig, HarnessErrorKind,
    HubStubComponent, VersionInfo,
};

const T0: u64 = 1_000;
const TOKEN: &str = "opaque-token-for-test";
// 記録・Debug に出てはいけない目印（SECR-11・SECR-23）。
const HOST_MARK: &str = "sample-canary-host.example.test";
const CLIENT_MARK: &str = "sample-canary-client.test";
const TOKEN_MARK: &str = "sample-canary-opaque";

fn hub_config() -> HubStubConfig {
    HubStubConfig {
        client_id: StubValue::test(CLIENT_MARK.to_owned()),
        public_host: StubValue::test(HOST_MARK.to_owned()),
        jwks_uri: StubValue::test(format!("https://{HOST_MARK}/jwks")),
        logout_receiver_uri: StubValue::test(format!("https://{HOST_MARK}/logout")),
        jwks_max_age: StubValue::test(Duration::from_secs(1)),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(300)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
    }
}

fn console_config() -> ConsoleStubConfig {
    ConsoleStubConfig {
        issuer: format!("https://{HOST_MARK}"),
        audience: TOKEN_MARK.to_owned(),
        client_id: CLIENT_MARK.to_owned(),
        introspection_endpoint: format!("https://{HOST_MARK}/introspect"),
        validity_endpoint: format!("https://{HOST_MARK}/validity"),
        delivery_window_max: Duration::from_secs(30),
    }
}

fn credential_config() -> AgentCredentialConfig {
    AgentCredentialConfig {
        client_id: CLIENT_MARK.to_owned(),
        device_authorization_endpoint: format!("https://{HOST_MARK}/device"),
        token_endpoint: format!("https://{HOST_MARK}/token"),
        revocation_endpoint: format!("https://{HOST_MARK}/revoke"),
        registration_revocation_endpoint: format!("https://{HOST_MARK}/revoke-registration"),
    }
}

fn delivery_ctx() -> DeliveryContext {
    DeliveryContext {
        requirement: AuthorityRequirement {
            required_scope: "scope-a".to_owned(),
            bound_jkt: None,
        },
        decider: ValidityTarget::Session {
            sid: "sid-for-test".to_owned(),
        },
        delegation_ref: "delegation-for-test".to_owned(),
    }
}

#[test]
fn sample_starts_stubs_records_and_stops() {
    // 手順 1: 試験ごとの設定を引数の型で組む（環境変数・ファイルは読まない。DPOL-32）。
    // 手順 2: 構成要素を作り、Box に入れる前に handle() を取る。
    let hub = HubStubComponent::new(hub_config());
    let hub_handle = hub.handle();
    let console = ConsoleStubComponent::new(console_config(), credential_config());
    let console_handle = console.handle();

    // 手順 3: 渡す順は起動順と逆でも、起動順は固定。
    let running = Harness::start(
        HarnessConfig::new()
            .with(Box::new(console))
            .with(Box::new(hub)),
    )
    .unwrap();

    // 手順 4: 起動した種類を確かめる（Host・Database は起動しない）。
    assert_eq!(
        running.kinds(),
        vec![ComponentKind::HubStub, ComponentKind::ConsoleStub]
    );

    // 手順 5: Hub の stub を handle.with で呼ぶ（JWKS の往復）。
    let key = AssertionSigningKey::generate("kid-sample.test").unwrap();
    let resp = hub_handle
        .with(|s| {
            s.jwks.publish(&key);
            s.jwks.serve(T0)
        })
        .unwrap();
    assert_eq!(resp.status, 200);

    // 手順 6: console の stub を呼ぶ（配送の時点の確認。相手側の口は試験が渡す）。
    let mut ports = ScriptedDeliveryPorts::new(DeliveryScript {
        agent: AgentCredentialScript::Active {
            scope: "scope-a".to_owned(),
            jkt: None,
            exp: T0 + 100,
        },
        decider: ValidityAnswer::Valid,
        hub: ValidityAnswer::Valid,
    });
    let decision = console_handle
        .with(|s| s.delivery.check(TOKEN, &delivery_ctx(), T0, &mut ports))
        .unwrap()
        .unwrap();
    assert_eq!(decision, DeliveryDecision::Delivered { checked_at: T0 });

    // 手順 7: 実行の記録を作り、stub の記録を取り込み、追跡ツリーの外（target/ 配下）へ書く。
    let record = running
        .run_record(VersionInfo::detect().unwrap())
        .unwrap()
        .with_hub_stub_record(&hub_handle.provisional_record().unwrap())
        .unwrap()
        .with_console_contract(&console_handle.contract_record().unwrap())
        .unwrap();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("it-run-record-sample");
    let path = record.write_new(&dir).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    for line in [
        "component: Host not_started\n",
        "component: Database not_started\n",
        "component: HubStub started stub\n",
        "component: ConsoleStub started stub\n",
        "note: ",
    ] {
        assert!(text.contains(line), "記録に {line:?} が無い");
    }

    // 手順 8: 設定の値が記録・Debug に出ない（SECR-11・SECR-23）。
    let debug = format!("{record:?}");
    for mark in [HOST_MARK, CLIENT_MARK, TOKEN_MARK] {
        assert!(!text.contains(mark));
        assert!(!debug.contains(mark));
    }

    // 手順 9: 停止すると stub は呼べなくなる。
    running.stop().unwrap();
    assert!(!hub_handle.is_running());
    assert!(!console_handle.is_running());
}

#[test]
fn sample_rejects_second_instance_of_same_kind() {
    // 複数インスタンスは作らない。同じ種類の 2 つ目は起動前に拒否される。
    let cfg = HarnessConfig::new()
        .with(Box::new(HubStubComponent::new(hub_config())))
        .with(Box::new(HubStubComponent::new(hub_config())));
    let err = Harness::start(cfg).err().unwrap();
    assert_eq!(err.kind(), HarnessErrorKind::Duplicate);
    assert_eq!(err.component(), ComponentKind::HubStub);
}
