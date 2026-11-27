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
    IntervalName, LoadConfig, LoadRecord, LoadRecordError, LoopModel, MeasurementConditions,
    MetricKey, RefId, RunRecordFile, RunSession, StageConfig, StageError, StageLimits, StagePlan,
    TargetName, TargetSpec, ValueClass, ValueName, ValueUnit,
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
        ValueName::new("p99_target").unwrap(),
        ValueClass::Undecided,
        None,
        ValueUnit::Ns,
        vec![RefId::new("AUDP-29").unwrap()],
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

// ---- 照合の試験 ----
//
// AAUD-30・AAUD-31 の方法（登録簿の各値を生・base64・base64url・16 進の形で出力に掛け、一致の件数を数える）を
// ハーネスの出力に当てはめたもので、AAUD-30・AAUD-31 を満たしたものではない（実環境の走査は Hub〔`AOPS-73`〕）。
// 対象は「stub の設定の 3 欄から負荷の記録へ値が流れないこと」の回帰の検出と、it-run の記録（設定の入る口
// `with_hub_stub_record` を通して書いたもの）に登録簿の値が現れないことの確認まで。it-run の記録に入るのは
// 欄名（`&'static str`）と印（`ValueMark`）だけで、設定の値が届く経路は無い。そのため it-run の記録側の
// 陽性対照は置かない（設定から値が届く経路が無い）。照合そのものの検出力は負荷の記録側の陽性対照で見る。
// 署名鍵（`AssertionSigningKey`）の秘密値は登録簿に入れておらず対象外で、未確認のまま持ち越し。
// TODO(TASK-500): 秘密値を登録簿に入れて走査する（AAUD-30・AAUD-31。#1391）。
// 出力の面の秘密値の扱いは TASK-518（SECR-23。#1407）。

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

/// 符号化の既知の入力と出力の対（RFC 4648 の 10 節の例・16 進・`+`/`/` を含む入力）。
/// 符号化が誤っていると、照合が何も見つけないまま「0 件」になるため、対で確かめる。
#[test]
fn encoders_match_rfc4648_vectors() {
    let b64 = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];
    for (i, o) in b64 {
        assert_eq!(base64_with(i.as_bytes(), B64, true), o);
        assert_eq!(
            base64_with(i.as_bytes(), B64, false),
            o.trim_end_matches('=')
        );
        assert_eq!(
            base64_with(i.as_bytes(), B64URL, false),
            o.trim_end_matches('=')
        );
    }
    // 62・63 番目の文字: 標準は `+` `/`、URL 安全は `-` `_`。
    assert_eq!(base64_with(&[0xfb, 0xff], B64, true), "+/8=");
    assert_eq!(base64_with(&[0xfb, 0xff], B64URL, false), "-_8");
    assert_eq!(hex(b"foobar", false), "666f6f626172");
    assert_eq!(hex(&[0xab, 0xcd], true), "ABCD");
    assert_eq!(hex(&[0xab, 0xcd], false), "abcd");
}

/// 登録簿の全値・全形が `haystack` に現れる件数だけを返す（値は返さない）。
fn count_matches(registry: &[Vec<u8>], haystack: &[u8]) -> usize {
    match_counts(registry, haystack).iter().flatten().sum()
}

/// 登録簿の値ごと・形ごとの一致の件数（`[値の番号][形の番号]`。形の順は `forms`）。`count_matches` はこの合計。
/// 陽性対照は、混ぜた形そのものの件数を見る（合計だけだと、ある形が別の形の部分列として見つかり、
/// その形の照合が外れても通る。例: パディングありの base64 はパディングなしの形を含む）。
fn match_counts(registry: &[Vec<u8>], haystack: &[u8]) -> Vec<Vec<usize>> {
    registry
        .iter()
        .map(|v| {
            forms(v)
                .iter()
                .map(|f| {
                    haystack
                        .windows(f.len())
                        .filter(|w| *w == f.as_slice())
                        .count()
                })
                .collect()
        })
        .collect()
}

/// JWT の形（`eyJ<英数字・-・_>.eyJ<…>.` の部分列）の件数。語の頭に限らず、本文のどこにあっても数える
/// （`x.`・`name_` などが前に付いた形も数える）。
fn count_jwt_like(haystack: &[u8]) -> usize {
    let seg = |from: usize| {
        let mut i = from;
        while i < haystack.len()
            && (haystack[i].is_ascii_alphanumeric() || matches!(haystack[i], b'-' | b'_'))
        {
            i += 1;
        }
        i
    };
    (0..haystack.len())
        .filter(|&p| haystack[p..].starts_with(b"eyJ"))
        .filter(|&p| {
            let e1 = seg(p + 3);
            haystack[e1..].starts_with(b".eyJ") && haystack.get(seg(e1 + 4)) == Some(&b'.')
        })
        .count()
}

/// 書き出した 2 つのファイルの全文。
struct Written {
    load: Vec<u8>,
    it_run: Vec<u8>,
}

/// 負荷の記録を作って書く（呼び出し元の入力は `cfg`。`hub_config` と同じ組み立て）。測る区間の名前は固定語彙の `stub_jwks`。負荷の記録と it-run の記録の全文を読み、消してから返す。
fn write_record_for(cfg: HubStubConfig) -> Written {
    let hub = HubStubComponent::new(cfg);
    let handle = hub.handle();
    let running = Harness::start(HarnessConfig::new().with(Box::new(hub))).unwrap();
    let key = AssertionSigningKey::generate("kid-load-trial.test").unwrap();
    handle.with(|s| s.jwks.publish(&key)).unwrap();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("load-trial-canary");
    // 設定が it-run の記録に入る口は `with_hub_stub_record` だけ（欄名と印のみを取り込む）。その口を通して書く。
    let it_record = running
        .run_record(VersionInfo::detect().unwrap())
        .unwrap()
        .with_hub_stub_record(&handle.provisional_record().unwrap())
        .unwrap();
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
    let out = Written {
        load: std::fs::read(&path).unwrap(),
        it_run: std::fs::read(&it_path).unwrap(),
    };
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(&it_path).unwrap();
    out
}

/// 登録簿と、その値を呼び出し元の入力（接続の設定の 3 欄）へ入れた構成。
/// 登録簿は、乱数の 3 値（設定には 16 進で入る）と、3 欄に入れた文字列そのものの全文 3 件の計 6 件。
fn canary_registry_and_config() -> (Vec<Vec<u8>>, HubStubConfig) {
    let raw: Vec<Vec<u8>> = (0..3).map(|_| random_value()).collect();
    let client_id = hex(&raw[0], false);
    let public_host = format!("{}.example.test", hex(&raw[1], false));
    let jwks_uri = format!("https://{}/jwks", hex(&raw[2], false));
    let mut registry = raw;
    registry.extend([
        client_id.clone().into_bytes(),
        public_host.clone().into_bytes(),
        jwks_uri.clone().into_bytes(),
    ]);
    let mut cfg = hub_config();
    cfg.client_id = StubValue::test(client_id);
    cfg.public_host = StubValue::test(public_host);
    cfg.jwks_uri = StubValue::test(jwks_uri);
    (registry, cfg)
}

/// AAUD-30・SECR-23（方法を当てはめたもの）: stub の設定の 3 欄に入れた登録簿の値（乱数の値と欄の全文）が、
/// 負荷の記録と、設定の口（`with_hub_stub_record`）を通した it-run の記録の全文に、生・base64・base64url・16 進のどの形でも現れない（一致の件数が 0）。
/// JWT の形の語も 0 件。
#[test]
fn written_records_have_no_registered_secret_in_any_form() {
    let (registry, cfg) = canary_registry_and_config();
    let w = write_record_for(cfg);
    assert!(!w.load.is_empty() && !w.it_run.is_empty());
    // 設定の口（`with_hub_stub_record`）を通った行が it-run の記録に実際に書き出されている（欄名と印だけ）。
    let it_text = String::from_utf8(w.it_run.clone()).unwrap();
    for field in ["client_id", "public_host", "jwks_uri"] {
        assert!(
            it_text
                .lines()
                .any(|l| l.starts_with(&format!("hub_stub_field: {field} "))),
            "hub_stub_field line for {field} missing from the it-run record"
        );
    }
    for (what, bytes) in [("load record", &w.load), ("it-run record", &w.it_run)] {
        let n = count_matches(&registry, bytes);
        assert_eq!(n, 0, "registered values found in the {what}: {n} match(es)");
        let j = count_jwt_like(bytes);
        assert_eq!(j, 0, "JWT-like words found in the {what}: {j}");
    }
}

/// 陽性対照 1（照合の検出力）: 登録簿の全値・全形（生・`+` を含む base64 も含む）を出力に混ぜると、
/// 照合は形ごとに検出する（混ぜた値の、混ぜた形そのものの件数が 1 以上。合計の件数では見ない）。JWT の形の語も数える。生のバイトと `+` を含む base64 は形の網の外にあり、
/// `write_new` を通らない。ここは照合そのものの検出力を見る（`write_new` に届く前に、登録簿の 16 進の文字列が語彙外の名前・出典・区間名として拒否されることを次の試験で見る）。
#[test]
fn matcher_detects_every_form_mixed_into_output() {
    let (registry, cfg) = canary_registry_and_config();
    let w = write_record_for(cfg);
    assert_eq!(count_matches(&registry, &w.load), 0);
    for (vi, v) in registry.iter().enumerate() {
        for (fi, f) in forms(v).iter().enumerate() {
            let mut mixed = w.load.clone();
            mixed.extend_from_slice(b"note: ");
            mixed.extend_from_slice(f);
            mixed.push(b'\n');
            let n = match_counts(&registry, &mixed)[vi][fi];
            assert!(n >= 1, "form {fi} of value {vi} was not detected");
        }
    }
    let mut jwt = w.load.clone();
    jwt.extend_from_slice(
        format!(
            "note: eyJ{}.eyJ{}.{}\n",
            "a".repeat(20),
            "b".repeat(20),
            "c".repeat(20)
        )
        .as_bytes(),
    );
    assert_eq!(count_jwt_like(&jwt), 1);
    // 前に文字が付いた形（`x.`・`name_`・語の途中）も数える。
    let body = format!(
        "eyJ{}.eyJ{}.{}",
        "a".repeat(20),
        "b".repeat(20),
        "c".repeat(20)
    );
    for prefix in ["x.", "name_", "Zm9v", "k="] {
        let mut m = w.load.clone();
        m.extend_from_slice(format!("note: {prefix}{body}\n").as_bytes());
        assert_eq!(count_jwt_like(&m), 1, "prefix {prefix}");
    }
    assert_eq!(count_jwt_like(b"eyJabc.def.ghi"), 0);
}

/// 登録簿から取った 16 進の文字列（固定語彙の外。文字種の規則には合う）は、値の名前・出典・区間名として記録に出せない
/// （SECR-23・#6038。語彙は crate の固定の定数で、呼び出し元は足せない）。設定由来の値を一律に拒否するわけではなく、
/// 固定語彙に一致する文字列（`p99_target` 等）は由来を問わず通る。この試験は、語彙外の 16 進の文字列を名前・出典に渡すと拒否され、
/// 区間名にした対象は 1 回も呼ばれない（実行の前の拒否と呼び出し 0 件。記録のファイルの未作成までは確かめない）。
/// 以前はここで「語彙にも入れた 16 進の値が書き出される」ことを陽性対照にしていたが、その経路を塞いだため
/// 拒否の試験に置き換えた。照合の検出力は陽性対照 1（`matcher_detects_every_form_mixed_into_output`）で見る。
#[test]
fn registry_hex_strings_outside_vocabulary_are_rejected_as_names() {
    let (registry, _cfg) = canary_registry_and_config();
    let candidates: Vec<String> = registry.iter().take(3).map(|raw| hex(raw, false)).collect();
    for c in &candidates {
        assert_eq!(ValueName::new(c), Err(LoadRecordError::InvalidName));
        assert_eq!(RefId::new(c), Err(LoadRecordError::InvalidRef));
    }
    // 区間名に使うと、実行の前に拒否される（対象を呼ばない）。
    let conditions = MeasurementConditions::detect(
        ComponentConditions::new(&[(ConditionComponent::HubStub, true)]).unwrap(),
        LoopModel::Closed,
    )
    .unwrap();
    let calls = std::sync::atomic::AtomicU64::new(0);
    for c in &candidates {
        let spec = TargetSpec::new(
            TargetName::new("hub_stub_jwks").unwrap(),
            IntervalName::new(c).unwrap(),
        );
        let plan = StagePlan::new(
            vec![StageConfig::new(LoadConfig::new(1, 5, 1).unwrap(), 1).unwrap()],
            MetricKey::P99Ns,
            StageLimits::new(8, 8, 1_000, 10_000).unwrap(),
        )
        .unwrap();
        let session = RunSession::new(
            RunRecordFile::new("it-run-0-1-1.txt").unwrap(),
            0,
            conditions.clone(),
        )
        .unwrap();
        let r = session.run(&[ConditionComponent::HubStub], &spec, &plan, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            CallResult::Ok
        });
        assert_eq!(
            r.err(),
            Some(LoadRecordError::Stage(StageError::InvalidPlan))
        );
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
