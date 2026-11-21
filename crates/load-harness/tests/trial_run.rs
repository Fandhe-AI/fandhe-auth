//! TASK-12 9/10: 小さな試し実行。it-harness の上で Hub の stub に対して段階付きの負荷を最後まで流し、
//! 記録のファイルが出ること・どの値も要件の値として記録されないことを確かめる。
//!
//! Docker・ネットワークなし（`make test` で必ず流れる。`#[ignore]` を付けない）。値は試験の中だけの仮の値
//! （A-2026-10-28-1・A-2026-10-28-14）。時間の値は判定しない。stub の合格を実物の合格に数えない
//! （A-2026-10-28-9・ACPT-7）。出たファイルは追跡ツリーの外（`CARGO_TARGET_TMPDIR`）に置く。

use std::path::Path;
use std::time::Duration;

use fandhe_auth_hub_stub::assertion::AssertionSigningKey;
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_it_harness::{
    ComponentKind, Harness, HarnessConfig, HubStubComponent, VersionInfo,
};
use fandhe_auth_load_harness::{
    ApprovedValues, CallResult, ComparisonValue, ComponentConditions, ConditionComponent,
    IntervalName, LoadConfig, LoadRecord, LoopModel, MeasurementConditions, MetricKey, RefId,
    RunRecordFile, RunSession, StageConfig, StageLimits, StagePlan, TargetName, TargetSpec,
    ValueClass, ValueName, ValueUnit,
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

fn to_condition(kind: ComponentKind) -> ConditionComponent {
    match kind {
        ComponentKind::Database => ConditionComponent::Database,
        ComponentKind::Host => ConditionComponent::Host,
        ComponentKind::HubStub => ConditionComponent::HubStub,
        ComponentKind::ConsoleStub => ConditionComponent::ConsoleStub,
    }
}

const META: [&str; 6] = [
    "format:",
    "run_record_file:",
    "run_started_at:",
    "recorded_at:",
    "note:",
    "judgment:",
];

#[test]
fn small_trial_run_writes_record_without_requirement_values() {
    let hub = HubStubComponent::new(hub_config());
    let handle = hub.handle();
    let running = Harness::start(HarnessConfig::new().with(Box::new(hub))).unwrap();
    let key = AssertionSigningKey::generate("kid-load-trial.test").unwrap();
    handle.with(|s| s.jwks.publish(&key)).unwrap();

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("load-trial");
    let it_record = running.run_record(VersionInfo::detect().unwrap()).unwrap();
    let it_path = it_record.write_new(&dir).unwrap();
    let it_name = it_path.file_name().unwrap().to_str().unwrap().to_owned();
    let run_file = RunRecordFile::new(&it_name).unwrap();
    let started = it_record.started_at().unix_seconds();

    let started_kinds: Vec<(ConditionComponent, bool)> = running
        .composition_record()
        .entries
        .iter()
        .map(|e| (to_condition(e.kind), e.stub))
        .collect();
    let conditions = MeasurementConditions::detect(
        ComponentConditions::new(&started_kinds).unwrap(),
        LoopModel::Closed,
    )
    .unwrap();

    let spec = TargetSpec::new(
        TargetName::new("hub_stub_jwks").unwrap(),
        IntervalName::new("stub_jwks").unwrap(),
    );
    let plan = StagePlan::new(
        vec![
            StageConfig::new(LoadConfig::new(1, 10, 1).unwrap(), 1).unwrap(),
            StageConfig::new(LoadConfig::new(2, 20, 2).unwrap(), 2).unwrap(),
        ],
        MetricKey::P99Ns,
        StageLimits::new(8, 8, 1_000, 10_000).unwrap(),
        vec![IntervalName::new("stub_jwks").unwrap()],
    )
    .unwrap();
    let call = || match handle.with(|s| s.jwks.serve(T0)) {
        Some(r) if r.status == 200 => CallResult::Ok,
        Some(_) => CallResult::Rejected,
        None => CallResult::Unavailable,
    };
    // 段階は実行の文脈（実行の記録・測定の条件）の中で回し、呼ぶ構成要素は呼ぶ前に申告する。
    // 閉包が実際に Hub の stub を呼ぶことは、この試験が stub 側の呼び出しの件数で確かめる（下の assert）。
    let evidence = RunSession::new(run_file, started, conditions)
        .unwrap()
        .run(&[ConditionComponent::HubStub], &spec, &plan, call)
        .unwrap();
    // 経路を実際に呼んだ証拠: 暖機を含む全試行数（11 x 1 + 22 x 2）。
    assert_eq!(handle.with(|s| s.jwks.records().len()), Some(55));

    // 性能の目標は持ち越し（AUDP-29）。要件の値として記録せず、未決を明示する。
    let target = ComparisonValue::new(
        ValueName::new("p99_target", &["p99_target"]).unwrap(),
        ValueClass::Undecided,
        None,
        ValueUnit::Ns,
        vec![RefId::new("AUDP-29", &["AUDP-29"]).unwrap()],
        &ApprovedValues::default(),
    )
    .unwrap();
    // 記録を組み立てる時点の実測時刻。逆行していれば LoadRecord が ClockOrder で拒否する（補正しない）。
    let recorded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let record = LoadRecord::new(&evidence, recorded_at, vec![target]).unwrap();
    let path = record.write_new(&dir).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, record.to_string());

    let name = path.file_name().unwrap().to_str().unwrap();
    let pid = std::process::id();
    assert!(name.starts_with(&format!("load-run-{started}-{pid}-")) && name.ends_with(".txt"));
    assert!(it_path.parent() == path.parent() && it_path.exists());

    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "format: fandhe-auth-load-record/1");
    assert_eq!(lines[1], format!("run_record_file: {it_name}"));
    assert_eq!(
        lines[2],
        format!("run_started_at: {}", it_record_started_line(&it_record))
    );
    let count = |p: &str| lines.iter().filter(|l| l.starts_with(p)).count();
    assert_eq!(count("stage:"), 2);
    assert_eq!(count("run:"), 3);
    assert_eq!(count("metric:"), 3);
    assert_eq!(count("worst_run:"), 2);
    for l in lines.iter().filter(|l| l.starts_with("run:")) {
        assert!(l.contains("failure=0 timeout=0"));
    }
    assert!(
        lines
            .iter()
            .any(|l| l.contains("trials_planned=10 warmup=1"))
    );
    assert!(lines.iter().any(|l| l.contains("executed=20 success=20")));
    assert!(lines.iter().any(|l| l.contains("excluded=2")));

    // どの値も要件の値として扱わない。
    for c in ["class=requirement", "class=initial", "class=poc_parameter"] {
        assert!(!text.contains(c));
    }
    let values: Vec<&&str> = lines.iter().filter(|l| l.starts_with("value:")).collect();
    assert_eq!(values.len(), 1);
    assert!(values[0].contains("class=undecided value=none"));
    for l in &lines {
        if META.iter().any(|m| l.starts_with(m)) {
            continue;
        }
        assert!(l.contains(" class=") && l.contains(" refs="), "{l}");
    }
    assert_eq!(*lines.last().unwrap(), "judgment:");
    assert!(text.ends_with('\n'));

    // 設定の目印・出力先のパスが出ない。
    let dir_str = dir.to_str().unwrap();
    let dbg = format!("{record:?}");
    for out in [&text, &dbg] {
        assert!(!out.contains(HOST_MARK) && !out.contains(CLIENT_MARK));
        assert!(!out.contains(dir_str));
    }

    // 上書きしない: 同じ記録をもう一度書くと別名の新しいファイルになる。
    let path2 = record.write_new(&dir).unwrap();
    assert_ne!(path, path2);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

    running.stop().unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(&path2).unwrap();
    std::fs::remove_file(&it_path).unwrap();
}

/// it-harness の記録の `started_at` の RFC 3339 表記（`UtcTimestamp` の `Display`）。
fn it_record_started_line(r: &fandhe_auth_it_harness::RunRecord) -> String {
    r.started_at().to_string()
}

// ---- AAUD-30・SECR-23: 登録簿の値を 4 つの形で記録のファイルに掛ける照合 ----

/// 実行時に乱数から作る 16 バイトの値（固定の値をソースに置かない）。`RandomState` は実行ごとに種が変わる。
fn random_value() -> Vec<u8> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher as _, Hasher as _};
    let state = RandomState::new();
    let mut out = Vec::with_capacity(16);
    for i in 0..2u64 {
        let mut h = state.build_hasher();
        h.write_u64(i);
        out.extend_from_slice(&h.finish().to_be_bytes());
    }
    out
}

fn hex(v: &[u8], upper: bool) -> String {
    v.iter()
        .map(|b| {
            if upper {
                format!("{b:02X}")
            } else {
                format!("{b:02x}")
            }
        })
        .collect()
}

fn base64_with(v: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::new();
    for c in v.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        let take = c.len() + 1;
        for i in 0..4 {
            if i < take {
                out.push(char::from(alphabet[((n >> (18 - 6 * i)) & 63) as usize]));
            } else if pad {
                out.push('=');
            }
        }
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// 値ごとの照合の形: 生・base64（パディングあり・なし）・base64url（なし）・16 進（小文字・大文字）。
/// 生のバイト列は UTF-8 とは限らないためバイト列で探す。
fn forms(v: &[u8]) -> Vec<Vec<u8>> {
    vec![
        v.to_vec(),
        base64_with(v, B64, true).into_bytes(),
        base64_with(v, B64, false).into_bytes(),
        base64_with(v, B64URL, false).into_bytes(),
        hex(v, false).into_bytes(),
        hex(v, true).into_bytes(),
    ]
}

/// 登録簿の全値・全形が `haystack` に現れる件数だけを返す（値は返さない）。
fn count_matches(registry: &[Vec<u8>], haystack: &[u8]) -> usize {
    registry
        .iter()
        .flat_map(|v| forms(v))
        .map(|f| {
            haystack
                .windows(f.len())
                .filter(|w| *w == f.as_slice())
                .count()
        })
        .sum()
}

/// 負荷の記録を作って書く（呼び出し元の入力は `cfg`。`hub_config` と同じ組み立て）。
fn write_record_for(cfg: HubStubConfig) -> std::path::PathBuf {
    let hub = HubStubComponent::new(cfg);
    let handle = hub.handle();
    let running = Harness::start(HarnessConfig::new().with(Box::new(hub))).unwrap();
    let key = AssertionSigningKey::generate("kid-load-trial.test").unwrap();
    handle.with(|s| s.jwks.publish(&key)).unwrap();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("load-trial-canary");
    let it_record = running.run_record(VersionInfo::detect().unwrap()).unwrap();
    let it_path = it_record.write_new(&dir).unwrap();
    let it_name = it_path.file_name().unwrap().to_str().unwrap().to_owned();
    let started_kinds: Vec<(ConditionComponent, bool)> = running
        .composition_record()
        .entries
        .iter()
        .map(|e| (to_condition(e.kind), e.stub))
        .collect();
    let conditions = MeasurementConditions::detect(
        ComponentConditions::new(&started_kinds).unwrap(),
        LoopModel::Closed,
    )
    .unwrap();
    let spec = TargetSpec::new(
        TargetName::new("hub_stub_jwks").unwrap(),
        IntervalName::new("stub_jwks").unwrap(),
    );
    let plan = StagePlan::new(
        vec![StageConfig::new(LoadConfig::new(1, 5, 1).unwrap(), 1).unwrap()],
        MetricKey::P99Ns,
        StageLimits::new(8, 8, 1_000, 10_000).unwrap(),
        vec![IntervalName::new("stub_jwks").unwrap()],
    )
    .unwrap();
    let call = || match handle.with(|s| s.jwks.serve(T0)) {
        Some(r) if r.status == 200 => CallResult::Ok,
        _ => CallResult::Unavailable,
    };
    let evidence = RunSession::new(
        RunRecordFile::new(&it_name).unwrap(),
        it_record.started_at().unix_seconds(),
        conditions,
    )
    .unwrap()
    .run(&[ConditionComponent::HubStub], &spec, &plan, call)
    .unwrap();
    let recorded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let record = LoadRecord::new(&evidence, recorded_at, Vec::new()).unwrap();
    let path = record.write_new(&dir).unwrap();
    running.stop().unwrap();
    std::fs::remove_file(&it_path).unwrap();
    path
}

/// 登録簿（実行時に作る 3 値）と、その値を呼び出し元の入力（接続の設定）へ入れた構成。
fn canary_registry_and_config() -> (Vec<Vec<u8>>, HubStubConfig) {
    let registry: Vec<Vec<u8>> = (0..3).map(|_| random_value()).collect();
    let mut cfg = hub_config();
    cfg.client_id = StubValue::test(hex(&registry[0], false));
    cfg.public_host = StubValue::test(format!("{}.example.test", hex(&registry[1], false)));
    cfg.jwks_uri = StubValue::test(format!("https://{}/jwks", hex(&registry[2], false)));
    (registry, cfg)
}

/// AAUD-30・SECR-23: 呼び出し元の入力に入れた登録簿の値が、書き出した記録の全文に、
/// 生・base64・base64url・16 進のどの形でも現れない（一致の件数が 0）。
#[test]
fn written_record_has_no_registered_secret_in_any_form() {
    let (registry, cfg) = canary_registry_and_config();
    let path = write_record_for(cfg);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!bytes.is_empty());
    let n = count_matches(&registry, &bytes);
    assert_eq!(n, 0, "registered values found in the record: {n} match(es)");
}

/// 陽性対照（AAUD-31 の趣旨）: 登録簿の値を記録に混ぜると、照合は形ごとに検出する。
/// 16 進 32 文字・base64 系の語は 64 バイト以下の英数字で、形の網（`is_fixed_shape`）の内側に収まる値である。
/// これが検出できなければ、上の「0 件」は空振りの可能性がある。
#[test]
fn matcher_detects_registered_secret_mixed_into_record() {
    let (registry, cfg) = canary_registry_and_config();
    let path = write_record_for(cfg);
    let base = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(count_matches(&registry, &base), 0);
    for (vi, v) in registry.iter().enumerate() {
        for (fi, f) in forms(v).iter().enumerate() {
            let mut mixed = base.clone();
            mixed.extend_from_slice(b"note: ");
            mixed.extend_from_slice(f);
            mixed.push(b'\n');
            let n = count_matches(&registry, &mixed);
            assert!(n >= 1, "form {fi} of value {vi} was not detected");
        }
    }
}
