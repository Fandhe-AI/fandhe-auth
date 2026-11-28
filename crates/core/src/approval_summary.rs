//! エージェントの承認画面に出す要約（`ApprovalSummary`）の型と、要約の内容の規則。
//!
//! 呼び出し元: 組込み先（Hub）が、Device Authorization の要求から要約を組み立てて画面へ渡す。
//! 表示側の TASK-175 がこの型の受け手になる（2 つ目の要約の型を作らない）。
//! 描画・文言・観察の実施は組込み先の担当で、本体は規則を表す型だけを持つ
//! （`LIB-1`・`LIB-3`・`D-2026-10-30-1`）。名前はすべて仮称で、人の確認待ち。
//!
//! # 要約の内容の規則
//!
//! 1. 要約は 5 項目で構成する: クライアント・scope・申告した環境・開始の時刻（UTC）・開始の送信元
//!    （`UISF-24`。[`SummaryItem::ALL`]）。
//! 2. 項目ごとに出所（[`Provenance`]）を持つ。申告した環境だけが「エージェントの申告で、検証されて
//!    いない」もの。開始の時刻と送信元はサーバーが観測した値（`DGRT-4`）、scope は要求に記録した値
//!    （`DGRT-3`）。組込み先は申告の項目を観測の項目と別の欄に出し、「検証されていない」ことを示す
//!    ラベルを付ける（`UISF-12`）。
//! 3. ラベルの文言は本体に持たない（Hub `ASCR-40`）。PoC の文言も定数にしない。
//! 4. 申告した環境は表示専用の自由文で、判定（承認の可否・scope・有効期限）の入力にしない
//!    （`ADLG-7`・`HAPR-1`）。判定の関数はこの型を引数に取らず、観測の型との相互の変換も持たない。
//!    型 [`DeclaredEnvironment`] は「エージェントの申告で、検証されていない」ことを型で分けるもので、
//!    値の検査は行わない。
//! 5. 申告した環境の検査（HTML の特殊文字は文字列のまま受ける・制御文字・双方向制御・ゼロ幅・BOM と
//!    長さ超過は拒否する、という `DGRT-33` の規則）は、本体の `AgentEnv`（`TASK-412`・#3871）が
//!    行う。長さの上限の値は `DGRT-21`（`DeviceGrantPolicy`・`TASK-417`）の側に残る。
//!    **現状は未検査の値を受ける**: この module は検査も上限の型も持たず、
//!    [`DeclaredEnvironment::from_unverified_declaration`] は受けた `String` をそのまま保つ。
//!    検査の前に呼ぶと、制御文字・双方向制御・長すぎる値も要約に入る。追跡の中身は、#3871 で
//!    この引数を `AgentEnv` の検査済みの型に替え、`String` の口をなくすこと（`TODO(#3871)`）。
//! 6. 要約は出力専用で、承認・拒否の能力を持たない。確認の手順の判定は `DVUI-12`・`DVUI-13`・
//!    `DVUI-15`・`HAPR-3` が正本で、画面は結果を受けるだけ（`UISF-14`）。
//! 7. ユーザーコードを URL に含む案内の項目を持たない（`UISF-13`・`DGRT-2`）。ユーザーコードを
//!    要約に載せるかは未決で、骨組みでは持たない。載せない既定として断定しない。根拠が割れている:
//!    承認画面はユーザーコードの表示を求め（`UISF-24`）、エージェント側の表示と照合する案内を
//!    求める（`UISF-37`）。一方、保存の形は低エントロピーの秘密値の扱い（`SECR-7`・`SECR-34`）で、
//!    これは保存の形の行であり、画面に出す可否を決める行ではない。判断は人に預ける（`TODO(#5841)`）。
//! 8. HTML へ出すときはエスケープする（`UISF-2`）。`SafeHtml` との結合は
//!    `TODO(#5842)`。
//! 9. 観察（`UISF-37`）の実施は Hub の `ASCR-37` と人が行い、AI・自動処理に代えない
//!    （`D-2026-10-17-51` の (1)）。観察の仕様は `TODO(#5847)`。
//! 10. `UISF-38`（交換専用の scope を含む要求の別表示）は規則としてだけ置く。項目は持たない。
//!     トークン交換は MS-12 に延期されている（`D-2026-11-06-4`）ので、今実装するかは人に伺う
//!     （`TODO(#5841)`）。
//! 11. ステータス: `UISF-12`・`UISF-13`・`UISF-14`・`UISF-37`・`UISF-38` は検討中、`UISF-24` は確定の
//!     範囲つき。型を置いても、どの行も満たしたことにはならない（`ACPT-7`）。
//! 12. I/O・時刻・乱数を持たず、HTTP・Hub・DB の型を出さない（`DPOL-1`・`LIB-3`）。
//!
//! # 申告と観測の取り違えを型で拒む
//!
//! 安定版の rustdoc は `compile_fail` のエラーコードを照合しない。コードを付けても、別の理由で
//! 落ちていないことは下の陽性対照（通る doctest）で確かめる。
//!
//! 申告から観測への変換は無い。
//!
//! ```compile_fail,E0277
//! use fandhe_auth_core::approval_summary::{DeclaredEnvironment, StartSource};
//! let d = DeclaredEnvironment::from_unverified_declaration("x".to_string());
//! let _ = StartSource::from(d);
//! ```
//!
//! 観測から申告への変換も無い（送信元）。
//!
//! ```compile_fail,E0277
//! use std::net::{IpAddr, Ipv4Addr};
//! use fandhe_auth_core::approval_summary::{DeclaredEnvironment, StartSource};
//! let s = StartSource::from_observed_peer(IpAddr::V4(Ipv4Addr::LOCALHOST));
//! let _ = DeclaredEnvironment::from(s);
//! ```
//!
//! 観測から申告への変換も無い（開始の時刻）。
//!
//! ```compile_fail,E0277
//! use fandhe_auth_core::approval_summary::{DeclaredEnvironment, StartedAt};
//! let t = StartedAt::from_unix_millis_utc(0);
//! let _ = DeclaredEnvironment::from(t);
//! ```
//!
//! `Display` を持たない。
//!
//! ```compile_fail,E0277
//! use fandhe_auth_core::approval_summary::DeclaredEnvironment;
//! let d = DeclaredEnvironment::from_unverified_declaration("x".to_string());
//! let _ = format!("{}", d);
//! ```
//!
//! 検査を通さない `From<&str>` を持たない。
//!
//! ```compile_fail,E0277
//! use fandhe_auth_core::approval_summary::DeclaredEnvironment;
//! let _ = DeclaredEnvironment::from("x");
//! ```
//!
//! `#[non_exhaustive]` のため、wildcard なしの網羅的な match は通らない。
//!
//! ```compile_fail,E0004
//! use fandhe_auth_core::approval_summary::SummaryItem;
//! fn f(i: SummaryItem) -> u8 {
//!     match i {
//!         SummaryItem::Client => 0,
//!         SummaryItem::Scope => 1,
//!         SummaryItem::DeclaredEnvironment => 2,
//!         SummaryItem::StartedAt => 3,
//!         SummaryItem::StartSource => 4,
//!     }
//! }
//! ```
//!
//! 陽性対照（上の各 `compile_fail` が、型名・列挙子の綴りや import の誤りで落ちていないことの確認）。
//! 同じ型・列挙子・関数に触れ、変換と `Display` だけを除いている:
//!
//! ```
//! use std::net::{IpAddr, Ipv4Addr};
//! use fandhe_auth_core::approval_summary::*;
//! let d = DeclaredEnvironment::from_unverified_declaration("env".to_string());
//! let s = StartSource::from_observed_peer(IpAddr::V4(Ipv4Addr::LOCALHOST));
//! let t = StartedAt::from_unix_millis_utc(0);
//! // `Debug` はある（`Display` だけが無い）。値は出ない。
//! assert_eq!(format!("{d:?}"), "DeclaredEnvironment(..)");
//! assert_eq!(s.addr(), IpAddr::V4(Ipv4Addr::LOCALHOST));
//! assert_eq!(t.unix_millis_utc(), 0);
//! // 列挙子に触れる。wildcard を付ければ通る（`#[non_exhaustive]`）。
//! fn f(i: SummaryItem) -> u8 {
//!     match i {
//!         SummaryItem::Client => 0,
//!         SummaryItem::Scope => 1,
//!         SummaryItem::DeclaredEnvironment => 2,
//!         SummaryItem::StartedAt => 3,
//!         SummaryItem::StartSource => 4,
//!         _ => 255,
//!     }
//! }
//! let got: Vec<u8> = SummaryItem::ALL.iter().map(|i| f(*i)).collect();
//! assert_eq!(got, [0, 1, 2, 3, 4]);
//! let summary = ApprovalSummary::new(
//!     "client".to_string(),
//!     "client-id".to_string(),
//!     vec!["read".to_string()],
//!     d,
//!     t,
//!     s,
//! );
//! assert_eq!(summary.client_id(), "client-id");
//! assert_eq!(summary.declared_environment().as_unescaped_text(), "env");
//! ```

use std::net::IpAddr;

/// 要約の項目（`UISF-24`）。後続の走査・契約テストが [`SummaryItem::ALL`] で引く。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SummaryItem {
    /// クライアントの名前と ID。
    Client,
    /// 要求した scope。
    Scope,
    /// エージェントが申告した環境（検証されていない）。
    DeclaredEnvironment,
    /// 開始の時刻（UTC）。
    StartedAt,
    /// 開始の送信元。
    StartSource,
}

impl SummaryItem {
    /// 要約を構成する 5 項目（`UISF-24`）。
    pub const ALL: [Self; 5] = [
        Self::Client,
        Self::Scope,
        Self::DeclaredEnvironment,
        Self::StartedAt,
        Self::StartSource,
    ];

    /// 項目の出所。組込み先が欄の分け方とラベルを決める根拠（`UISF-12`）。
    pub const fn provenance(self) -> Provenance {
        match self {
            Self::Client => Provenance::Registered,
            Self::Scope => Provenance::ServerRecorded,
            Self::DeclaredEnvironment => Provenance::AgentDeclared,
            Self::StartedAt | Self::StartSource => Provenance::ServerObserved,
        }
    }
}

/// 項目の出所。
///
/// 組込み先は未知の列挙子を「検証済み」側へ倒さず、`AgentDeclared` と同じ扱い
/// （別の欄・検証されていない表示）に倒す（fail-closed）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Provenance {
    /// クライアントの登録の値（`OCL-3`）。
    Registered,
    /// 要求に記録した値（`DGRT-3`）。
    ServerRecorded,
    /// サーバーが観測した値（`DGRT-4`）。
    ServerObserved,
    /// エージェントの申告で、検証されていない値（`UISF-12`・`ADLG-7`）。
    AgentDeclared,
}

/// エージェントが申告した環境。表示専用の自由文で、判定に使わない（`ADLG-7`）。
///
/// 攻撃者が選べる値なので、HTML へ出す前に必ずエスケープする（`UISF-2`）。
/// `Display`・`FromStr`・`From<&str>`・`Default`・`Deref` は意図して持たない。
#[derive(Clone)]
pub struct DeclaredEnvironment(String);

impl DeclaredEnvironment {
    /// 申告の値を、検査せずにそのまま保って作る。
    ///
    /// この関数は何も保証しない。制御文字・双方向制御・ゼロ幅・BOM の拒否（`DGRT-33`）と長さの
    /// 上限（`DGRT-21`）は行わない。**現状は未検査の値を受ける**: 検査は本体の `AgentEnv`
    /// （`TASK-412`・#3871）が行うが、まだ無い。検査の前に呼ぶと、検査していない値も要約に入る。
    /// 型は「検証されていない申告」であることを示すだけで、安全な文字だけを持つことを示さない。
    // TODO(#3871): 引数を `AgentEnv` の検査済みの型に替え、`String` の口をなくす。
    pub fn from_unverified_declaration(text: String) -> Self {
        Self(text)
    }

    /// エスケープ前の値を読む唯一の道。HTML へ出す前にエスケープする義務がある。
    // TODO(#5842): `SafeHtml` と結ぶ。
    pub fn as_unescaped_text(&self) -> &str {
        &self.0
    }
}

/// 値を出さない。自由文のログへの混入・改ざんを避ける（提案）。
impl std::fmt::Debug for DeclaredEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeclaredEnvironment(..)")
    }
}

/// サーバーが観測した開始の送信元（`DGRT-4`）。申告の型とは別で、相互の変換を持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartSource(IpAddr);

impl StartSource {
    /// 組込み先が渡す、観測した接続元からだけ作る。転送ヘッダから取らない（Hub `ARUN-29`・`ARUN-43`）。
    // TODO(#3868): 型の形は仮。開始の送信元を観測した接続元だけにする作業（`TASK-412` の 1/8。`DGRT-4`）で
    // 観測した接続元の型（`PeerInfo`〔仮称〕）が決まったら合わせる。要約への組み立ては #5841。
    pub fn from_observed_peer(addr: IpAddr) -> Self {
        Self(addr)
    }

    /// 観測した接続元のアドレス。
    pub fn addr(&self) -> IpAddr {
        self.0
    }
}

/// サーバーが記録した開始の時刻（UTC の Unix ミリ秒）。
// TODO(TASK-659): 時刻の受け渡しの型が決まったら置き換える。表示（ISO 8601）は組込み先。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartedAt(i64);

impl StartedAt {
    /// サーバーが記録した UTC のミリ秒から作る。
    pub fn from_unix_millis_utc(ms: i64) -> Self {
        Self(ms)
    }

    /// UTC の Unix ミリ秒。
    pub fn unix_millis_utc(&self) -> i64 {
        self.0
    }
}

/// 承認画面に出す要約。出力専用で、承認・拒否の能力を持たない（`UISF-14`）。
///
/// ユーザーコード・URL・`UISF-38` の項目は持たない（規則 7・10）。
#[derive(Debug, Clone)]
pub struct ApprovalSummary {
    client_name: String,
    client_id: String,
    scopes: Vec<String>,
    declared_environment: DeclaredEnvironment,
    started_at: StartedAt,
    start_source: StartSource,
}

impl ApprovalSummary {
    /// 骨組みのコンストラクタ。
    // TODO(#5841): 値の検査と、要求の記録（`DGRT-3`・`DGRT-4`）からの組み立てを足す。
    pub fn new(
        client_name: String,
        client_id: String,
        scopes: Vec<String>,
        declared_environment: DeclaredEnvironment,
        started_at: StartedAt,
        start_source: StartSource,
    ) -> Self {
        Self {
            client_name,
            client_id,
            scopes,
            declared_environment,
            started_at,
            start_source,
        }
    }

    /// クライアントの名前（エスケープ前。出すときはエスケープする）。
    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    /// クライアントの ID（エスケープ前）。
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// 要求した scope。
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// 申告した環境（検証されていない）。
    pub fn declared_environment(&self) -> &DeclaredEnvironment {
        &self.declared_environment
    }

    /// 開始の時刻。
    pub fn started_at(&self) -> StartedAt {
        self.started_at
    }

    /// 開始の送信元。
    pub fn start_source(&self) -> StartSource {
        self.start_source
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::net::Ipv4Addr;

    #[test]
    fn items_are_five_and_distinct() {
        // UISF-24: 項目は 5 件で互いに異なる。
        let set: HashSet<_> = SummaryItem::ALL.iter().collect();
        assert_eq!(SummaryItem::ALL.len(), 5);
        assert_eq!(set.len(), 5);
    }

    #[test]
    fn only_declared_environment_is_agent_declared() {
        // UISF-12: 申告の出所は申告した環境の 1 件だけ。時刻・送信元は観測。
        let declared: Vec<_> = SummaryItem::ALL
            .iter()
            .filter(|i| i.provenance() == Provenance::AgentDeclared)
            .collect();
        assert_eq!(declared, [&SummaryItem::DeclaredEnvironment]);
        assert_eq!(
            SummaryItem::StartedAt.provenance(),
            Provenance::ServerObserved
        );
        assert_eq!(
            SummaryItem::StartSource.provenance(),
            Provenance::ServerObserved
        );
    }

    #[test]
    fn debug_does_not_leak_declared_text() {
        // UISF-12・ADLG-7: Debug は申告の自由文を出さない。
        let marker = "sentinel-text-123";
        let d = DeclaredEnvironment::from_unverified_declaration(marker.to_string());
        assert!(!format!("{d:?}").contains(marker));
        assert!(!format!("{d:#?}").contains(marker));
        let s = ApprovalSummary::new(
            "n".to_string(),
            "i".to_string(),
            vec![],
            d,
            StartedAt::from_unix_millis_utc(1),
            StartSource::from_observed_peer(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        );
        assert!(!format!("{s:?}").contains(marker));
        assert!(!format!("{s:#?}").contains(marker));
    }
}
