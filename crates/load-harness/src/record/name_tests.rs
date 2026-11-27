//! 記録のファイル全体の単体試験（TASK-12 9/10）。偽の値・合成の時刻（epoch 付近）だけを使い、
//! PoC の数値・要件の値を置かない（ACPT-7）。
//!
//! crate 内の試験にしてある理由: 値の名前・出典の語彙は crate の固定の定数で、外から任意の名前・出典を作る口は無い（#6038）。
//! 語彙の外の名前・出典（長さの境界・目印・重複等）で記録の検査を見る試験は、非公開のフィールドから直接作る。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    BoundedText, MAX_RECORD_BYTES, REF_IDS, RESERVED_REFS, RefId, VALUE_NAMES, ValueName, compose,
    is_name, is_ref_shape, name_in, ref_in,
};
use crate::{
    ApprovedValues, CallResult, ComparisonValue, ComponentConditions, ConditionComponent,
    ConditionError, IntervalName, InvalidValueReason, LOAD_RECORD_FORMAT, LoadConfig, LoadRecord,
    LoadRecordError, LoopModel, MeasurementConditions, MetricKey, RunEvidence, RunRecordFile,
    RunSession, StageConfig, StageError, StageLimits, StagePlan, TargetName, TargetSpec,
    ValueClass, ValueUnit,
};
use std::cell::Cell;
use std::fmt::{self, Write as _};

/// 排他的に新規作成したディレクトリ。既存のもの（中断した試験の残り・PID の再利用で重なる名前）は
/// 消さず、自分が作ったものだけを Drop で片付ける。
pub(super) struct TestDir(PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl TestDir {
    pub(super) fn new(name: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        // 共有の一時ディレクトリでなく、Cargo の出力先（試験の実行ファイルの隣）に置く。
        // ソースツリーの固定パスにせず、出力先を別の書き込み可能な場所へ変えた構成にも追従する。
        let base = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("test-tmp");
        // 一時ディレクトリが無い環境でも create_dir が親の欠けで失敗しないよう、親を先に作る。
        std::fs::create_dir_all(&base).unwrap();
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

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

/// 固定語彙の外の名前を直接作る（公開の `ValueName::new` は固定語彙だけを通す。記録の検査の試験用）。
fn vname(n: &str) -> ValueName {
    ValueName(n.to_owned())
}

/// 固定語彙の外の出典を直接作る（公開の `RefId::new` は固定語彙だけを通す）。
fn refs(ids: &[&str]) -> Vec<RefId> {
    ids.iter().map(|s| RefId((*s).to_owned())).collect()
}

fn val(
    name: &str,
    class: ValueClass,
    v: Option<u64>,
    ids: &[&str],
) -> Result<ComparisonValue, LoadRecordError> {
    // 承認済みの一覧は候補とは独立した固定の本文から読む（候補の出典から作らない。ACPT-7）。
    // 要件の値・初期値は一覧と一致しても常に拒否される（requirement_and_initial_always_rejected）。ここは区分の規則と出典の上限を見る。
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

/// ACPT-7・load.md 規則 4: 承認一覧の置き場・形・承認の手順が決まるまで（TASK-12・#111）、
/// 要件の値・初期値は一覧に一致する行があっても常に拒否する（fail-closed）。
#[test]
fn requirement_and_initial_always_rejected() {
    use ValueClass::*;
    let ca = LoadRecordError::InvalidValue(InvalidValueReason::ClassNotAllowed);
    let ok = list("v|initial|5|ns|A-1\nv|requirement|5|ns|A-1\n");
    for c in [Requirement, Initial] {
        // 一覧に完全一致する行があっても、空の一覧でも、一致しない値でも同じ。
        assert_eq!(approval(c, 5, &["A-1"], &ok).err(), Some(ca));
        assert_eq!(
            approval(c, 5, &["A-1"], &ApprovedValues::default()).err(),
            Some(ca)
        );
        assert_eq!(approval(c, 6, &["B-2"], &ok).err(), Some(ca));
        // 出典なし・値なしでも、出典の規則より先に拒否する。
        assert_eq!(approval(c, 5, &[], &ok).err(), Some(ca));
        assert_eq!(
            ComparisonValue::new(vname("v"), c, None, ValueUnit::Ns, refs(&["A-1"]), &ok).err(),
            Some(ca)
        );
    }
    // 出典の上限・重複の検査より先に拒否する（本来は TooManyRefs・InvalidRef になる入力）。
    let nine: Vec<String> = (0..9).map(|i| format!("R-{i}")).collect();
    let nine: Vec<&str> = nine.iter().map(String::as_str).collect();
    for c in [Requirement, Initial] {
        assert_eq!(approval(c, 5, &nine, &ok).err(), Some(ca));
        assert_eq!(approval(c, 5, &["A", "A"], &ok).err(), Some(ca));
    }
    // 測定値・試験の条件・未決・他所有者の値は、これまでどおり通る。
    assert!(approval(Measured, 5, &[], &ok).is_ok());
    assert!(approval(TestCondition, 5, &[], &ok).is_ok());
    assert!(val("v", Undecided, None, &[]).is_ok());
    assert!(val("v", External, None, &["X-1"]).is_ok());
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
/// 止まったら（回帰）待ちを打ち切って失敗させる。残ったスレッドをほどくのは Linux だけ（後処理の open が
/// 止まらないと fifo(7) で確かめられる対象に限る）。他の対象ではほどかずに失敗させる。
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
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // 読み手のスレッドが結果を送らずに終わった（panic）。待たずにその理由で失敗させる。
            h.join().expect("reader thread panicked");
            panic!("reader thread ended without a result");
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // 読み手が open で待っている。FIFO を読み書きで開く挙動は POSIX では未定義で、止まらないと
            // 保証されるのは Linux だけ（fifo(7)）。Linux では読み書きで開いて待ちをほどき、書き手を閉じる。
            // 他の対象では FIFO を開かない（後処理で試験のスレッド自身が止まらないため）。
            // どちらでも読み手のスレッドは join せず（FIFO の読みで待ち得るため）、そのまま失敗させる。
            if cfg!(target_os = "linux") {
                let w = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&fifo);
                drop(w);
            }
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
        // 承認の手順が決まるまで常に ClassNotAllowed（TASK-12・#111）。
        for (v, ids) in [
            (Some(1), &[][..]),
            (None, &["X-1"][..]),
            (Some(1), &["X-1"][..]),
        ] {
            assert_eq!(
                val("v", c, v, ids).unwrap_err(),
                LoadRecordError::InvalidValue(InvalidValueReason::ClassNotAllowed)
            );
        }
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
    // 出典を持てる区分（external）で上限と重複を見る。要件の値・初期値は常に拒否される。
    assert_eq!(
        val("v", ValueClass::External, None, &nine).unwrap_err(),
        LoadRecordError::TooManyRefs
    );
    assert!(val("v", ValueClass::External, None, &nine[..8]).is_ok());
    assert_eq!(
        val("v", ValueClass::External, None, &["A", "A"]).unwrap_err(),
        LoadRecordError::InvalidRef
    );
}

/// 形式検査を直接見る。公開の `new` は語彙との完全一致でも拒否するため、`new` 経由の否定試験だけでは
/// `is_name`・`is_ref_shape` を外した退行を検出できない（SECR-23）。
#[test]
fn shape_checks_reject_bad_candidates_directly() {
    for bad in ["", "A", "a-b", "a\nb", "a\u{2028}b", "a b", &"a".repeat(65)] {
        assert!(!is_name(bad), "is_name must reject {bad:?}");
    }
    assert!(is_name("p99_target"));
    assert!(is_name(&"a".repeat(64)));
    for bad in [
        "",
        "a\nb",
        "a\u{2028}b",
        "a\u{2029}b",
        "a,b",
        "a b",
        "a\0b",
        "none",
        "harness_config",
        "measured",
        &"a".repeat(65),
    ] {
        assert!(!is_ref_shape(bad), "is_ref_shape must reject {bad:?}");
    }
    assert!(is_ref_shape("AUDP-29"));
    assert!(is_ref_shape(&"a".repeat(64)));
}

/// 形式検査と語彙照合を分けて見る。不正な候補を語彙に含めて `name_in`・`ref_in`（`new` が呼ぶ処理）へ渡すため、
/// 形式検査の呼び出しを外すと、語彙に一致して通ってしまい、この試験が落ちる（SECR-23）。
#[test]
fn constructors_check_shape_independently_of_vocab() {
    let long = "a".repeat(65);
    let bad_names = ["", "A", "a-b", "a\nb", "a\u{2028}b", "a b", long.as_str()];
    assert!(!name_in("free_text", &["p99_target"]), "語彙の外は拒否");
    assert!(name_in("p99_target", &["p99_target"]));
    for bad in bad_names {
        assert!(
            !name_in(bad, &[bad]),
            "name_in must reject {bad:?} by shape"
        );
    }
    let bad_refs = [
        "",
        "a\nb",
        "a\u{2028}b",
        "a\u{2029}b",
        "a,b",
        "a b",
        "a\0b",
        "none",
        "harness_config",
        "measured",
        long.as_str(),
    ];
    assert!(!ref_in("CONFIG-SECRET-1", &["AUDP-29"]), "語彙の外は拒否");
    assert!(ref_in("AUDP-29", &["AUDP-29"]));
    for bad in bad_refs {
        assert!(!ref_in(bad, &[bad]), "ref_in must reject {bad:?} by shape");
    }
}

#[test]
fn input_validation() {
    for bad in ["", "A", "a-b", "a\nb", "a\u{2028}b", "a b", &"a".repeat(65)] {
        assert_eq!(
            ValueName::new(bad).unwrap_err(),
            LoadRecordError::InvalidName
        );
    }
    // 形が正しくても固定語彙の外は拒否する（語彙を足す口は無い。#6038）。
    assert_eq!(
        ValueName::new("free_text").unwrap_err(),
        LoadRecordError::InvalidName
    );
    assert!(ValueName::new("p99_target").is_ok());
    for reserved in ["none", "harness_config", "measured"] {
        assert_eq!(
            RefId::new(reserved).unwrap_err(),
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
        assert_eq!(RefId::new(bad).unwrap_err(), LoadRecordError::InvalidRef);
    }
    // 固定語彙に無い出典は、文字種が正しくても拒否する（SECR-23）。
    assert_eq!(
        RefId::new("CONFIG-SECRET-1").unwrap_err(),
        LoadRecordError::InvalidRef
    );
    assert!(RefId::new("AUDP-29").is_ok());
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
        IntervalName::new("client_response").unwrap(),
    )
}

fn plan() -> StagePlan {
    StagePlan::new(
        vec![StageConfig::new(LoadConfig::new(1, 3, 0).unwrap(), 1).unwrap()],
        MetricKey::P99Ns,
        StageLimits::new(4, 4, 100, 100).unwrap(),
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
    run_spec_in(session, exercised, &spec(), plan, calls)
}

fn run_spec_in(
    session: RunSession,
    exercised: &[ConditionComponent],
    spec: &TargetSpec,
    plan: &StagePlan,
    calls: &AtomicU64,
) -> Result<RunEvidence, LoadRecordError> {
    session.run(exercised, spec, plan, || {
        calls.fetch_add(1, Ordering::Relaxed);
        CallResult::Ok
    })
}

fn evidence(s: u64) -> Result<RunEvidence, LoadRecordError> {
    let calls = AtomicU64::new(0);
    run_in(
        RunSession::new(file_at(s), s, conditions())?,
        &[ConditionComponent::HubStub],
        &plan(),
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
            &plan(),
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

/// 段階の実行の誤りは `Stage` で返し、記録の元を作らない（区間名が固定語彙の外なら最初の回の前に拒否）。
#[test]
fn run_session_stage_error_is_reported() {
    let calls = AtomicU64::new(0);
    let other = TargetSpec::new(
        TargetName::new("route_a").unwrap(),
        IntervalName::new("other").unwrap(),
    );
    let e = run_spec_in(
        RunSession::new(file_at(0), 0, conditions()).unwrap(),
        &[ConditionComponent::HubStub],
        &other,
        &plan(),
        &calls,
    )
    .unwrap_err();
    assert_eq!(e, LoadRecordError::Stage(StageError::InvalidPlan));
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(e.to_string(), "load record error: stage run failed");
}

#[test]
fn invalid_value_reason_is_fixed_enum() {
    let e = val("v", ValueClass::External, None, &[]).unwrap_err();
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
    let v = ComparisonValue::new(
        vname(secret),
        ValueClass::External,
        None,
        ValueUnit::Ns,
        refs(&["CANARY-REF"]),
        &ApprovedValues::default(),
    )
    .unwrap();
    // 数値の欄（SECR-23）: 測定値の数値と名前が Debug に出ない。
    let secret_m = "canary_meas_zz";
    let m = ComparisonValue::new(
        vname(secret_m),
        ValueClass::Measured,
        Some(424_242),
        ValueUnit::Ns,
        vec![],
        &ApprovedValues::default(),
    )
    .unwrap();
    let dbg = format!(
        "{v:?} {m:?} {:?} {:?}",
        vname(secret),
        refs(&["CANARY-REF"])
    );
    let rec = build(0, 1, vec![v, m]).unwrap();
    let dbg = format!("{dbg} {rec:?} {:?}", file());
    for bad in [secret, secret_m, "CANARY-REF", "424242", "it-run-0-1-1"] {
        assert!(!dbg.contains(bad));
    }
    let e = RunRecordFile::new("bad\nlines").unwrap_err().to_string();
    assert!(!e.contains("bad"));
    let e = RefId::new("x\ny").unwrap_err().to_string();
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

/// 正当な最大の入力（出典 8 件が各 64 バイト・名前 64 バイト）で、形の網（語・行の上限）に当たらず
/// `LoadRecord::new` が通る（SECR-23・AAUD-30。網は正当な入力を拒否しない）。区間名の 64 バイトは
/// 固定語彙では作れないため、`stage` の試験が `StagedRun` を直接組んで確かめる。
#[test]
fn record_with_max_length_refs_and_names_is_accepted() {
    let ids: Vec<String> = (0..8).map(|i| format!("{i}{}", "r".repeat(63))).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let name = "n".repeat(64);
    let v = val(&name, ValueClass::External, None, &ids).unwrap();
    let ev = evidence(0).unwrap();
    let rec = LoadRecord::new(&ev, 1, vec![v]).unwrap();
    let text = rec.to_string();
    assert!(
        text.lines()
            .any(|l| l.matches(&"r".repeat(63)).count() == 8)
    );
}

/// 書き込みの回数を数える表示。1 回に 10 バイトを `n` 回書く。
struct Chunks<'a> {
    n: usize,
    writes: &'a Cell<usize>,
}

impl fmt::Display for Chunks<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for _ in 0..self.n {
            self.writes.set(self.writes.get() + 1);
            f.write_str("0123456789")?;
        }
        Ok(())
    }
}

/// #6038（上限を整形中に確かめる）: 上限を超えた最初の書き込みで打ち切り、後続を書かない。
/// 整形後に長さを見る旧来の作りには `BoundedText` が無く、この試験は成り立たない。
#[test]
fn bounded_text_stops_at_first_overflow() {
    let writes = Cell::new(0);
    let mut t = BoundedText::new(100);
    let r = write!(
        t,
        "{}",
        Chunks {
            n: 10_000,
            writes: &writes
        }
    );
    assert!(r.is_err());
    // 10 回（100 バイト）は通り、11 回目で拒否されて以後は書かれない。
    assert_eq!(writes.get(), 11);
    assert_eq!(t.buf.len(), 100);
    assert!(t.buf.capacity() <= 100);
}

#[test]
fn bounded_text_accepts_exact_limit_and_rejects_one_more() {
    let mut t = BoundedText::new(10);
    assert!(t.write_str("0123456789").is_ok());
    assert_eq!(t.buf.len(), 10);
    assert!(t.write_str("x").is_err());
    assert_eq!(t.buf.len(), 10);
    let mut t = BoundedText::new(10);
    assert!(t.write_str("012345").is_ok());
    assert!(t.write_str("67890").is_err());
    assert_eq!(t.buf, "012345");
    // 空の追記は通る。
    assert!(t.write_str("").is_ok());
}

/// 小さな上限では `Format` で拒否し、本番の上限では同じ入力が通る。上限ちょうどは受理する。
#[test]
fn compose_with_small_limit_is_format_error() {
    let ev = evidence(0).unwrap();
    assert!(matches!(
        compose(&ev, 1, &[], 64),
        Err(LoadRecordError::Format)
    ));
    let ok = compose(&ev, 1, &[], MAX_RECORD_BYTES).unwrap();
    let len = ok.to_string().len();
    assert!(compose(&ev, 1, &[], len).is_ok());
    assert!(matches!(
        compose(&ev, 1, &[], len - 1),
        Err(LoadRecordError::Format)
    ));
}

/// 固定語彙の各要素は形の規則を満たし、予約語を含まず、重複が無い（#6038）。
#[test]
fn fixed_vocabularies_are_well_formed() {
    let mut seen = std::collections::BTreeSet::new();
    for n in VALUE_NAMES {
        assert!(is_name(n));
        assert!(seen.insert(n));
        assert!(ValueName::new(n).is_ok());
    }
    let mut seen = std::collections::BTreeSet::new();
    for r in REF_IDS {
        assert!(is_ref_shape(r));
        assert!(!RESERVED_REFS.contains(&r));
        assert!(seen.insert(r));
        assert!(RefId::new(r).is_ok());
    }
}
