//! 記録のファイル全体の単体試験（TASK-12 9/10）。偽の値・合成の時刻（epoch 付近）だけを使い、
//! PoC の数値・要件の値を置かない（ACPT-7）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use fandhe_auth_load_harness::{
    ApprovedValues, CallResult, ComparisonValue, ComponentConditions, ConditionComponent,
    ConditionError, IntervalName, InvalidValueReason, LOAD_RECORD_FORMAT, LoadConfig, LoadRecord,
    LoadRecordError, LoopModel, MeasurementConditions, MetricKey, RefId, RunEvidence,
    RunRecordFile, RunSession, StageConfig, StageError, StageLimits, StagePlan, StdMonotonicClock,
    TargetName, TargetSpec, ValueClass, ValueName, ValueUnit,
};

/// 排他的に新規作成したディレクトリ。既存のもの（中断した試験の残り・PID の再利用で重なる名前）は
/// 消さず、自分が作ったものだけを Drop で片付ける。
struct TestDir(PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl TestDir {
    fn new(name: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
        std::fs::create_dir_all(base).unwrap();
        for _ in 0..100 {
            let d = base.join(format!(
                "{name}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            // create_dir は既存なら失敗する。別の名前で作り直し、既存を消さない。
            match std::fs::create_dir(&d) {
                Ok(()) => return Self(d),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("create test dir: {e}"),
            }
        }
        panic!("no free test dir name");
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// 試験用の語彙。入れた名前だけが通る。
fn vname(n: &str) -> ValueName {
    ValueName::new(n, &[n]).unwrap()
}

fn refs(ids: &[&str]) -> Vec<RefId> {
    ids.iter().map(|s| RefId::new(s, ids).unwrap()).collect()
}

fn val(
    name: &str,
    class: ValueClass,
    v: Option<u64>,
    ids: &[&str],
) -> Result<ComparisonValue, LoadRecordError> {
    // 承認済みの一覧は候補とは独立した固定の本文から読む（候補の出典から作らない。ACPT-7）。
    // 候補が承認済みの一覧と一致する場合の試験は approval_* で確かめる。ここは区分の規則と出典の上限を見る。
    let approved = list("other_item|initial|9|count|APPROVED-1\n");
    ComparisonValue::new(vname(name), class, v, ValueUnit::Ns, refs(ids), &approved)
}

fn approval(
    class: ValueClass,
    v: u64,
    ids: &[&str],
    approved: &ApprovedValues,
) -> Result<ComparisonValue, LoadRecordError> {
    ComparisonValue::new(
        vname("v"),
        class,
        Some(v),
        ValueUnit::Ns,
        refs(ids),
        approved,
    )
}

/// 承認一覧の書式の試験用に、固定の本文を一時ファイルへ書いて `load` で読む。本文は試験の固定の文言で、
/// 候補から組み立てない。ここで書くファイルは書式の検査の入力で、人の承認ではない（ACPT-7）。
/// ファイルは排他的に作ったディレクトリの中に `create_new` で作り、ディレクトリごと Drop で消す
/// （他の試験のファイルを切り詰め・削除しない）。
fn try_list(text: &str) -> Result<ApprovedValues, LoadRecordError> {
    use std::io::Write as _;
    let dir = TestDir::new("approved");
    let path = dir.path().join("approved.txt");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    f.write_all(text.as_bytes()).unwrap();
    drop(f);
    ApprovedValues::load(&path)
}

fn list(text: &str) -> ApprovedValues {
    try_list(text).unwrap()
}

/// ACPT-7・load.md 規則 4: 承認済みの一覧と一致しない値は、無関係な承認済み ID を添えても出せない。
#[test]
fn approval_must_match_name_class_value_unit_refs() {
    use ValueClass::*;
    let ok = list("v|initial|5|ns|A-1\n");
    assert!(approval(Initial, 5, &["A-1"], &ok).is_ok());
    let na = LoadRecordError::InvalidValue(InvalidValueReason::NotInApprovedList);
    assert_eq!(
        approval(Initial, 5, &["A-1"], &ApprovedValues::default()).unwrap_err(),
        na
    );
    // 数値・区分・出典・単位・項目のどれか 1 つが違っても出せない。
    assert_eq!(approval(Initial, 6, &["A-1"], &ok).err(), Some(na));
    assert_eq!(approval(Requirement, 5, &["A-1"], &ok).err(), Some(na));
    assert_eq!(approval(Initial, 5, &["B-2"], &ok).err(), Some(na));
    assert_eq!(approval(Initial, 5, &["A-1", "B-2"], &ok).err(), Some(na));
    let other_unit = list("v|initial|5|count|A-1\n");
    assert_eq!(approval(Initial, 5, &["A-1"], &other_unit).err(), Some(na));
    let other_name = list("w|initial|5|ns|A-1\n");
    assert_eq!(approval(Initial, 5, &["A-1"], &other_name).err(), Some(na));
}

/// 承認済みの一覧は本文からだけ読む。PoC の区分・不正な行は受けない（規則 5）。
#[test]
fn approved_list_parse_is_strict() {
    assert!(try_list("# c\n\nv|initial|5|ns|A-1,B-2\n").is_ok());
    for bad in [
        "v|poc_parameter|5|ns|A-1",
        "v|initial|x|ns|A-1",
        "v|initial|5|ms|A-1",
        "v|initial|5|ns|",
        "v|initial|5|ns|A-1,A-1",
        "v|initial|5|ns",
        "V|initial|5|ns|A-1",
        // RefId::new と同じ検査（予約語・空白・64 文字超）。
        "v|initial|5|ns|none",
        "v|initial|5|ns|measured",
        "v|initial|5|ns|A 1",
        &format!("v|initial|5|ns|{}", "a".repeat(65)),
    ] {
        assert_eq!(try_list(bad).err(), Some(LoadRecordError::Format), "{bad}");
    }
}

/// 読み込み専用の口: 存在しない・ディレクトリ・大きすぎるものは `ApprovedFile`。
#[test]
fn approved_load_rejects_non_files_and_oversize() {
    let td = TestDir::new("approved-nonfile");
    let missing = td.path().join("missing.txt");
    assert_eq!(
        ApprovedValues::load(&missing).err(),
        Some(LoadRecordError::ApprovedFile)
    );
    assert_eq!(
        ApprovedValues::load(td.path()).err(),
        Some(LoadRecordError::ApprovedFile)
    );
    let big = "#".repeat(70_000);
    assert_eq!(try_list(&big).err(), Some(LoadRecordError::ApprovedFile));
}

/// 書き手のいない FIFO を指しても `open` で止まらず、開いたハンドルの種別の検査で `ApprovedFile` になる。
/// 止まったら（回帰）待ちを打ち切って失敗させ、書き手側を開いて残ったスレッドをほどく。
#[cfg(unix)]
#[test]
fn approved_load_does_not_block_on_fifo() {
    use std::sync::mpsc;
    use std::time::Duration;
    let td = TestDir::new("approved-fifo");
    let fifo = td.path().join("approved.fifo");
    // mkfifo が無い・失敗した環境では黙って通さず、試験を失敗させる。
    let st = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("run mkfifo");
    assert!(st.success(), "mkfifo failed");
    let (tx, rx) = mpsc::channel();
    let p = fifo.clone();
    let h = std::thread::spawn(move || {
        let _ = tx.send(ApprovedValues::load(&p));
    });
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(r) => {
            assert_eq!(r.err(), Some(LoadRecordError::ApprovedFile));
            h.join().unwrap();
        }
        Err(_) => {
            // 読み手が open で待っている。書き手として開いて待ちをほどいてから失敗させる。
            let _w = std::fs::OpenOptions::new().write(true).open(&fifo);
            let _ = h.join();
            panic!("ApprovedValues::load blocked on a FIFO");
        }
    }
}

fn invalid(r: Result<ComparisonValue, LoadRecordError>) -> bool {
    matches!(r, Err(LoadRecordError::InvalidValue(_)))
}

#[test]
fn class_rules_fail_closed() {
    use ValueClass::*;
    for c in [Requirement, Initial] {
        assert!(invalid(val("v", c, Some(1), &[])));
        assert!(invalid(val("v", c, None, &["X-1"])));
        // 独立した承認一覧に無いため、区分の規則を通った後に NotInApprovedList になる。
        assert_eq!(
            val("v", c, Some(1), &["X-1"]).unwrap_err(),
            LoadRecordError::InvalidValue(InvalidValueReason::NotInApprovedList)
        );
    }
    // 規則 5: PoC の数値は比べる値として置かない。
    assert_eq!(
        val("v", PocParameter, Some(1), &["X-1"]).unwrap_err(),
        LoadRecordError::InvalidValue(InvalidValueReason::ClassNotAllowed)
    );
    assert!(val("v", Undecided, None, &[]).is_ok());
    assert!(val("v", Undecided, None, &["X-1"]).is_ok());
    assert!(invalid(val("v", Undecided, Some(1), &[])));
    assert!(invalid(val("v", External, None, &[])));
    assert!(invalid(val("v", External, Some(1), &["X-1"])));
    assert!(val("v", External, None, &["X-1"]).is_ok());
    for c in [TestCondition, Measured] {
        assert!(invalid(val("v", c, None, &[])));
        assert!(invalid(val("v", c, Some(1), &["X-1"])));
        assert!(val("v", c, Some(1), &[]).is_ok());
    }
}

#[test]
fn refs_limits() {
    let nine: Vec<String> = (0..9).map(|i| format!("R-{i}")).collect();
    let nine: Vec<&str> = nine.iter().map(String::as_str).collect();
    assert_eq!(
        val("v", ValueClass::Initial, Some(1), &nine).unwrap_err(),
        LoadRecordError::TooManyRefs
    );
    assert_eq!(
        val("v", ValueClass::Initial, Some(1), &nine[..8]).unwrap_err(),
        LoadRecordError::InvalidValue(InvalidValueReason::NotInApprovedList)
    );
    assert_eq!(
        val("v", ValueClass::Initial, Some(1), &["A", "A"]).unwrap_err(),
        LoadRecordError::InvalidRef
    );
}

#[test]
fn input_validation() {
    for bad in ["", "A", "a-b", "a\nb", "a\u{2028}b", "a b", &"a".repeat(65)] {
        assert_eq!(
            ValueName::new(bad, &[bad]).unwrap_err(),
            LoadRecordError::InvalidName
        );
    }
    // 形が正しくても語彙の外は拒否する。
    assert_eq!(
        ValueName::new("free_text", &["p99_target"]).unwrap_err(),
        LoadRecordError::InvalidName
    );
    for reserved in ["none", "harness_config", "measured"] {
        assert_eq!(
            RefId::new(reserved, &[reserved]).unwrap_err(),
            LoadRecordError::InvalidRef
        );
    }
    for bad in [
        "",
        "a\nb",
        "a\u{2028}b",
        "a\u{2029}b",
        "a,b",
        "a b",
        "a\0b",
        &"a".repeat(65),
    ] {
        assert_eq!(
            RefId::new(bad, &[bad]).unwrap_err(),
            LoadRecordError::InvalidRef
        );
    }
    // 信頼済みの一覧に無い出典は、文字種が正しくても拒否する（SECR-23・SECR-26）。
    assert_eq!(
        RefId::new("CONFIG-SECRET-1", &["AUDP-29"]).unwrap_err(),
        LoadRecordError::InvalidRef
    );
    assert!(RefId::new("AUDP-29", &["AUDP-29"]).is_ok());
    assert!(RunRecordFile::new("it-run-0-1-2.txt").is_ok());
    for bad in [
        "",
        "../it-run-0-1-2.txt",
        "it-run-0-1-2.txt\n",
        "x-run-0-1-2.txt",
        "it-run-0-1.txt",
        "it-run-a-1-2.txt",
        "it-run-0-1-2.log",
        "it-run-0-1-2-3.txt",
        "it-run-253402300800-1-2.txt",
        "it-run-99999999999999999999999-1-2.txt",
        "it-run-00-1-2.txt",
        "it-run-0-01-2.txt",
        "it-run-0-1-02.txt",
        "it-run-0-4294967296-2.txt",
        "dir/it-run-0-1-2.txt",
    ] {
        assert_eq!(
            RunRecordFile::new(bad).unwrap_err(),
            LoadRecordError::InvalidRunRecordFile
        );
    }
}

#[test]
fn value_line_shapes() {
    let l = |v: ComparisonValue| v.to_string();
    assert_eq!(
        l(val("p99_target", ValueClass::Undecided, None, &[]).unwrap()),
        "value: p99_target class=undecided value=none unit=ns refs=none"
    );
    assert_eq!(
        l(val("c", ValueClass::TestCondition, Some(3), &[]).unwrap()),
        "value: c class=test_condition value=3 unit=ns refs=harness_config"
    );
    assert_eq!(
        l(val("m", ValueClass::Measured, Some(7), &[]).unwrap()),
        "value: m class=measured value=7 unit=ns refs=measured"
    );
    assert_eq!(
        l(val("e", ValueClass::External, None, &["A", "B"]).unwrap()),
        "value: e class=external value=none unit=ns refs=A,B"
    );
}

fn spec() -> TargetSpec {
    TargetSpec::new(
        TargetName::new("route_a").unwrap(),
        IntervalName::new("op").unwrap(),
    )
}

fn plan_with(interval: &str) -> StagePlan {
    StagePlan::new(
        vec![StageConfig::new(LoadConfig::new(1, 3, 0).unwrap(), 1).unwrap()],
        MetricKey::P99Ns,
        StageLimits::new(4, 4, 100, 100).unwrap(),
        vec![IntervalName::new(interval).unwrap()],
    )
    .unwrap()
}

fn conditions_of(started: &[(ConditionComponent, bool)]) -> MeasurementConditions {
    MeasurementConditions::detect(
        ComponentConditions::new(started).unwrap(),
        LoopModel::Closed,
    )
    .unwrap()
}

fn conditions() -> MeasurementConditions {
    conditions_of(&[(ConditionComponent::HubStub, true)])
}

/// 文脈 `session` で段階を回す。呼び出しの回数を `calls` に数える。
fn run_in(
    session: RunSession,
    exercised: &[ConditionComponent],
    plan: &StagePlan,
    calls: &AtomicU64,
) -> Result<RunEvidence, LoadRecordError> {
    session.run(
        exercised,
        || Box::new(StdMonotonicClock::new()),
        &spec(),
        plan,
        || {
            calls.fetch_add(1, Ordering::Relaxed);
            CallResult::Ok
        },
    )
}

fn evidence(s: u64) -> Result<RunEvidence, LoadRecordError> {
    let calls = AtomicU64::new(0);
    run_in(
        RunSession::new(file_at(s), s, conditions())?,
        &[ConditionComponent::HubStub],
        &plan_with("op"),
        &calls,
    )
}

fn file_at(s: u64) -> RunRecordFile {
    RunRecordFile::new(&format!("it-run-{s}-1-1.txt")).unwrap()
}

fn file() -> RunRecordFile {
    file_at(0)
}

fn build(s: u64, r: u64, values: Vec<ComparisonValue>) -> Result<LoadRecord, LoadRecordError> {
    LoadRecord::new(&evidence(s)?, r, values)
}

#[test]
fn record_layout_order_and_timestamps() {
    let v = val("p99_target", ValueClass::Undecided, None, &["AUDP-29"]).unwrap();
    let rec = build(0, 60, vec![v]).unwrap();
    let text = rec.to_string();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], format!("format: {LOAD_RECORD_FORMAT}"));
    assert_eq!(lines[1], "run_record_file: it-run-0-1-1.txt");
    assert_eq!(lines[2], "run_started_at: 1970-01-01T00:00:00Z");
    assert_eq!(lines[3], "recorded_at: 1970-01-01T00:01:00Z");
    assert!(lines[4].starts_with("condition_machine: cpu_count"));
    let pos = |p: &str| lines.iter().position(|l| l.starts_with(p)).unwrap();
    assert!(pos("condition_loop:") < pos("stage:"));
    assert!(pos("stage:") < pos("run:"));
    assert!(pos("worst_run:") < pos("value:"));
    assert_eq!(
        lines[lines.len() - 2],
        "value: p99_target class=undecided value=none unit=ns refs=AUDP-29"
    );
    assert_eq!(lines[lines.len() - 1], "judgment:");
    assert!(text.ends_with("judgment:\n"));
}

#[test]
fn rfc3339_known_values() {
    // 2000-02-29T12:34:56Z = 951827696（閏年）。2024-12-31T23:59:59Z = 1735689599。
    let t = build(951_827_696, 1_735_689_599, vec![])
        .unwrap()
        .to_string();
    assert!(t.contains("run_started_at: 2000-02-29T12:34:56Z\n"));
    assert!(t.contains("recorded_at: 2024-12-31T23:59:59Z\n"));
}

#[test]
fn run_start_mismatch_and_time_range() {
    let m = RunSession::new(file_at(5), 6, conditions());
    assert_eq!(m.unwrap_err(), LoadRecordError::RunStartMismatch);
    const MAX: u64 = 253_402_300_799;
    let t = build(MAX, MAX, vec![]).unwrap().to_string();
    assert!(t.contains("recorded_at: 9999-12-31T23:59:59Z\n"));
    assert_eq!(
        build(0, MAX + 1, vec![]).unwrap_err(),
        LoadRecordError::TimeOutOfRange
    );
    assert_eq!(
        RunSession::new(file_at(0), MAX + 1, conditions()).unwrap_err(),
        LoadRecordError::TimeOutOfRange
    );
}

/// 実行の整合（A-2026-10-28-9・ACPT-7）: 呼び出す構成要素が空・測定の条件で起動していない組は、対象を
/// 1 回も呼ばずに拒否する。stub を申告した記録は stub の起動と `note:` を必ず出す。
/// 段階の結果を後から渡す口（別の実行の結果・後からの申告）は型に無いため、実行時の否定の試験は置けない。
#[test]
fn run_session_binds_exercised_components_to_conditions() {
    let bad = |c: MeasurementConditions, exercised: &[ConditionComponent]| {
        let calls = AtomicU64::new(0);
        let e = run_in(
            RunSession::new(file_at(0), 0, c).unwrap(),
            exercised,
            &plan_with("op"),
            &calls,
        )
        .unwrap_err();
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "target called before rejection"
        );
        e
    };
    assert_eq!(bad(conditions(), &[]), LoadRecordError::InvalidRunEvidence);
    assert_eq!(
        bad(conditions(), &[ConditionComponent::ConsoleStub]),
        LoadRecordError::InvalidRunEvidence
    );
    assert_eq!(
        bad(
            conditions(),
            &[ConditionComponent::HubStub, ConditionComponent::Host]
        ),
        LoadRecordError::InvalidRunEvidence
    );
    // 実物だけの構成で stub を申告した実行は作れない（note: を欠いた stub の記録を作らない）。
    let real_only = conditions_of(&[(ConditionComponent::Database, false)]);
    assert_eq!(
        bad(real_only, &[ConditionComponent::HubStub]),
        LoadRecordError::InvalidRunEvidence
    );
    // stub の種類を実物として構成に載せる道も無い。
    assert_eq!(
        ComponentConditions::new(&[(ConditionComponent::HubStub, false)]).unwrap_err(),
        ConditionError::StubMismatch
    );
    let t = build(0, 0, vec![]).unwrap().to_string();
    assert!(t.contains("condition_component: HubStub started stub "));
    assert!(t.contains("note: stub results are not counted as real results\n"));
}

/// 段階の実行の誤りは `Stage` で返し、記録の元を作らない（区間名が語彙外なら最初の回の前に拒否）。
#[test]
fn run_session_stage_error_is_reported() {
    let calls = AtomicU64::new(0);
    let e = run_in(
        RunSession::new(file_at(0), 0, conditions()).unwrap(),
        &[ConditionComponent::HubStub],
        &plan_with("other"),
        &calls,
    )
    .unwrap_err();
    assert_eq!(e, LoadRecordError::Stage(StageError::InvalidPlan));
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(e.to_string(), "load record error: stage run failed");
}

#[test]
fn invalid_value_reason_is_fixed_enum() {
    let e = val("v", ValueClass::Initial, Some(1), &[]).unwrap_err();
    assert_eq!(
        e,
        LoadRecordError::InvalidValue(InvalidValueReason::RefsRequired)
    );
    assert_eq!(
        e.to_string(),
        "load record error: invalid value (refs_required)"
    );
}

#[test]
fn clock_order() {
    assert_eq!(
        build(10, 9, vec![]).unwrap_err(),
        LoadRecordError::ClockOrder
    );
    assert!(build(10, 10, vec![]).is_ok());
}

#[test]
fn value_row_limits_and_duplicates() {
    let many: Vec<ComparisonValue> = (0..65)
        .map(|i| val(&format!("v{i}"), ValueClass::Undecided, None, &[]).unwrap())
        .collect();
    assert_eq!(
        build(0, 0, many).unwrap_err(),
        LoadRecordError::TooManyValues
    );
    let dup = vec![
        val("v", ValueClass::Undecided, None, &[]).unwrap(),
        val("v", ValueClass::Measured, Some(1), &[]).unwrap(),
    ];
    assert_eq!(
        build(0, 0, dup).unwrap_err(),
        LoadRecordError::DuplicateValue
    );
}

#[test]
fn debug_and_errors_do_not_leak() {
    let secret = "canary_name_zz";
    let approved = list(&format!("{secret}|initial|424242|ns|CANARY-REF\n"));
    let v = ComparisonValue::new(
        vname(secret),
        ValueClass::Initial,
        Some(424_242),
        ValueUnit::Ns,
        refs(&["CANARY-REF"]),
        &approved,
    )
    .unwrap();
    let dbg = format!("{v:?} {:?} {:?}", vname(secret), refs(&["CANARY-REF"]));
    let rec = build(0, 1, vec![v]).unwrap();
    let dbg = format!("{dbg} {rec:?} {:?}", file());
    for bad in [secret, "CANARY-REF", "424242", "it-run-0-1-1"] {
        assert!(!dbg.contains(bad));
    }
    let e = RunRecordFile::new("bad\nlines").unwrap_err().to_string();
    assert!(!e.contains("bad"));
    let e = RefId::new("x\ny", &["x\ny"]).unwrap_err().to_string();
    assert!(!e.contains('x'));
}

#[test]
fn p99_target_only_undecided() {
    // AUDP-29: 性能の目標は未決。数値と出典を付けて要件の値にできない。
    for c in [
        ValueClass::Requirement,
        ValueClass::Initial,
        ValueClass::PocParameter,
    ] {
        assert_eq!(
            val("p99_target", c, Some(1), &["AUDP-29"]).unwrap_err(),
            LoadRecordError::InvalidValue(InvalidValueReason::ClassNotAllowed)
        );
    }
    assert_eq!(
        val("p99_target", ValueClass::External, None, &["AUDP-29"]).unwrap_err(),
        LoadRecordError::InvalidValue(InvalidValueReason::ClassNotAllowed)
    );
    assert!(val("p99_target", ValueClass::Undecided, None, &["AUDP-29"]).is_ok());
}
