//! 契約の仮定の印と記録の試験（TASK-8・#77）。A-2026-10-28-12・ACPT-7。

use fandhe_auth_console_stub::contract::{
    CONTRACT_ASSUMPTION, ContractItem, ContractMark, contract_record,
};
use fandhe_auth_console_stub::record::CallKind;

/// `CallKind` の追加を網羅の match で検知する。
fn all_kinds() -> [CallKind; 7] {
    let kinds = [
        CallKind::DeviceAuthorization,
        CallKind::DeviceToken,
        CallKind::TokenRevocation,
        CallKind::RegistrationRevocation,
        CallKind::Introspection,
        CallKind::DeciderValidity,
        CallKind::HubValidity,
    ];
    for k in kinds {
        match k {
            CallKind::DeviceAuthorization
            | CallKind::DeviceToken
            | CallKind::TokenRevocation
            | CallKind::RegistrationRevocation
            | CallKind::Introspection
            | CallKind::DeciderValidity
            | CallKind::HubValidity => {}
        }
    }
    kinds
}

#[test]
fn assumption_id_is_a_2026_10_28_12() {
    assert_eq!(CONTRACT_ASSUMPTION, "A-2026-10-28-12");
}

#[test]
fn every_item_appears_exactly_once() {
    let rec = contract_record();
    assert_eq!(rec.entries.len(), 9);
    for k in all_kinds() {
        let n = rec
            .entries
            .iter()
            .filter(|e| e.item == ContractItem::Port(k))
            .count();
        assert_eq!(n, 1, "{k:?}");
    }
    for item in [
        ContractItem::ConsoleAudience,
        ContractItem::DeciderValidityCaller,
    ] {
        assert_eq!(rec.entries.iter().filter(|e| e.item == item).count(), 1);
    }
}

#[test]
fn ports_are_all_assumed() {
    let rec = contract_record();
    for k in all_kinds() {
        let e = rec.find(ContractItem::Port(k)).expect("entry");
        assert_eq!(e.mark, ContractMark::Assumed, "{k:?}");
    }
}

#[test]
fn decided_marks_are_only_the_two_items_of_2026_11_05() {
    let rec = contract_record();
    let decided = rec
        .entries
        .iter()
        .filter(|e| matches!(e.mark, ContractMark::DecidedInSpec { .. }))
        .count();
    assert_eq!(decided, 2);
    assert_eq!(
        rec.find(ContractItem::ConsoleAudience).unwrap().mark,
        ContractMark::DecidedInSpec {
            decisions: &["D-2026-11-05-5"]
        }
    );
    assert_eq!(
        rec.find(ContractItem::DeciderValidityCaller).unwrap().mark,
        ContractMark::DecidedInSpec {
            decisions: &["D-2026-11-05-2"]
        }
    );
}

#[test]
fn display_names_assumption_and_marks_without_values() {
    let rec = contract_record();
    let s = rec.to_string();
    assert!(s.contains("A-2026-10-28-12"));
    assert!(s.contains("実物ではない"));
    for e in &rec.entries {
        assert!(s.contains(e.item.name()), "{}", e.item.name());
    }
    assert!(s.contains("仮定"));
    assert!(!s.contains("確定"));
    assert!(!s.contains("://"));
}

#[test]
fn mark_labels_never_claim_confirmed_or_verified() {
    for m in [
        ContractMark::Assumed,
        ContractMark::DecidedInSpec {
            decisions: &["D-2026-11-05-5"],
        },
    ] {
        match m {
            ContractMark::Assumed | ContractMark::DecidedInSpec { .. } => {}
        }
        let l = m.label();
        assert!(!l.contains("確定"));
        assert!(!l.contains("検証済み"));
    }
}
