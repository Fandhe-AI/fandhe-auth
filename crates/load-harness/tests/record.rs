//! 記録のファイル全体の単体試験（TASK-12 9/10）。偽の値・合成の時刻（epoch 付近）だけを使い、
//! PoC の数値・要件の値を置かない（ACPT-7）。

use fandhe_auth_load_harness::{
    ApprovedValue, CallResult, ComparisonValue, ComponentConditions, IntervalName,
    InvalidValueReason, LOAD_RECORD_FORMAT, LoadConfig, LoadRecord, LoadRecordError, LoopModel,
    MeasurementConditions, MetricKey, RefId, RunRecordFile, StageConfig, StageLimits, StagePlan,
    StagedRun, StdMonotonicClock, TargetName, TargetSpec, ValueClass, ValueName, ValueUnit,
    run_stages,
};

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
    // 承認済みの組は引数と同じ内容で用意する（一致の検査そのものは approval_* の試験で確かめる）。
    let approved: Vec<ApprovedValue<'_>> = v
        .map(|value| ApprovedValue {
            name,
            class,
            value,
            unit: ValueUnit::Ns,
            refs: ids,
        })
        .into_iter()
        .collect();
    ComparisonValue::new(vname(name), class, v, ValueUnit::Ns, refs(ids), &approved)
}

fn approval(
    class: ValueClass,
    v: u64,
    ids: &[&str],
    approved: &[ApprovedValue<'_>],
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

/// ACPT-7・load.md 規則 4: 承認済みの組と一致しない値は、無関係な承認済み ID を添えても出せない。
#[test]
fn approval_must_match_name_class_value_unit_refs() {
    use ValueClass::*;
    let ok = ApprovedValue {
        name: "v",
        class: Initial,
        value: 5,
        unit: ValueUnit::Ns,
        refs: &["A-1"],
    };
    assert!(approval(Initial, 5, &["A-1"], &[ok]).is_ok());
    let na = LoadRecordError::InvalidValue(InvalidValueReason::NotApproved);
    assert_eq!(approval(Initial, 5, &["A-1"], &[]).unwrap_err(), na);
    // 数値・区分・出典・単位・項目のどれか 1 つが違っても出せない。
    assert_eq!(approval(Initial, 6, &["A-1"], &[ok]).err(), Some(na));
    assert_eq!(approval(Requirement, 5, &["A-1"], &[ok]).err(), Some(na));
    assert_eq!(approval(Initial, 5, &["B-2"], &[ok]).err(), Some(na));
    assert_eq!(approval(Initial, 5, &["A-1", "B-2"], &[ok]).err(), Some(na));
    let other_unit = ApprovedValue {
        unit: ValueUnit::Count,
        ..ok
    };
    assert_eq!(
        approval(Initial, 5, &["A-1"], &[other_unit]).err(),
        Some(na)
    );
    let other_name = ApprovedValue { name: "w", ..ok };
    assert_eq!(
        approval(Initial, 5, &["A-1"], &[other_name]).err(),
        Some(na)
    );
}

fn invalid(r: Result<ComparisonValue, LoadRecordError>) -> bool {
    matches!(r, Err(LoadRecordError::InvalidValue(_)))
}

#[test]
fn class_rules_fail_closed() {
    use ValueClass::*;
    for c in [Requirement, Initial, PocParameter] {
        assert!(invalid(val("v", c, Some(1), &[])));
        assert!(invalid(val("v", c, None, &["X-1"])));
        assert!(val("v", c, Some(1), &["X-1"]).is_ok());
    }
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
    assert!(val("v", ValueClass::Initial, Some(1), &nine[..8]).is_ok());
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

fn staged() -> StagedRun {
    let spec = TargetSpec::new(
        TargetName::new("route_a").unwrap(),
        IntervalName::new("op").unwrap(),
    );
    let plan = StagePlan::new(
        vec![StageConfig::new(LoadConfig::new(1, 3, 0).unwrap(), 1).unwrap()],
        MetricKey::P99Ns,
        StageLimits::new(4, 4, 100, 100).unwrap(),
        vec![IntervalName::new("op").unwrap()],
    )
    .unwrap();
    run_stages(
        || Box::new(StdMonotonicClock::new()),
        &spec,
        &plan,
        || CallResult::Ok,
    )
    .unwrap()
}

fn conditions() -> MeasurementConditions {
    MeasurementConditions::detect(ComponentConditions::new(&[]).unwrap(), LoopModel::Closed)
        .unwrap()
}

fn file_at(s: u64) -> RunRecordFile {
    RunRecordFile::new(&format!("it-run-{s}-1-1.txt")).unwrap()
}

fn file() -> RunRecordFile {
    file_at(0)
}

fn build(s: u64, r: u64, values: Vec<ComparisonValue>) -> Result<LoadRecord, LoadRecordError> {
    LoadRecord::new(&file_at(s), s, r, &conditions(), &staged(), values)
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
    let m = LoadRecord::new(&file_at(5), 6, 6, &conditions(), &staged(), vec![]);
    assert_eq!(m.unwrap_err(), LoadRecordError::RunStartMismatch);
    const MAX: u64 = 253_402_300_799;
    let t = build(MAX, MAX, vec![]).unwrap().to_string();
    assert!(t.contains("recorded_at: 9999-12-31T23:59:59Z\n"));
    assert_eq!(
        build(0, MAX + 1, vec![]).unwrap_err(),
        LoadRecordError::TimeOutOfRange
    );
    assert_eq!(
        LoadRecord::new(
            &file_at(0),
            MAX + 1,
            MAX + 1,
            &conditions(),
            &staged(),
            vec![]
        )
        .unwrap_err(),
        LoadRecordError::TimeOutOfRange
    );
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
    let v = val(secret, ValueClass::Initial, Some(424_242), &["CANARY-REF"]).unwrap();
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
fn p99_target_only_undecided_or_external() {
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
    assert!(val("p99_target", ValueClass::Undecided, None, &["AUDP-29"]).is_ok());
}
