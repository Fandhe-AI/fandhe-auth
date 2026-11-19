//! ブラウザと仮想 authenticator の種類、および「仮想は実機・Safari に数えない」印。
//!
//! 呼び出し元は [`crate::harness::E2eConfig`] と実行の記録（5/6 以降）。判定は網羅の `match` で書くので、
//! 種類を足して判定を書き忘れるとコンパイルできない（ACPT-7・ACPT-8）。

/// 対象のブラウザ。Chrome と Firefox だけ。
///
/// Safari の変種は置かない。Safari の結果はこのハーネスの型に入る経路が無い（`TASK-784`・`TASK-75`〜`78` の担当）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Browser {
    /// Chrome（chromedriver。2/6 で結線）。
    Chrome,
    /// Firefox（geckodriver。3/6 で結線）。
    Firefox,
}

/// authenticator の種類。WebDriver の仮想 authenticator の 1 種類だけ。
///
/// 仮想 authenticator の options（protocol・transport・resident key・UV）の値は 1/6 で決めない
/// （WebDriver の Virtual Authenticator 拡張への写像とあわせて 2/6 で決める。PoC の値を要件の値として置かない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthenticatorKind {
    /// WebDriver の仮想 authenticator。
    WebDriverVirtual,
}

impl AuthenticatorKind {
    /// この authenticator の結果の証拠の種類。
    pub fn evidence(self) -> Evidence {
        match self {
            AuthenticatorKind::WebDriverVirtual => Evidence::Virtual,
        }
    }
}

/// 結果の証拠の種類。〔仮想〕だけで、実機・Safari の変種を置かない（ACPT-7・ACPT-8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Evidence {
    /// 仮想 authenticator の結果。
    Virtual,
}

impl Evidence {
    /// 実機の結果として数えるか。常に `false`（ACPT-7・ACPT-8）。
    pub fn counts_as_real_device(self) -> bool {
        match self {
            Evidence::Virtual => false,
        }
    }

    /// Safari の結果として数えるか。常に `false`（ACPT-7・ACPT-8。Safari は `TASK-784` の担当）。
    pub fn counts_as_safari(self) -> bool {
        match self {
            Evidence::Virtual => false,
        }
    }
}
