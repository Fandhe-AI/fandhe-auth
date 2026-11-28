//! TASK-13 (8/8): 障害注入のハーネスの試し実行。8 種類の注入を基準線・注入ありの順に 1 回ずつ通し、
//! 2 つの記録（`run_mode` が baseline と fault）を書き出して中身を確かめる。Docker・外部ネットワークなし
//! （上流は 127.0.0.1 の echo。`tests/support/upstream.rs`）。この試験が使い方の見本（`docs/harness/fault-injection.md`）。
//!
//! - 主試験 `small_fault_trial_writes_baseline_and_fault_records` は `make test` で流れ、記録の本文を出さない。
//! - 本文を出す `print_checked_fault_record_bodies` は `#[ignore]`。`make fault-trial`（`scripts/fault-trial.sh`）が
//!   `--ignored --exact` で選んだときだけ流れ、行ごとの許可リストを通った本文だけを固定の開始行・終了行で挟んで出す
//!   （通らなければ出さず失敗する。fail-closed。SECR-11・SECR-23・SECR-26）。
//! - 遅れ・ずれ・状態コードは試験の中だけの仮の値で、要件の値ではない（THRT-29・A-2026-10-28-1）。
//! - 記録は事実の出力で、合否の判定でも spec の「確定」でもない。stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）。
//! - Docker の有無・OS で実行時に飛ばす分岐は作らない。Unix 以外ではインスタンスの停止の 2 段が `cfg` で消える（DPOL-30〜32）。

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_hub_stub::jwks::HubJwksEndpoint;
use fandhe_auth_it_harness::fault::{
    ClockSkewInjector, DbFaultProxy, ExternalStoreFaultProxy, FAULT_RECORD_FORMAT, FaultCategory,
    FaultKind, FaultPair, FaultPhase, FaultRecord, FaultStep, FaultTarget, FaultTimeline,
    HubStubFaultInjector, HubStubPort, HubStubRefusalKind, PairedRecords, RunMode,
};
use fandhe_auth_it_harness::record::UtcTimestamp;

#[path = "support/upstream.rs"]
mod upstream;
use upstream::echo_upstream;

/// 本文の区画の開始行・終了行。`scripts/fault-trial.sh` の同名の文字列と一致させる。
const PRINT_BEGIN: &str = "-----BEGIN FAULT RECORD BODIES-----";
const PRINT_END: &str = "-----END FAULT RECORD BODIES-----";

/// hub-stub の呼び出しに渡す論理時刻（試験の中だけの仮の値）。
const NOW: u64 = 1_000;

/// 記録の `note:` 行として許す固定の文（`record.rs` の `Display` が出すもの。変わったら試験が落ちる）。
const ALLOWED_NOTES: [&str; 2] = [
    "記録は事実の出力で、合否の判定でも spec の確定でもない（ACPT-7）",
    "stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）",
];

/// 設定の目印の文字列。本文に現れてはならない（照合の陰性側。SECR-23）。
const CONFIG_MARKS: [&str; 4] = [
    "trial-client.test",
    "trial-hub.example.test",
    "https://trial-hub.example.test/jwks",
    "https://trial-hub.example.test/logout",
];

fn hub_config() -> HubStubConfig {
    HubStubConfig {
        client_id: StubValue::test("trial-client.test".to_owned()),
        public_host: StubValue::test("trial-hub.example.test".to_owned()),
        jwks_uri: StubValue::test("https://trial-hub.example.test/jwks".to_owned()),
        logout_receiver_uri: StubValue::test("https://trial-hub.example.test/logout".to_owned()),
        jwks_max_age: StubValue::test(Duration::from_secs(1)),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(300)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
    }
}

fn step(kind: FaultKind, phase: FaultPhase) -> FaultStep {
    FaultStep { kind, phase }
}

/// 試験の中だけの仮の値の障害。
fn db_delay() -> FaultKind {
    FaultKind::db_delay(Duration::from_millis(5)).unwrap()
}

fn jwks_delay() -> FaultKind {
    FaultKind::hub_stub_delay(HubStubPort::Jwks, Duration::from_millis(5)).unwrap()
}

fn heartbeat_refusal() -> FaultKind {
    FaultKind::hub_stub_refusal(HubStubPort::Heartbeat, HubStubRefusalKind::Status(503)).unwrap()
}

fn clock_skew() -> FaultKind {
    FaultKind::clock_skew(1_500).unwrap()
}

/// 試験対象の往復の代わり。プロキシ越しに 1 往復して、そのまま返ったか（遮断の効きを確かめる）。
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

/// 本体の誤り。値・パス・ポートを載せない固定の型にする（SECR-23）。`Debug` は意図して実装しない
/// （実装すると下の `From` が自分自身と重なる）。
struct Fail;

impl<E: std::fmt::Debug> From<E> for Fail {
    fn from(_: E) -> Self {
        Self
    }
}

/// 本体の外へ出す観測（実行ごとに 1 件）。記録の本文とは別で、本文には入らない。
#[derive(Debug, Default)]
struct Observation {
    ports: Vec<String>,
}

/// 1 つのシナリオ。モードを見ない。注入は `inject` に宣言した段だけを通して行う（見本）。
/// `InstanceFaultHandle::sigterm` が使う `/bin/kill` の有無（`fault_instance.rs` と同じ確認）。
fn sigterm_available() -> bool {
    cfg!(unix) && Path::new("/bin/kill").exists()
}

fn scenario(
    tl: FaultTimeline,
    upstream_port: u16,
    observed: &mut Vec<Observation>,
) -> Result<FaultRecord, Fail> {
    let mut tl = tl;
    let mut obs = Observation::default();
    let mut db = DbFaultProxy::start_to_port(upstream_port)?;
    let mut store = ExternalStoreFaultProxy::start_to_port(upstream_port)?;
    obs.ports.push(db.listen_port().to_string());
    obs.ports.push(store.listen_port().to_string());
    let mut hub_clock = ClockSkewInjector::new();
    let waits: Rc<RefCell<usize>> = Rc::default();
    let w2 = Rc::clone(&waits);
    let mut hub = HubStubFaultInjector::new(move |_d: Duration| *w2.borrow_mut() += 1, &tl)?;

    // DB の遮断（解除まで）と遅れ。
    tl.inject(&[step(FaultKind::DbBlock, FaultPhase::Applied)], |tl| {
        db.block(tl)
    })?;
    let _ = echoes(db.listen_port());
    tl.inject(&[step(FaultKind::DbBlock, FaultPhase::Released)], |tl| {
        db.release_block(tl)
    })?;
    tl.inject(&[step(db_delay(), FaultPhase::Applied)], |tl| {
        db.delay(tl, Duration::from_millis(5))
    })?;
    tl.inject(&[step(db_delay(), FaultPhase::Released)], |tl| {
        db.release_delay(tl)
    })?;

    // DB の外の保存先の遮断。
    tl.inject(
        &[step(FaultKind::ExternalStoreBlock, FaultPhase::Applied)],
        |tl| store.block(tl),
    )?;
    tl.inject(
        &[step(FaultKind::ExternalStoreBlock, FaultPhase::Released)],
        |tl| store.release_block(tl),
    )?;

    // Hub の stub の遅れ（JWKS の口。1 回呼んで効きを残す）と拒否（別の口。同じ口に重ねない）。
    let delay = jwks_delay();
    tl.inject(&[step(delay, FaultPhase::Applied)], |tl| {
        hub.apply(tl, delay)
    })?;
    let mut jwks = hub.bind_receiver(HubJwksEndpoint::new(&hub_config()));
    hub.serve_jwks(&tl, &mut jwks, NOW)?;
    tl.inject(&[step(delay, FaultPhase::Released)], |tl| {
        hub.release(tl, delay)
    })?;
    let refusal = heartbeat_refusal();
    tl.inject(&[step(refusal, FaultPhase::Applied)], |tl| {
        hub.apply(tl, refusal)
    })?;
    tl.inject(&[step(refusal, FaultPhase::Released)], |tl| {
        hub.release(tl, refusal)
    })?;

    // 時計のずれ。終了は時計も結んだ `finish_with_clock`。
    let skew = clock_skew();
    tl.inject(&[step(skew, FaultPhase::Applied)], |tl| {
        hub_clock.apply(tl, 1_500)
    })?;
    tl.inject(&[step(skew, FaultPhase::Released)], |tl| {
        hub_clock.release(tl)
    })?;

    // インスタンスの停止（Unix だけ。`cfg` のコンパイル時の区別）。基準線では signal を送らず、後始末の `shutdown` だけで止まる。
    #[cfg(unix)]
    {
        use fandhe_auth_it_harness::fault::InstanceFaultHandle;
        use std::process::Command;
        let mut kill = InstanceFaultHandle::spawn(Command::new("sleep").arg("30"))?;
        // SIGTERM は固定パスの /bin/kill で送る。無い環境（最小コンテナ等）ではこの段だけ飛ばす。
        let term = if sigterm_available() {
            let mut term = InstanceFaultHandle::spawn(Command::new("sleep").arg("30"))?;
            tl.inject(
                &[step(FaultKind::InstanceSigterm, FaultPhase::Applied)],
                |tl| term.sigterm(tl),
            )?;
            Some(term)
        } else {
            None
        };
        tl.inject(
            &[step(FaultKind::InstanceSigkill, FaultPhase::Applied)],
            |tl| kill.sigkill(tl),
        )?;
        if let Some(term) = term {
            term.shutdown();
        }
        kill.shutdown();
    }

    let (record, _log) = hub.finish_with_clock(
        &mut hub_clock,
        tl,
        UtcTimestamp::from_system_time(SystemTime::now()).map_err(|_| Fail)?,
    )?;
    db.stop();
    store.stop();
    observed.push(obs);
    Ok(record)
}

/// 試し実行の結果。本文・組・観測。
struct Trial {
    pair: PairedRecords,
    baseline_body: String,
    fault_body: String,
    forbidden_ports: Vec<String>,
    dir: PathBuf,
    files: (PathBuf, PathBuf),
}

/// 基準線 → 注入ありの順に流し、2 つの記録を書き、読み戻して `to_string` と一致することを確かめる。
fn run_trial() -> Trial {
    let up = echo_upstream();
    let mut observed = Vec::new();
    let pair = FaultPair::run(|tl| scenario(tl, up.port(), &mut observed)).expect("trial pair");
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fault-trial");
    let files = pair.write_new(&dir).expect("write records");
    let baseline_body = std::fs::read_to_string(&files.0).expect("read baseline");
    let fault_body = std::fs::read_to_string(&files.1).expect("read fault");
    assert_eq!(baseline_body, pair.baseline().to_string());
    assert_eq!(fault_body, pair.fault().to_string());
    let mut forbidden_ports: Vec<String> = observed.into_iter().flat_map(|o| o.ports).collect();
    forbidden_ports.push(up.port().to_string());
    Trial {
        pair,
        baseline_body,
        fault_body,
        forbidden_ports,
        dir,
        files,
    }
}

impl Trial {
    /// 成功時だけ消す（失敗時は調べられるよう残す。置き場は追跡ツリーの外）。
    fn remove_files(self) {
        let _ = std::fs::remove_file(&self.files.0);
        let _ = std::fs::remove_file(&self.files.1);
    }
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_rfc3339_utc(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
}

fn is_signed_digits(s: &str) -> bool {
    s.strip_prefix(['+', '-']).is_some_and(is_digits)
}

/// `injection:` 行の 1 行を、トークンごとの許可リストで照合する。
fn check_injection_line(rest: &str) -> Result<(), String> {
    let t: Vec<&str> = rest.split(' ').collect();
    if t.len() < 6 {
        return Err("injection line too short".to_owned());
    }
    let ok_head = is_digits(t[0])
        && matches!(t[1], "applied" | "released")
        && FaultCategory::CATALOG.iter().any(|c| c.name() == t[2])
        && t[3].strip_prefix("target=").is_some_and(|v| {
            [
                FaultTarget::Database,
                FaultTarget::Instance,
                FaultTarget::ExternalStore,
                FaultTarget::HubStub,
            ]
            .iter()
            .any(|x| x.name() == v)
        })
        && t[4].strip_prefix("at=").is_some_and(is_rfc3339_utc)
        && t[5].strip_prefix("offset_ms=").is_some_and(is_digits);
    if !ok_head {
        return Err("injection line head is not allowed".to_owned());
    }
    for tok in &t[6..] {
        let ok = tok.strip_prefix("delay_ms=").is_some_and(is_digits)
            || tok.strip_prefix("skew_ms=").is_some_and(is_signed_digits)
            || tok
                .strip_prefix("port=")
                .is_some_and(|v| matches!(v, "presentation" | "logout" | "heartbeat" | "jwks"))
            || tok.strip_prefix("refusal=").is_some_and(|v| {
                matches!(v, "unreachable" | "malformed_body")
                    || v.strip_prefix("status_").is_some_and(is_digits)
            })
            || tok.strip_prefix("log_pos=").is_some_and(is_digits);
        if !ok {
            return Err("injection line has a token that is not allowed".to_owned());
        }
    }
    Ok(())
}

/// 本文を出してよいかを行ごとの許可リストで照合する（fail-closed）。通った本文だけを返す。
/// 値の内容は一致で確かめず、許す形だけを通す。誤りの文言に本文の一部を載せない。
fn printable_body<'a>(text: &'a str, forbidden_ports: &[String]) -> Result<&'a str, String> {
    if !text.ends_with('\n') {
        return Err("body does not end with a newline".to_owned());
    }
    for line in text.lines() {
        let Some((key, value)) = line.split_once(": ") else {
            return Err("line without a key".to_owned());
        };
        match key {
            "format" if value == FAULT_RECORD_FORMAT => {}
            "run_mode" if matches!(value, "baseline" | "fault") => {}
            "run_started_at" | "recorded_at" if is_rfc3339_utc(value) => {}
            "injection_count" | "unreleased_count" if is_digits(value) => {}
            "injection" => check_injection_line(value)?,
            "note" if ALLOWED_NOTES.contains(&value) => {}
            _ => return Err("line is not allowed".to_owned()),
        }
    }
    // 陰性の照合。待ち受けポート・上流のポート（5 桁以上。試験中の経過の ms と取り違えない）を数値として含まない。
    for tok in text.split([' ', '\n', '=']) {
        if tok.len() >= 5 && forbidden_ports.iter().any(|p| p == tok) {
            return Err("body contains a port".to_owned());
        }
    }
    if CONFIG_MARKS.iter().any(|m| text.contains(m)) {
        return Err("body contains a configuration mark".to_owned());
    }
    Ok(text)
}

fn count_lines(body: &str, prefix: &str) -> usize {
    body.lines().filter(|l| l.starts_with(prefix)).count()
}

/// Unix は 14 件。`/bin/kill` が無ければ SIGTERM の 1 件を除いた 13 件。Unix 以外は 12 件。
fn expected_events() -> usize {
    if !cfg!(unix) {
        12
    } else if sigterm_available() {
        14
    } else {
        13
    }
}

/// 主試験。`make test` で流れる。本文は出さない（環境変数・`--nocapture` に依らない）。
#[test]
fn small_fault_trial_writes_baseline_and_fault_records() {
    let trial = run_trial();
    let expected_events = expected_events();
    let (b, f) = (trial.pair.baseline(), trial.pair.fault());
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    let (nb, nf) = (name(&trial.files.0), name(&trial.files.1));
    assert!(nb.starts_with("it-fault-baseline-"));
    assert!(nf.starts_with("it-fault-fault-"));
    assert_ne!(nb, nf);
    assert!(trial.dir.ends_with("fault-trial"));

    // 基準線: 事象 0 件・未解除 0 件。
    assert_eq!((b.mode(), f.mode()), (RunMode::Baseline, RunMode::Fault));
    assert!(trial.baseline_body.contains("run_mode: baseline\n"));
    assert!(trial.baseline_body.contains("injection_count: 0\n"));
    assert!(trial.baseline_body.contains("unreleased_count: 0\n"));
    assert!(b.events().is_empty());
    assert_eq!(b.unreleased_count(), 0);

    // 注入あり: 件数が期待どおり・未解除 0 件・8 種（Unix 以外は 6 種）の名前が 1 回以上出る。
    assert!(trial.fault_body.contains("run_mode: fault\n"));
    assert_eq!(f.events().len(), expected_events);
    assert!(
        trial
            .fault_body
            .contains(&format!("injection_count: {expected_events}\n"))
    );
    assert_eq!(f.unreleased_count(), 0);
    assert!(trial.fault_body.contains("unreleased_count: 0\n"));
    for c in FaultCategory::CATALOG {
        let expected = match c {
            FaultCategory::InstanceSigterm => sigterm_available(),
            FaultCategory::InstanceSigkill => cfg!(unix),
            _ => true,
        };
        let seen = trial
            .fault_body
            .lines()
            .any(|l| l.starts_with("injection: ") && l.split(' ').nth(3) == Some(c.name()));
        assert_eq!(seen, expected, "category {}", c.name());
    }

    // 段の並びは一致し、形式の行は 1 行ずつ。
    assert_eq!(b.injection_points(), f.injection_points());
    for body in [&trial.baseline_body, &trial.fault_body] {
        assert_eq!(count_lines(body, "format: "), 1);
        assert!(body.contains(&format!("format: {FAULT_RECORD_FORMAT}\n")));
    }

    // 本文の照合を通る（出す試験と同じ関数）。
    printable_body(&trial.baseline_body, &trial.forbidden_ports).expect("baseline body");
    printable_body(&trial.fault_body, &trial.forbidden_ports).expect("fault body");
    let dir = trial.dir.to_string_lossy().into_owned();
    assert!(!trial.baseline_body.contains(&dir) && !trial.fault_body.contains(&dir));
    trial.remove_files();
}

/// 照合の陰性: 許可リストに無い行・トークン・ポートを含む本文は出さない。
#[test]
fn printable_body_rejects_unexpected_content() {
    let ports = vec!["54321".to_owned()];
    let good = format!(
        "format: {FAULT_RECORD_FORMAT}\nrun_mode: baseline\nrun_started_at: 2026-01-01T00:00:00Z\nrecorded_at: 2026-01-01T00:00:01Z\ninjection_count: 0\nunreleased_count: 0\n"
    );
    assert!(printable_body(&good, &ports).is_ok());
    for bad in [
        format!("{good}extra: 1\n"),
        format!("{good}note: free text\n"),
        format!(
            "{good}injection: 1 applied db_block target=database at=2026-01-01T00:00:00Z offset_ms=1 host=x\n"
        ),
        format!(
            "{good}injection: 1 applied db_block target=database at=2026-01-01T00:00:00Z offset_ms=54321\n"
        ),
        format!(
            "{good}injection: 1 applied unknown target=database at=2026-01-01T00:00:00Z offset_ms=1\n"
        ),
        format!("{good}injection: 1 applied db_block target=database at=today offset_ms=1\n"),
        format!("{good}no key line\n"),
        good.trim_end().to_owned(),
    ] {
        assert!(printable_body(&bad, &ports).is_err());
    }
}

/// 本文を出す試験。照合を通った本文だけを、固定の開始行・終了行で挟んで 1 区画で出す（基準線、注入ありの順）。
/// `make fault-trial` が `--ignored --exact` で選ぶ。通常は流れない。
#[test]
#[ignore = "prints the checked record bodies; run via make fault-trial"]
fn print_checked_fault_record_bodies() {
    let trial = run_trial();
    let b = printable_body(&trial.baseline_body, &trial.forbidden_ports)
        .expect("baseline body")
        .to_owned();
    let f = printable_body(&trial.fault_body, &trial.forbidden_ports)
        .expect("fault body")
        .to_owned();
    trial.remove_files();
    println!("{PRINT_BEGIN}\n{b}{f}{PRINT_END}");
}
