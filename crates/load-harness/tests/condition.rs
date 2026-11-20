//! TASK-12 7/10: 測定の条件（機材・構成・負荷の型）の記録の行の試験。
//!
//! 偽の取得元で決定的にする。実機の値・時間では判定しない。stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）。
//! 形式は `docs/harness/load.md` の 5・7。7 の見本の `condition_loop` は `undecided` だが、5/10 の閉ループの仮置きで `closed` になる。

use fandhe_auth_load_harness::{
    BuildProfile, ComponentConditions, ConditionComponent, ConditionError, LoopModel,
    MachineCondition, MachineProbe, MeasurementConditions, StdMachineProbe, Virtualization,
};

const TAIL: &str = "class=test_condition refs=harness_config";
const CANARY: &str = "load-canary-Machine.example.test";

struct FakeProbe {
    arch: Option<String>,
    os: Option<String>,
}

impl FakeProbe {
    fn unknown() -> Self {
        Self {
            arch: None,
            os: None,
        }
    }
}

impl MachineProbe for FakeProbe {
    fn cpu_count(&self) -> Option<u64> {
        None
    }
    fn memory_mib(&self) -> Option<u64> {
        None
    }
    fn arch(&self) -> Option<String> {
        self.arch.clone()
    }
    fn os_family(&self) -> Option<String> {
        self.os.clone()
    }
    fn virtualization(&self) -> Virtualization {
        Virtualization::Unknown
    }
    fn build_profile(&self) -> BuildProfile {
        BuildProfile::Unknown
    }
}

fn conds(started: &[(ConditionComponent, bool)]) -> MeasurementConditions {
    MeasurementConditions::new(
        MachineCondition::detect_with(&FakeProbe::unknown()).unwrap(),
        ComponentConditions::new(started).unwrap(),
        LoopModel::Closed,
    )
}

#[test]
fn golden_matches_sample_with_closed_loop() {
    let out = conds(&[(ConditionComponent::HubStub, true)]).to_string();
    let mut expected = String::new();
    for k in [
        "cpu_count",
        "memory_mib",
        "arch",
        "os_family",
        "virtualization",
        "build_profile",
    ] {
        expected.push_str(&format!("condition_machine: {k} unknown {TAIL}\n"));
    }
    expected.push_str(&format!(
        "condition_component: Database not_started {TAIL}\n"
    ));
    expected.push_str(&format!("condition_component: Host not_started {TAIL}\n"));
    expected.push_str(&format!(
        "condition_component: HubStub started stub {TAIL}\n"
    ));
    expected.push_str(&format!(
        "condition_component: ConsoleStub not_started {TAIL}\n"
    ));
    expected.push_str("note: stub results are not counted as real results\n");
    expected.push_str(&format!("condition_loop: closed {TAIL}\n"));
    assert_eq!(out, expected);
}

#[test]
fn rows_always_present_and_note_only_with_stub() {
    let out = conds(&[]).to_string();
    assert_eq!(out.matches("condition_machine: ").count(), 6);
    assert_eq!(out.matches("condition_component: ").count(), 4);
    assert_eq!(out.matches("not_started").count(), 4);
    assert_eq!(out.matches("note:").count(), 0);
    assert_eq!(
        out.lines()
            .filter(|l| *l == format!("condition_loop: closed {TAIL}"))
            .count(),
        1
    );

    for started in [
        vec![(ConditionComponent::ConsoleStub, true)],
        vec![
            (ConditionComponent::HubStub, true),
            (ConditionComponent::ConsoleStub, true),
        ],
    ] {
        assert_eq!(conds(&started).to_string().matches("note:").count(), 1);
    }
    let real = conds(&[(ConditionComponent::Database, false)]).to_string();
    assert!(real.contains("condition_component: Database started real "));
    assert_eq!(real.matches("note:").count(), 0);
}

#[test]
fn components_reject_duplicate_and_stub_mismatch() {
    assert_eq!(
        ComponentConditions::new(&[
            (ConditionComponent::HubStub, true),
            (ConditionComponent::HubStub, true)
        ])
        .unwrap_err(),
        ConditionError::DuplicateComponent
    );
    assert_eq!(
        ComponentConditions::new(&[(ConditionComponent::Database, true)]).unwrap_err(),
        ConditionError::StubMismatch
    );
    assert_eq!(
        ComponentConditions::new(&[(ConditionComponent::HubStub, false)]).unwrap_err(),
        ConditionError::StubMismatch
    );
    let e = ConditionError::StubMismatch;
    assert!(!format!("{e} {e:?}").contains("HubStub"));
}

#[test]
fn probe_values_outside_vocab_are_errors_without_leaking() {
    for bad in [
        CANARY.to_owned(),
        "Upper".to_owned(),
        "line\nbreak".to_owned(),
        "a".repeat(65),
        String::new(),
        "sep\u{2028}x".to_owned(),
        "myhost01".to_owned(),
    ] {
        for (arch, os) in [(Some(bad.clone()), None), (None, Some(bad.clone()))] {
            let e = MachineCondition::detect_with(&FakeProbe { arch, os }).unwrap_err();
            assert_eq!(e, ConditionError::InvalidMachineValue);
            assert!(!format!("{e} {e:?}").contains("canary"));
        }
    }
    // std を使える x86_64-unknown-hermit の OS 名は許可語彙に含める。
    for os in ["hermit", "teeos", "uefi"] {
        assert!(
            MachineCondition::detect_with(&FakeProbe {
                arch: Some("x86_64".to_owned()),
                os: Some(os.to_owned()),
            })
            .is_ok()
        );
    }
    let ok = MachineCondition::detect_with(&FakeProbe {
        arch: Some("x86_64".to_owned()),
        os: Some("linux".to_owned()),
    })
    .unwrap()
    .to_string();
    assert!(ok.contains("arch x86_64 "));
    assert!(ok.contains("os_family linux "));
    // 標準の取得元が返す arm64ec（Windows ARM64EC）は語彙に入れて受理する。
    let ec = MachineCondition::detect_with(&FakeProbe {
        arch: Some("arm64ec".to_owned()),
        os: Some("windows".to_owned()),
    })
    .unwrap()
    .to_string();
    assert!(ec.contains("arch arm64ec "));
    // 取得不能（None）は unknown。
    let none = MachineCondition::detect_with(&FakeProbe::unknown())
        .unwrap()
        .to_string();
    assert!(none.contains(&format!("condition_machine: arch unknown {TAIL}\n")));
}

#[test]
fn std_probe_build_profile_is_unknown() {
    assert_eq!(StdMachineProbe.build_profile(), BuildProfile::Unknown);
}

#[test]
fn explicit_machine_rejects_invalid_strings() {
    let r = MachineCondition::new(
        Some(4),
        Some(8),
        Some(CANARY.to_owned()),
        None,
        Virtualization::None,
        BuildProfile::Release,
    );
    assert_eq!(r.unwrap_err(), ConditionError::InvalidMachineValue);
    let r = MachineCondition::new(
        None,
        None,
        Some("line\nbreak".to_owned()),
        None,
        Virtualization::None,
        BuildProfile::Release,
    );
    assert_eq!(r.unwrap_err(), ConditionError::InvalidMachineValue);
    let r = MachineCondition::new(
        None,
        None,
        Some("arm64".to_owned()),
        None,
        Virtualization::None,
        BuildProfile::Release,
    );
    assert_eq!(r.unwrap_err(), ConditionError::InvalidMachineValue);
    let ok = MachineCondition::new(
        Some(4),
        Some(8),
        Some("aarch64".to_owned()),
        Some("linux".to_owned()),
        Virtualization::Vm,
        BuildProfile::Release,
    )
    .unwrap()
    .to_string();
    assert!(ok.contains("cpu_count 4 "));
    assert!(ok.contains("memory_mib 8 "));
    assert!(ok.contains("virtualization vm "));
    assert!(ok.contains("build_profile release "));
}

#[test]
fn real_probe_smoke_checks_shape_only() {
    let out =
        MeasurementConditions::detect(ComponentConditions::new(&[]).unwrap(), LoopModel::Closed)
            .unwrap()
            .to_string();
    let machine: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("condition_machine: "))
        .collect();
    assert_eq!(machine.len(), 6);
    for l in machine {
        let v = l.split(' ').nth(2).unwrap();
        let ok = v == "unknown"
            || v.parse::<u64>().is_ok()
            || v.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        assert!(ok, "unexpected value shape");
    }
    // 取得元は std のものを直接も使える。
    MachineCondition::detect_with(&StdMachineProbe).unwrap();
}
