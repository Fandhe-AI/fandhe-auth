//! TASK-13 (7/8): 故障なしの基準線の実行の試験。Docker・外部ネットワークなし（上流は 127.0.0.1 の echo）。
//! 同じシナリオの本体を、モードで分岐させずに基準線・注入ありの順で流し、`run_mode` の違う 2 つの記録が分かれ、
//! 注入の段の並びが一致することを確かめる。値は試験の中だけの仮の値（THRT-29・A-2026-10-28-1）。
//! 記録は事実の出力で合否の判定ではない。stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）。

use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_hub_stub::jwks::HubJwksEndpoint;
use fandhe_auth_it_harness::fault::{
    ClockSkewInjector, DbFaultProxy, FaultKind, FaultPair, FaultPhase, FaultRecord,
    FaultRecordError, FaultStep, FaultTimeline, HubStubFaultInjector, HubStubPort, InjectError,
    PairError, RunMode,
};
use fandhe_auth_it_harness::record::UtcTimestamp;

#[path = "support/upstream.rs"]
mod upstream;
use upstream::echo_upstream;

const NOW: u64 = 1_000;

fn finished_at() -> UtcTimestamp {
    UtcTimestamp::from_unix_seconds(1_700_000_060)
}

fn step(kind: FaultKind, phase: FaultPhase) -> FaultStep {
    FaultStep { kind, phase }
}

fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn hub_config() -> HubStubConfig {
    HubStubConfig {
        client_id: StubValue::test("hub-client.test".to_owned()),
        public_host: StubValue::test("hub.example.test".to_owned()),
        jwks_uri: StubValue::test("https://hub.example.test/jwks".to_owned()),
        logout_receiver_uri: StubValue::test("https://hub.example.test/logout".to_owned()),
        jwks_max_age: StubValue::test(Duration::from_secs(1)),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(300)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
    }
}

/// 試験対象の往復。プロキシ越しに 1 往復して、そのまま返ったか。
fn echoes(port: u16) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(5),
    ) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
    if s.write_all(b"ping").is_err() {
        return false;
    }
    let mut buf = [0u8; 4];
    s.read_exact(&mut buf).is_ok() && &buf == b"ping"
}

/// 本体の外へ出す観測。実行ごとに 1 件（基準線、注入ありの順）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Obs {
    echo_while_blocked: bool,
    echo_after_release: bool,
    jwks_injected_flags: Vec<bool>,
    waits: usize,
    skew_ms_while_applied: i64,
    skew_ms_after_release: i64,
}

/// 1 つのシナリオ。モードを見ない。注入は `inject` に宣言した段だけを通して行う。
fn scenario(tl: FaultTimeline, port: u16, obs: &mut Vec<Obs>) -> Result<FaultRecord, ()> {
    let mut tl = tl;
    let mut o = Obs::default();
    let mut proxy = DbFaultProxy::start_to_port(port).map_err(|_| ())?;
    let mut hub_clock = ClockSkewInjector::new();
    let clock = hub_clock.clock();
    let waits: Rc<RefCell<usize>> = Rc::default();
    let w2 = Rc::clone(&waits);
    let mut hub = HubStubFaultInjector::new(move |_d: Duration| *w2.borrow_mut() += 1, &tl)
        .map_err(|_| ())?;

    // DB の遮断。
    tl.inject(&[step(FaultKind::DbBlock, FaultPhase::Applied)], |tl| {
        proxy.block(tl)
    })
    .map_err(|_| ())?;
    o.echo_while_blocked = echoes(proxy.listen_port());
    tl.inject(&[step(FaultKind::DbBlock, FaultPhase::Released)], |tl| {
        proxy.release_block(tl)
    })
    .map_err(|_| ())?;
    o.echo_after_release = echoes(proxy.listen_port());

    // Hub の stub の遅れ。
    let delay = FaultKind::hub_stub_delay(HubStubPort::Jwks, Duration::from_millis(5)).unwrap();
    tl.inject(&[step(delay, FaultPhase::Applied)], |tl| {
        hub.apply(tl, delay)
    })
    .map_err(|_| ())?;
    let mut jwks = hub.bind_receiver(HubJwksEndpoint::new(&hub_config()));
    hub.serve_jwks(&tl, &mut jwks, NOW).map_err(|_| ())?;

    // 時計のずれ。
    let skew = FaultKind::clock_skew(1_500).unwrap();
    tl.inject(&[step(skew, FaultPhase::Applied)], |tl| {
        hub_clock.apply(tl, 1_500)
    })
    .map_err(|_| ())?;
    o.skew_ms_while_applied = clock.skew_ms();
    tl.inject(&[step(skew, FaultPhase::Released)], |tl| {
        hub_clock.release(tl)
    })
    .map_err(|_| ())?;
    o.skew_ms_after_release = clock.skew_ms();

    let (record, log) = hub
        .finish_with_clock(&mut hub_clock, tl, finished_at())
        .map_err(|_| ())?;
    o.jwks_injected_flags = log.records().iter().map(|r| r.injected.is_some()).collect();
    o.waits = *waits.borrow();
    obs.push(o);
    Ok(record)
}

/// 受け入れ条件 (a)(b): 注入ありと同じ形で基準線を流せ、2 つの記録が分かれる。
#[test]
fn baseline_runs_the_same_body_and_records_are_separate() {
    let up = echo_upstream();
    let mut obs = Vec::new();
    let pair = FaultPair::run(|tl| scenario(tl, up.port(), &mut obs)).unwrap();
    let (b, f) = (pair.baseline(), pair.fault());
    assert_eq!((b.mode(), f.mode()), (RunMode::Baseline, RunMode::Fault));
    // 基準線: 事象 0 件・未解除 0 件・同じキー。
    assert!(b.to_string().contains("run_mode: baseline\n"));
    assert!(b.to_string().contains("injection_count: 0\n"));
    assert_eq!(b.unreleased_count(), 0);
    assert!(b.events().is_empty());
    // 注入あり: 段の数だけ事象がある。
    assert!(f.to_string().contains("run_mode: fault\n"));
    assert_eq!(f.events().len(), f.injection_points().len());
    assert_eq!(f.events().len(), 5);
    // 段の並びは一致する。
    assert_eq!(b.injection_points(), f.injection_points());
    // 記録の形式（キーの集合）は同じ。stub の注記は stub の事象があるときだけ増える既存の動き。
    let keys = |r: &FaultRecord| {
        r.to_string()
            .lines()
            .filter_map(|l| l.split_once(": ").map(|(k, _)| k.to_owned()))
            .filter(|k| k != "injection")
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(keys(b), keys(f));
    // 観測: 基準線は何も起きず、注入ありでは効いている。
    let [ob, of] = obs.as_slice() else {
        panic!("two runs")
    };
    assert!(ob.echo_while_blocked && ob.echo_after_release);
    assert_eq!(ob.jwks_injected_flags, [false]);
    assert_eq!((ob.waits, ob.skew_ms_while_applied), (0, 0));
    assert!(!of.echo_while_blocked && of.echo_after_release);
    assert_eq!(of.waits, 1);
    assert_eq!(of.skew_ms_while_applied, 1_500);
    assert_eq!(of.skew_ms_after_release, 0);
}

/// 2 つの記録は別のファイルへ書かれ、名前に種別が入る。中身は `to_string` と一致し、パス・ポートを含まない。
#[test]
fn write_new_makes_two_distinct_files() {
    let up = echo_upstream();
    let mut obs = Vec::new();
    let pair = FaultPair::run(|tl| scenario(tl, up.port(), &mut obs)).unwrap();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fault_baseline");
    let (pb, pf) = pair.write_new(&dir).unwrap();
    assert_ne!(pb, pf);
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name(&pb).starts_with("it-fault-baseline-"));
    assert!(name(&pf).starts_with("it-fault-fault-"));
    assert_eq!(
        std::fs::read_to_string(&pb).unwrap(),
        pair.baseline().to_string()
    );
    assert_eq!(
        std::fs::read_to_string(&pf).unwrap(),
        pair.fault().to_string()
    );
    let all = format!("{}{}{pair:?}", pair.baseline(), pair.fault(),);
    assert!(!all.contains(&dir.to_string_lossy().into_owned()));
    assert!(!all.contains(&up.port().to_string()));
    let _ = std::fs::remove_file(pb);
    let _ = std::fs::remove_file(pf);
}

/// 基準線で一度も適用しなかった 2 つの注入器も、確定できる。
#[test]
fn unused_injectors_can_finish_in_baseline() {
    let tl = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    let mut clock = ClockSkewInjector::new();
    let hub = HubStubFaultInjector::new(|_: Duration| {}, &tl).unwrap();
    let (rec, log) = hub
        .finish_with_clock(&mut clock, tl, finished_at())
        .unwrap();
    assert_eq!(rec.mode(), RunMode::Baseline);
    assert!(log.records().is_empty());
}

/// 本体がモードで分岐して段の並びが違うと `ShapeMismatch`。両方の記録が返る。
#[test]
fn branching_body_is_a_shape_mismatch() {
    let n = Cell::new(0);
    let err = FaultPair::run(|mut tl| -> Result<FaultRecord, ()> {
        let i = n.get();
        n.set(i + 1);
        if i == 0 {
            tl.inject(
                &[step(FaultKind::DbBlock, FaultPhase::Applied)],
                |_| -> Result<(), ()> { Ok(()) },
            )
            .map_err(|_| ())?;
        } else {
            let k = FaultKind::db_delay(Duration::from_millis(5)).unwrap();
            tl.inject(&[step(k, FaultPhase::Applied)], |tl| {
                tl.record_now(k, FaultPhase::Applied)
            })
            .map_err(|_| ())?;
        }
        tl.finish(finished_at()).map_err(|_| ())
    })
    .unwrap_err();
    let PairError::ShapeMismatch(p) = err else {
        panic!("shape mismatch")
    };
    assert_ne!(
        p.baseline().injection_points(),
        p.fault().injection_points()
    );
}

/// `inject` を経ない直接記録の事象は、基準線に何も無くても `ShapeMismatch`（段と事象が一対一でない）。
#[test]
fn direct_record_without_inject_is_a_shape_mismatch() {
    let n = Cell::new(0);
    let err = FaultPair::run(|mut tl| -> Result<FaultRecord, ()> {
        n.set(n.get() + 1);
        if n.get() == 2 {
            tl.record_now(FaultKind::DbBlock, FaultPhase::Applied)
                .map_err(|_| ())?;
            tl.record_now(FaultKind::DbBlock, FaultPhase::Released)
                .map_err(|_| ())?;
        }
        tl.finish(finished_at()).map_err(|_| ())
    })
    .unwrap_err();
    let PairError::ShapeMismatch(p) = err else {
        panic!("shape mismatch")
    };
    assert!(p.baseline().events().is_empty());
    assert_eq!(p.fault().events().len(), 2);
    assert!(p.fault().injection_points().is_empty());
}

/// 渡していない別の実行の記録を返す本体は `RunMismatch`。
#[test]
fn record_of_another_run_is_a_run_mismatch() {
    let err = FaultPair::run(|_tl| -> Result<FaultRecord, ()> {
        FaultTimeline::baseline(Instant::now(), t0())
            .unwrap()
            .finish(finished_at())
            .map_err(|_| ())
    })
    .unwrap_err();
    assert!(matches!(
        err,
        PairError::RunMismatch {
            mode: RunMode::Baseline
        }
    ));
}

/// 本体の誤りはどちらの実行かを付けて返す。注入ありで 0 件なら確定が拒否され、`Body` で返る。
#[test]
fn body_errors_name_the_run() {
    let err = FaultPair::run(|_| -> Result<FaultRecord, ()> { Err(()) }).unwrap_err();
    assert!(matches!(
        err,
        PairError::Body {
            mode: RunMode::Baseline,
            ..
        }
    ));
    let err = FaultPair::run(|tl| tl.finish(finished_at()).map_err(|f| f.error)).unwrap_err();
    assert!(matches!(
        err,
        PairError::Body {
            mode: RunMode::Fault,
            error: FaultRecordError::NoInjectionInFaultRun
        }
    ));
    assert_eq!(format!("{err:?}"), "Body { mode: Fault, .. }".to_owned());
}

/// `inject` の基準線の検査: 操作を呼ばず、宣言を注入ありと同じ規則で検査する。
#[test]
fn baseline_inject_validates_steps_without_calling_op() {
    let mut tl = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    let called = Cell::new(false);
    let op = |_: &mut FaultTimeline| -> Result<(), ()> {
        called.set(true);
        Ok(())
    };
    let blk = FaultKind::DbBlock;
    // 空・対応する Applied の無い Released・一回きりの事象の Released。
    assert_eq!(
        tl.inject(&[], op).unwrap_err(),
        InjectError::Record(FaultRecordError::InvalidParam)
    );
    assert_eq!(
        tl.inject(&[step(blk, FaultPhase::Released)], op)
            .unwrap_err(),
        InjectError::Record(FaultRecordError::ReleaseWithoutApply)
    );
    assert_eq!(
        tl.inject(
            &[step(FaultKind::InstanceSigterm, FaultPhase::Released)],
            op
        )
        .unwrap_err(),
        InjectError::Record(FaultRecordError::ReleaseOfInstantaneous)
    );
    assert!(
        tl.inject(&[step(blk, FaultPhase::Applied)], op)
            .unwrap()
            .is_none()
    );
    assert!(
        tl.inject(&[step(blk, FaultPhase::Released)], op)
            .unwrap()
            .is_none()
    );
    assert!(!called.get());
    let rec = tl.finish(finished_at()).unwrap();
    assert_eq!(rec.injection_points().len(), 2);
    assert!(rec.events().is_empty());
}

/// 注入ありの `inject`: 宣言と積まれた事象の食い違いは `Mismatch`、記録の後の失敗は段に数えて `Op`。
#[test]
fn fault_inject_checks_declared_steps_against_events() {
    let mut tl = FaultTimeline::start(Instant::now(), t0()).unwrap();
    let blk = FaultKind::DbBlock;
    // 何も積まずに成功: 食い違い。
    assert_eq!(
        tl.inject(&[step(blk, FaultPhase::Applied)], |_| -> Result<(), ()> {
            Ok(())
        })
        .unwrap_err(),
        InjectError::Mismatch
    );
    // 何も積まずに失敗: 操作の失敗。
    assert_eq!(
        tl.inject(&[step(blk, FaultPhase::Applied)], |_| -> Result<(), u8> {
            Err(7)
        })
        .unwrap_err(),
        InjectError::Op(7)
    );
    // 記録の後に失敗: 事実なので段に数える。
    assert_eq!(
        tl.inject(&[step(blk, FaultPhase::Applied)], |tl| {
            tl.record_now(blk, FaultPhase::Applied).unwrap();
            Err::<(), u8>(9)
        })
        .unwrap_err(),
        InjectError::Op(9)
    );
    // 段の種類が違う: 食い違い（積まれた事象は残る）。
    let d = FaultKind::db_delay(Duration::from_millis(5)).unwrap();
    assert_eq!(
        tl.inject(&[step(blk, FaultPhase::Released)], |tl| {
            tl.record_now(d, FaultPhase::Applied)
        })
        .unwrap_err(),
        InjectError::Mismatch
    );
    let rec = tl.finish(finished_at()).unwrap();
    assert_eq!(rec.injection_points().len(), 1);
    assert_eq!(rec.events().len(), 2);
}
