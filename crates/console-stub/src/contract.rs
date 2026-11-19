//! console の stub が置いた契約の仮定の印と記録（TASK-8・#77）。試験専用で、実物の console ではない。
//!
//! 役割: console の文書ができるまで、口の形・項目は仮定で進める（A-2026-10-28-12。根拠は
//! Hub DELEG-9・Hub DELEG-14・Hub DELEG-20 と D-2026-10-17-5・D-2026-10-17-49 の (3)）。どの項目が仮定で、
//! どの項目が spec の決定で決まっているかを、静的な ID と印だけで残す。呼び出し元は試験と、
//! `docs/stubs/console-stub.md`（#79）を書く人。
//!
//! 設計上の約束:
//! - 値・URL・ホスト名・識別子を持たない。出すのは項目名・印・根拠の ID だけ（SECR-23〜27）。
//! - 印は「仮定」と「決定（実物の console との接続は未検証）」の 2 種類だけ。確定・検証済み・実物を表す
//!   種類は作らない（ACPT-7）。stub の合格を実物の合格に数えない。
//! - 決定の印を付けるのは TASK-8 の 2026-11-05 追記で決まった 2 項目だけ（D-2026-11-05-2・D-2026-11-05-5）。
//!   印の割り当ては書き手の読みで、見直しと「確定」への変更は人が行う。
//! - 振る舞いは変えない。ネットワーク・環境変数・ファイルを扱わない（DPOL-32）。
//!
//! 対象外: introspection の応答の登録 ID（AGCR-57。TASK-808）、設定値の仮置き（A-2026-10-28-14・TASK-451）、
//! 実物との差は `docs/stubs/console-stub.md`（#79）に書いた。

use std::fmt;

use crate::record::CallKind;

/// 契約が仮定であることの根拠となる仮定 ID（A-2026-10-28-12）。
pub const CONTRACT_ASSUMPTION: &str = "A-2026-10-28-12";

/// 契約の項目の印。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractMark {
    /// console の文書ができるまでの仮定（A-2026-10-28-12）。
    Assumed,
    /// spec の決定で決まった部分。実物の console との接続は未検証。
    DecidedInSpec { decisions: &'static [&'static str] },
}

impl ContractMark {
    /// 印の語。「確定」「検証済み」の語は使わない（ACPT-7）。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Assumed => "仮定",
            Self::DecidedInSpec { .. } => "決定（実物の console との接続は未検証）",
        }
    }
}

impl fmt::Display for ContractMark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())?;
        if let Self::DecidedInSpec { decisions } = self {
            write!(f, " [{}]", decisions.join("・"))?;
        }
        Ok(())
    }
}

/// 契約の項目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractItem {
    /// 口の本文の形・経路名。
    Port(CallKind),
    /// console のクライアントの `aud` の形（OCL-58）。
    ConsoleAudience,
    /// ② の呼び出し元の認証・照会の対象・応答の項目（ADLG-29）。
    DeciderValidityCaller,
}

impl ContractItem {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Port(kind) => kind.as_str(),
            Self::ConsoleAudience => "console_audience",
            Self::DeciderValidityCaller => "decider_validity_caller",
        }
    }
}

/// 項目・印・根拠の ID の 1 組。`refs` は振る舞い ID で、Hub の ID には `Hub ` を前置する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractEntry {
    pub item: ContractItem,
    pub mark: ContractMark,
    pub refs: &'static [&'static str],
}

/// 口ごとの項目。`CallKind` を足したときに直し忘れるとコンパイルできない。
fn port_entry(kind: CallKind) -> ContractEntry {
    let refs: &'static [&'static str] = match kind {
        CallKind::DeviceAuthorization => &["DGRT-1", "AGCR-8"],
        CallKind::DeviceToken => &["DGRT-7", "DGRT-8", "AGCR-1"],
        CallKind::TokenRevocation => &["AGCR-43", "OPRV-53"],
        // 経路名は最終でない（D-2026-10-17-84）。
        CallKind::RegistrationRevocation => &["AGCR-35", "AGCR-36", "Hub ARUN-58"],
        CallKind::Introspection => &["ADLG-15", "OPRV-54", "AGCR-32"],
        CallKind::DeciderValidity => &["ADLG-29", "Hub ARUN-60"],
        CallKind::HubValidity => &["ADLG-31"],
    };
    ContractEntry {
        item: ContractItem::Port(kind),
        mark: ContractMark::Assumed,
        refs,
    }
}

/// 契約の記録。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractRecord {
    pub entries: Vec<ContractEntry>,
}

impl ContractRecord {
    pub fn find(&self, item: ContractItem) -> Option<&ContractEntry> {
        self.entries.iter().find(|e| e.item == item)
    }
}

/// 契約の記録を返す。7 つの口を固定の順で並べ、続けて口でない 2 項目を置く。
pub fn contract_record() -> ContractRecord {
    let mut entries: Vec<ContractEntry> = [
        CallKind::DeviceAuthorization,
        CallKind::DeviceToken,
        CallKind::TokenRevocation,
        CallKind::RegistrationRevocation,
        CallKind::Introspection,
        CallKind::DeciderValidity,
        CallKind::HubValidity,
    ]
    .into_iter()
    .map(port_entry)
    .collect();
    entries.push(ContractEntry {
        item: ContractItem::ConsoleAudience,
        mark: ContractMark::DecidedInSpec {
            decisions: &["D-2026-11-05-5"],
        },
        refs: &["OCL-58"],
    });
    entries.push(ContractEntry {
        item: ContractItem::DeciderValidityCaller,
        mark: ContractMark::DecidedInSpec {
            decisions: &["D-2026-11-05-2"],
        },
        refs: &["ADLG-29"],
    });
    ContractRecord { entries }
}

impl fmt::Display for ContractRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "console の stub（実物ではない）。console の文書ができるまで契約は仮定（{CONTRACT_ASSUMPTION}。"
        )?;
        write!(
            f,
            "根拠: Hub DELEG-9・Hub DELEG-14・Hub DELEG-20・D-2026-10-17-5・D-2026-10-17-49 の (3)）。"
        )?;
        writeln!(f, "実物との接続は未検証")?;
        for e in &self.entries {
            writeln!(f, "{}: {}（{}）", e.item.name(), e.mark, e.refs.join("・"))?;
        }
        Ok(())
    }
}
