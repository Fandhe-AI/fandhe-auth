//! 契約テストの枠（`runner`）を、crate の外から公開 API だけで空の実装 2 型に流す結合試験（TASK-9 8/8）。
//!
//! 空の実装はストレージを持たない型で、TASK-716 のメモリ実装でも PostgreSQL・`fandhe-db` の実装でもない。
//! ケース表は空（TASK-584 で埋める）なので、ここで確かめるのは「枠が実装の型ごとに動く」ことだけである。
//! 0 件の実行は実測なしで、どちらの契約の充足の根拠にもならない（AUDB-6・TASK-628）。
//! 合否を表す bool は作らず、期待も緩めない（AUDB-6・AUDB-69）。
//! 単体試験（`src/runner.rs`）との違い: 外部 crate の立場、別々の 2 型、本物の `cases()`、factory の呼び出し回数。

use fandhe_auth_db_tests::runner::{cases, run_all, run_cases};
use std::cell::Cell;

// TODO(TASK-584): S に契約 trait の境界が付き、ケース表が埋まったら本試験を書き直す

/// ストレージを持たない空の実装 A。
struct EmptyImplA;
/// ストレージを持たない空の実装 B（A とは別の型）。
struct EmptyImplB;

#[test]
fn real_case_table_is_empty_for_both_types_and_factory_is_never_called() {
    let calls_a = Cell::new(0usize);
    let a = run_cases(
        "empty-a",
        || {
            calls_a.set(calls_a.get() + 1);
            EmptyImplA
        },
        &cases::<EmptyImplA>(),
    );
    let calls_b = Cell::new(0usize);
    let b = run_cases(
        "empty-b",
        || {
            calls_b.set(calls_b.get() + 1);
            EmptyImplB
        },
        &cases::<EmptyImplB>(),
    );
    // 0 件は実測なし。合格ではない（AUDB-6）。
    assert_eq!(
        (a.implementation, a.executed(), a.failed()),
        ("empty-a", 0, 0)
    );
    assert_eq!(
        (b.implementation, b.executed(), b.failed()),
        ("empty-b", 0, 0)
    );
    assert_eq!((calls_a.get(), calls_b.get()), (0, 0));
}

#[test]
fn run_all_keeps_implementation_names_in_order_with_zero_executed() {
    // `run_all` は 1 つの型 S だけを受ける。別々の型は型ごとに呼ぶ（S の境界は TASK-584）。
    let table = cases::<EmptyImplA>();
    let reports = run_all(
        &[("first", &|| EmptyImplA), ("second", &|| EmptyImplA)],
        &table,
    );
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].implementation, "first");
    assert_eq!(reports[1].implementation, "second");
    assert!(reports.iter().all(|r| r.executed() == 0 && r.failed() == 0));

    let table_b = cases::<EmptyImplB>();
    let reports_b = run_all(&[("only-b", &|| EmptyImplB)], &table_b);
    assert_eq!(reports_b.len(), 1);
    assert_eq!(reports_b[0].implementation, "only-b");
    assert_eq!(reports_b[0].executed(), 0);
}
