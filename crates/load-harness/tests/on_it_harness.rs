//! TASK-12 5/10: 結合試験のハーネス（`crates/it-harness`）の上で、Hub の stub へ負荷をかける試験。
//!
//! 呼び出し元は試験ビルドだけ。Docker・ネットワークなし（`make test` で必ず流れる。`#[ignore]` を付けない）。
//! 値は試験の中だけの仮の値（A-2026-10-28-1・A-2026-10-28-14）。件数だけを見て、時間の値は判定しない。
//! stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）。Host は TASK-716 の完了まで起動しない。
//! 区間名 `stub_jwks`・失敗の種類名は仮置き。

use std::time::Duration;

use fandhe_auth_hub_stub::assertion::AssertionSigningKey;
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_it_harness::{Harness, HarnessConfig, HubStubComponent};
use fandhe_auth_load_harness::{
    CallResult, FailureKind, IntervalName, LoadConfig, Outcome, RunMeter, StdMonotonicClock,
    TargetName, TargetSpec, Trial, run_closed_loop, run_target, summarize_run,
};

const T0: u64 = 1_000;
const HOST_MARK: &str = "load-canary-host.example.test";
const CLIENT_MARK: &str = "load-canary-client.test";

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

fn new_meter(name: &IntervalName) -> RunMeter {
    RunMeter::new(
        Box::new(StdMonotonicClock::new()),
        std::slice::from_ref(name),
    )
    .unwrap()
}

#[test]
fn loads_hub_stub_on_harness_then_sees_stop() {
    let name = IntervalName::new("stub_jwks").unwrap();
    let unavailable = FailureKind::new("stub_unavailable").unwrap();
    let unexpected = FailureKind::new("unexpected_status").unwrap();

    let hub = HubStubComponent::new(hub_config());
    let handle = hub.handle();
    let running = Harness::start(HarnessConfig::new().with(Box::new(hub))).unwrap();
    let key = AssertionSigningKey::generate("kid-load.test").unwrap();
    handle.with(|s| s.jwks.publish(&key)).unwrap();

    let target = |t: &mut Trial<'_>| {
        let resp = t
            .measure(&name, || handle.with(|s| s.jwks.serve(T0)))
            .unwrap();
        match resp {
            Some(r) if r.status == 200 => Outcome::Success,
            Some(_) => Outcome::Failure(unexpected.clone()),
            None => Outcome::Failure(unavailable.clone()),
        }
    };

    let config = LoadConfig::new(4, 40, 4).unwrap();
    let run = run_closed_loop(new_meter(&name), &config, target).unwrap();
    let s = &run.samples;
    assert_eq!((s.executed, s.success, s.excluded), (40, 40, 4));
    assert!((1..=4).contains(&s.max_in_flight));
    assert_eq!(
        summarize_run(s, std::slice::from_ref(&name))
            .unwrap()
            .success_metrics[&name]
            .n,
        40
    );

    running.stop().unwrap();

    // 停止後は呼べず、失敗として数えられる（ハーネスの状態が結果に出る）。
    let config = LoadConfig::new(2, 6, 0).unwrap();
    let run = run_closed_loop(new_meter(&name), &config, target).unwrap();
    assert_eq!(run.samples.failure, 6);
    assert_eq!(run.samples.failure_kinds[&unavailable], 6);
}

#[test]
fn run_target_calls_hub_stub_jwks_on_harness() {
    let hub = HubStubComponent::new(hub_config());
    let handle = hub.handle();
    let running = Harness::start(HarnessConfig::new().with(Box::new(hub))).unwrap();
    let key = AssertionSigningKey::generate("kid-load-target.test").unwrap();
    handle.with(|s| s.jwks.publish(&key)).unwrap();

    let spec = TargetSpec::new(
        TargetName::new("hub_stub_jwks").unwrap(),
        IntervalName::new("stub_jwks").unwrap(),
    );
    let call = || match handle.with(|s| s.jwks.serve(T0)) {
        Some(r) if r.status == 200 => CallResult::Ok,
        Some(_) => CallResult::Rejected,
        None => CallResult::Unavailable,
    };

    let config = LoadConfig::new(4, 40, 4).unwrap();
    let r = run_target(Box::new(StdMonotonicClock::new()), &spec, &config, call).unwrap();
    let s = &r.run.samples;
    assert_eq!((s.executed, s.success, s.excluded), (40, 40, 4));
    assert!((1..=4).contains(&s.max_in_flight));
    assert_eq!(r.summary.success_metrics[spec.interval()].n, 40);
    // 経路を実際に呼んだ証拠: stub 側の記録が暖機を含む全件に一致する。
    assert_eq!(handle.with(|s| s.jwks.records().len()), Some(44));
    let dbg = format!("{r:?}");
    assert!(!dbg.contains(HOST_MARK) && !dbg.contains(CLIENT_MARK));

    running.stop().unwrap();

    let config = LoadConfig::new(2, 6, 0).unwrap();
    let r = run_target(Box::new(StdMonotonicClock::new()), &spec, &config, call).unwrap();
    assert_eq!(r.run.samples.failure, 6);
}
