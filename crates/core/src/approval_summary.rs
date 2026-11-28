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
//! 4. 申告した環境は表示専用の自由文で、長さの上限（設定で受ける。値は本体で決めない）を超える値と、
//!    表示を操作・隠す文字（制御文字・双方向制御・ゼロ幅等）を含む値は構築で拒否する
//!    （[`DeclaredEnvironment::from_agent_declaration`]。`UISF-12`・`LGUI-14`）。
//!    判定（承認の可否・scope・有効期限）の入力にしない
//!    （`ADLG-7`・`HAPR-1`）。判定の関数はこの型を引数に取らず、観測の型との相互の変換も持たない。
//! 5. 要約は出力専用で、承認・拒否の能力を持たない。確認の手順の判定は `DVUI-12`・`DVUI-13`・
//!    `DVUI-15`・`HAPR-3` が正本で、画面は結果を受けるだけ（`UISF-14`）。
//! 6. ユーザーコードを URL に含む案内の項目を持たない（`UISF-13`・`DGRT-2`）。ユーザーコード自体は
//!    低エントロピーの秘密値の扱いを受けるので（`SECR-7`・`SECR-34`）、骨組みでは要約に載せない。
//!    載せるかは人が決める（`TODO(#5841)`）。
//! 7. HTML へ出すときはエスケープする（`LGUI-14`・`UISF-2`）。`SafeHtml` との結合は
//!    `TODO(#5842)`。
//! 8. 観察（`UISF-37`）の実施は Hub の `ASCR-37` と人が行い、AI・自動処理に代えない
//!    （`D-2026-10-17-51` の (1)）。観察の仕様は `TODO(#5847)`。
//! 9. `UISF-38`（交換専用の scope を含む要求の別表示）は規則としてだけ置く。項目は持たない。
//!    トークン交換は MS-12 に延期されている（`D-2026-11-06-4`）ので、今実装するかは人に伺う
//!    （`TODO(#5841)`）。
//! 10. ステータス: `UISF-12`・`UISF-13`・`UISF-14`・`UISF-37`・`UISF-38` は検討中、`UISF-24` は確定の
//!     範囲つき。型を置いても、どの行も満たしたことにはならない（`ACPT-7`）。
//! 11. I/O・時刻・乱数を持たず、HTTP・Hub・DB の型を出さない（`DPOL-1`・`LIB-3`）。
//!
//! # 申告と観測の取り違えを型で拒む
//!
//! ```compile_fail
//! use fandhe_auth_core::approval_summary::{DeclaredEnvironment, DeclaredEnvironmentLimits, StartSource};
//! let l = DeclaredEnvironmentLimits::new(16);
//! let d = DeclaredEnvironment::from_agent_declaration("x".to_string(), l).unwrap();
//! let _ = StartSource::from(d);
//! ```
//!
//! ```compile_fail
//! use fandhe_auth_core::approval_summary::{DeclaredEnvironment, DeclaredEnvironmentLimits};
//! let l = DeclaredEnvironmentLimits::new(16);
//! let d = DeclaredEnvironment::from_agent_declaration("x".to_string(), l).unwrap();
//! let _ = format!("{}", d);
//! ```
//!
//! ```compile_fail
//! use fandhe_auth_core::approval_summary::DeclaredEnvironment;
//! let _ = DeclaredEnvironment::from("x");
//! ```
//!
//! ```compile_fail
//! use fandhe_auth_core::approval_summary::SummaryItem;
//! // `#[non_exhaustive]` のため、wildcard なしの網羅的な match は通らない。
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
//! 陽性対照（上の `compile_fail` が別の理由で落ちていないことの確認）:
//!
//! ```
//! use std::net::{IpAddr, Ipv4Addr};
//! use fandhe_auth_core::approval_summary::*;
//! let s = ApprovalSummary::new(
//!     "client".to_string(),
//!     "client-id".to_string(),
//!     vec!["read".to_string()],
//!     DeclaredEnvironment::from_agent_declaration(
//!         "env".to_string(),
//!         DeclaredEnvironmentLimits::new(16),
//!     )
//!     .unwrap(),
//!     StartedAt::from_unix_millis_utc(0),
//!     StartSource::from_observed_peer(IpAddr::V4(Ipv4Addr::LOCALHOST)),
//! );
//! assert_eq!(s.client_id(), "client-id");
//! assert_eq!(s.declared_environment().as_unescaped_text(), "env");
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

/// 申告した環境の検査の設定。上限の数値は設定で受け、本体は既定値を持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclaredEnvironmentLimits {
    max_chars: usize,
}

impl DeclaredEnvironmentLimits {
    /// 許す最大の文字数（Unicode のスカラー値の数）で作る。
    pub const fn new(max_chars: usize) -> Self {
        Self { max_chars }
    }
}

/// 申告した環境の検査の失敗。値そのものは載せない（文字の種類は載せる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeclaredEnvironmentError {
    /// 上限を超えている。
    TooLong,
    /// 表示を操作・隠す文字を含む。
    ForbiddenChar(char),
}

/// 表示の順序の操作・不可視・制御の文字か。
fn is_display_manipulating(c: char) -> bool {
    c.is_control() // C0・C1・DEL（改行・タブも含む）
        || matches!(
            c,
            '\u{00AD}' // soft hyphen
            | '\u{061C}' // ALM
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}' // ゼロ幅・LRM・RLM
            | '\u{2028}'..='\u{202E}' // 行・段落区切り・双方向制御
            | '\u{2060}'..='\u{206F}' // word joiner・不可視演算子・双方向 isolate
            | '\u{FE00}'..='\u{FE0F}' // variation selectors
            | '\u{FEFF}' // BOM・ZWNBSP
            | '\u{FFF9}'..='\u{FFFB}' // interlinear annotation
            | '\u{E0000}'..='\u{E007F}' // tag characters
        )
}

/// エージェントが申告した環境。表示専用の自由文で、判定に使わない（`ADLG-7`）。
///
/// 攻撃者が選べる値なので、HTML へ出す前に必ずエスケープする（`LGUI-14`・`UISF-2`）。
/// `Display`・`FromStr`・`From<&str>`・`Default`・`Deref` は意図して持たない。
#[derive(Clone)]
pub struct DeclaredEnvironment(String);

impl DeclaredEnvironment {
    /// エージェントの申告から作る唯一の道。
    ///
    /// 長さ（文字数）が `limits` の上限を超える値、表示を操作・隠す文字を含む値は拒否する。
    /// 上限の数値は呼び出し側が設定で渡す（本体は既定値を持たない）。
    pub fn from_agent_declaration(
        text: String,
        limits: DeclaredEnvironmentLimits,
    ) -> Result<Self, DeclaredEnvironmentError> {
        if let Some(c) = text.chars().find(|c| is_display_manipulating(*c)) {
            return Err(DeclaredEnvironmentError::ForbiddenChar(c));
        }
        if text.chars().count() > limits.max_chars {
            return Err(DeclaredEnvironmentError::TooLong);
        }
        Ok(Self(text))
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
    // TODO(#5841): 型の形は仮。観測した接続元の型（`PeerInfo`〔仮称〕）が決まったら合わせる。
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
/// ユーザーコード・URL・`UISF-38` の項目は持たない（規則 6・9）。
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

    fn limits() -> DeclaredEnvironmentLimits {
        DeclaredEnvironmentLimits::new(32)
    }

    #[test]
    fn declared_length_boundary() {
        // 上限ちょうどは受け、1 文字超過は拒否（文字数で数える）。
        let ok = "a".repeat(32);
        assert!(DeclaredEnvironment::from_agent_declaration(ok, limits()).is_ok());
        let over = "a".repeat(33);
        assert_eq!(
            DeclaredEnvironment::from_agent_declaration(over, limits()).unwrap_err(),
            DeclaredEnvironmentError::TooLong
        );
        let multibyte = "あ".repeat(32);
        assert!(DeclaredEnvironment::from_agent_declaration(multibyte, limits()).is_ok());
        let zero = DeclaredEnvironmentLimits::new(0);
        assert!(DeclaredEnvironment::from_agent_declaration(String::new(), zero).is_ok());
        assert!(DeclaredEnvironment::from_agent_declaration("a".into(), zero).is_err());
    }

    #[test]
    fn declared_rejects_display_manipulating_chars() {
        let forbidden = [
            '\u{0000}',
            '\u{0007}',
            '\u{000A}',
            '\u{001B}',
            '\u{007F}',
            '\u{0085}',
            '\u{009F}',
            '\u{00AD}',
            '\u{061C}',
            '\u{200B}',
            '\u{200C}',
            '\u{200D}',
            '\u{200E}',
            '\u{200F}',
            '\u{2028}',
            '\u{2029}',
            '\u{202A}',
            '\u{202B}',
            '\u{202C}',
            '\u{202D}',
            '\u{202E}',
            '\u{2060}',
            '\u{2066}',
            '\u{2067}',
            '\u{2068}',
            '\u{2069}',
            '\u{FEFF}',
            '\u{E0041}',
        ];
        for c in forbidden {
            let t = format!("a{c}b");
            assert_eq!(
                DeclaredEnvironment::from_agent_declaration(t, limits()).unwrap_err(),
                DeclaredEnvironmentError::ForbiddenChar(c),
                "U+{:04X}",
                c as u32
            );
        }
        // 通常の文字（空白・日本語・記号）は受ける。
        let ok = "Linux x86_64 日本語";
        assert!(DeclaredEnvironment::from_agent_declaration(ok.into(), limits()).is_ok());
    }

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
        let d = DeclaredEnvironment::from_agent_declaration(marker.to_string(), limits()).unwrap();
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
