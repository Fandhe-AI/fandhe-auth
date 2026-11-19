//! 契約テストを複数の DB 実装で流す枠（TASK-9。名前はすべて仮称で、人が確定する）。
//!
//! 呼び出し側（組込み先の試験・将来の PostgreSQL 向け／`fandhe-db` 向けの試験）が、試す実装の型 `S` と
//! 「ケースごとに新しいインスタンスを作る関数」を渡し、同じケース表を実装ごとに流す。
//! 本 module は実装の中身を知らない。`sqlx`・DB ドライバ・非同期ランタイムに依存せず、SQL も持たない
//! （DPOL-37・AUDB-3・LIB-2）。
//!
//! 規則:
//!
//! - ケースごとに factory から新しいインスタンスを作り、前のケースの状態を持ち込まない。
//! - 1 件が不合格でも止めず、表の全ケースを流す（不合格を取りこぼさない。AUDB-4 の向き）。
//! - 報告は実装の名前・流した件数・ケースごとの結果を持つ。ケースが 0 件の実行を「契約を満たした」
//!   と読ませない。合格を表す bool は置かず、流した件数と不合格の件数を別々に返す（AUDB-6）。
//!   0 件の実行は、PostgreSQL・`fandhe-db` いずれの契約の充足の根拠にもならない（TASK-628）。
//! - 不合格の理由の文字列に秘密値・接続文字列を入れない（SECR-23〜27）。
//! - 試験用の切り替えの feature・環境変数・設定を足さない（DPOL-30〜32）。
//!
//! 公開マクロ・dev-dependencies での提供形は TASK-718（LIB-11・LIB-12）、メモリ実装は TASK-716（LIB-6）、
//! 変異による検知は TASK-585（AUDB-4）の範囲で、ここでは作らない。

// TODO(TASK-584): `S` の境界を契約 trait にする。契約 trait の置き場は DPOL-38 で未定。
// async になる場合も非同期ランタイムに依存しない形を TASK-584・TASK-4 で決める（LIB-2）。
// 並行のケース（LIB-6・AUDB-2）で `S: Sync` やスレッドが要るかもそのとき決める。
// TODO(TASK-584): ケース表（`cases`）に ADB-N のケースを入れる。

/// 1 件のケース。`id` は後で `ADB-N` を入れる欄で、`run` は不合格の理由を `Err` で返す。
pub struct Case<S> {
    /// ケースの識別子（`ADB-N` 等）。
    pub id: &'static str,
    /// 新しいインスタンスを受けて合否を返す関数。
    pub run: fn(&S) -> Result<(), String>,
}

/// ケース 1 件の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseOutcome {
    /// ケースの識別子。
    pub id: &'static str,
    /// 合格なら `None`、不合格なら理由（秘密値を含めない。SECR-23〜27）。
    pub failure: Option<String>,
}

/// 1 つの実装に対する実行の報告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// 呼び出し側が渡した実装の表示名。
    pub implementation: &'static str,
    /// ケースごとの結果（表の順）。
    pub outcomes: Vec<CaseOutcome>,
}

impl Report {
    /// 流したケースの件数。0 件は実測なしを意味し、合格として扱わない（AUDB-6）。
    pub fn executed(&self) -> usize {
        self.outcomes.len()
    }

    /// 不合格の件数。
    pub fn failed(&self) -> usize {
        self.outcomes.iter().filter(|o| o.failure.is_some()).count()
    }
}

/// ケース表。いまは空（TASK-584 で中身を入れる）。
pub fn cases<S>() -> Vec<Case<S>> {
    Vec::new()
}

/// 1 つの実装に表の全ケースを流す。ケースごとに `factory` で新しいインスタンスを作る。
pub fn run_cases<S>(
    implementation: &'static str,
    factory: impl Fn() -> S,
    table: &[Case<S>],
) -> Report {
    let outcomes = table
        .iter()
        .map(|case| {
            let instance = factory();
            CaseOutcome {
                id: case.id,
                failure: (case.run)(&instance).err(),
            }
        })
        .collect();
    Report {
        implementation,
        outcomes,
    }
}

/// 複数の実装に同じ表を流す薄い補助。実装ごとの報告を返す。
pub fn run_all<S>(
    implementations: &[(&'static str, &dyn Fn() -> S)],
    table: &[Case<S>],
) -> Vec<Report> {
    implementations
        .iter()
        .map(|(name, factory)| run_cases(name, factory, table))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // ストレージを持たない試験用の型（TASK-716 のメモリ実装ではない）。
    struct ImplA;
    struct ImplB;

    fn pass_a(_: &ImplA) -> Result<(), String> {
        Ok(())
    }
    fn fail_a(_: &ImplA) -> Result<(), String> {
        Err("expected failure".to_string())
    }
    fn pass_b(_: &ImplB) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn same_entry_runs_two_implementations() {
        // 枠は実装の型に generic で、同じ入口を実装ごとに通す（DPOL-37）。
        let table_a = [
            Case {
                id: "c1",
                run: pass_a,
            },
            Case {
                id: "c2",
                run: pass_a,
            },
        ];
        let table_b = [
            Case {
                id: "c1",
                run: pass_b,
            },
            Case {
                id: "c2",
                run: pass_b,
            },
        ];
        let a = run_cases("a", || ImplA, &table_a);
        let b = run_cases("b", || ImplB, &table_b);
        assert_eq!((a.implementation, a.executed(), a.failed()), ("a", 2, 0));
        assert_eq!((b.implementation, b.executed(), b.failed()), ("b", 2, 0));
        let both = run_all(&[("a", &|| ImplA), ("a2", &|| ImplA)], &table_a);
        assert_eq!(both.len(), 2);
        assert_eq!(both[1].implementation, "a2");
    }

    #[test]
    fn failure_is_attributed_and_later_cases_still_run() {
        // AUDB-4 の向き: 不合格で止めず、流した実装の報告にだけ現れる。
        let bad = [
            Case {
                id: "c1",
                run: fail_a,
            },
            Case {
                id: "c2",
                run: pass_a,
            },
        ];
        let good = [
            Case {
                id: "c1",
                run: pass_a,
            },
            Case {
                id: "c2",
                run: pass_a,
            },
        ];
        let reports = run_all(&[("bad", &|| ImplA)], &bad);
        assert_eq!(reports[0].executed(), 2);
        assert_eq!(reports[0].failed(), 1);
        assert_eq!(
            reports[0].outcomes[0].failure.as_deref(),
            Some("expected failure")
        );
        let ok = run_cases("good", || ImplA, &good);
        assert_eq!(ok.failed(), 0);
    }

    #[test]
    fn factory_is_called_once_per_case() {
        let calls = Cell::new(0usize);
        let table = [
            Case {
                id: "c1",
                run: pass_a,
            },
            Case {
                id: "c2",
                run: pass_a,
            },
            Case {
                id: "c3",
                run: pass_a,
            },
        ];
        let r = run_cases(
            "a",
            || {
                calls.set(calls.get() + 1);
                ImplA
            },
            &table,
        );
        assert_eq!(calls.get(), table.len());
        assert_eq!(r.executed(), table.len());
    }

    #[test]
    fn empty_table_reports_zero_executed_not_a_pass() {
        // AUDB-6: 0 件は件数 0 として見え、合格の bool は無い。
        let table = cases::<ImplA>();
        assert!(table.is_empty());
        let r = run_cases("a", || ImplA, &table);
        assert_eq!(r.executed(), 0);
        assert_eq!(r.failed(), 0);
    }
}
