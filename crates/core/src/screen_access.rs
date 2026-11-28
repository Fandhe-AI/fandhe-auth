//! 画面ごとの到達の規則の型（TASK-770 の 1/8。骨組みのみ）。
//!
//! 責務表の区分は「画面」（`D-2026-11-21-7`）。利用者ごとに変わる画面は、認証セッションが
//! 有効なときだけ描画に進み、他の状態はログインの誘導へ送る（`UISF-1`・`SESS-44`）。
//! 本モジュールは、その判定の意味を表す型だけを持つ。
//!
//! 画面の描画・稼働、状態コードと `Location` の出力、文言は組込み先（Hub の `ASCR`）が行う。
//! 本体は判定の意味だけを返す（`A-2026-10-30-11`・`D-2026-10-30-1`・`LIB-3`）。
//! HTTP の型・Hub の型・DB の型は出さず、時刻・乱数も読まない（`DPOL-1`）。
//!
//! 状態の判別（Cookie の解析・DB の照会。同名の Cookie が 2 つある要求を含む）は、
//! `SESS-4` の本体の部分（仮称 `SessionResolver::resolve`）で扱い、本モジュールでは行わない
//! （TODO(TASK-217): resolver の結果型との関係を決める）。応答の実行は Hub（`ARUN-36`）。
//! 同名の Cookie が 2 つある要求は、値が有効でも「セッション値なし」として扱う
//! （`SESS-4`。fail-closed）。
//!
//! ステータス: `UISF-1` は確定の範囲つきで、根拠は旧構成の PoC の証拠である
//! （`Q-2026-10-30-4` の暫定の扱い）。PoC の絶対期限切れは `SESS-4`（`UISF-1`）の
//! 確定の範囲に含む。24 時間・無操作の期限と新しい `sid` の経路は `SESS-21`（検討中）。
//!
//! 到達の条件の検査関数 [`ScreenAccessRule::decide`] は #5787（2/8）で実装済み。
//!
//! 後続の sub-issue: 画面ごとの区分の一覧（#5788）、
//! 登録の導線の規則（#5789）、導線の列挙と走査（#5790）、負の試験（#5791）、
//! 契約テスト（#5792）、使い方の文書（#5793）。

/// 画面の要求時の認証セッションの状態（`UISF-1` の 5 状態）。
///
/// 状態の判別（Cookie の解析・DB の照会）は `SESS-4` の本体の部分（仮称
/// `SessionResolver::resolve`）で扱い、ここでは行わない。
/// 値は区分だけで、セッション値などの秘密値は持たない（`SECR-23`）。
///
/// 描画に至る状態は [`SessionState::Valid`] の 1 つだけである（`SESS-44`）。
/// 期限切れのうち PoC の絶対期限切れは `SESS-4` の範囲で、24 時間・無操作の期限は
/// `SESS-21`（検討中）。
///
/// 認証セッションを DB から引けない間（DB に届かない間）の状態は、この 5 状態に丸めない。
/// その間の拒否の形は `AVLB-22`（検討中）が持つ（`SESS-44` の確定の範囲の外）。
/// TODO(TASK-551): 拒否の形を実装するときに、この型との関係を決める。
///
/// TODO(TASK-217): resolver の結果型と本型の関係（結果型にまとめるか、変換を 1 か所に限るか）を
/// 決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionState {
    /// 有効な認証セッション。
    Valid,
    /// Cookie がない（同名 Cookie が 2 つある要求を含む。`SESS-4`）。
    NoCookie,
    /// 失効している。
    Revoked,
    /// 期限切れ。PoC の絶対期限切れは `SESS-4` の範囲で、24 時間・無操作の期限は
    /// `SESS-21`（検討中）。
    Expired,
    /// セッション値の形が不正。
    Malformed,
}

impl SessionState {
    /// 描画に進んでよい状態かを返す。`Valid` だけが `true`（`UISF-1`・`SESS-44`）。
    ///
    /// 判定の関数 [`ScreenAccessRule::decide`] は「その他は描画」の腕を書かず、この関数を起点に組む。
    #[must_use]
    pub const fn is_valid(self) -> bool {
        matches!(self, Self::Valid)
    }
}

/// 画面ごとの到達の規則（型名は仮称）。
///
/// どの画面がどの規則かの一覧は持たない（TODO(#5788): 区分の一覧と根拠 ID）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScreenAccessRule {
    /// 利用者ごとに変わる画面。描画は有効な認証セッションのときだけ（`UISF-1`・`SESS-44`）。
    RequiresSession,
    /// 利用者ごとに変わらない画面。セッションの状態を描画の条件にしない。
    SessionNotRequired,
}

impl ScreenAccessRule {
    /// 規則と要求時の状態から、到達の判定の意味を返す（`UISF-1`・`SESS-44`）。
    ///
    /// - [`ScreenAccessRule::RequiresSession`]: [`SessionState::Valid`] のときだけ
    ///   [`AccessDecision::Render`]、他の 4 状態は [`AccessDecision::RedirectToLogin`]。
    /// - [`ScreenAccessRule::SessionNotRequired`]: 常に [`AccessDecision::Render`]。
    ///
    /// 同じ入力には同じ出力を返す純粋関数で、時刻・乱数・I/O を読まない（`DPOL-1`・`DPOL-3`）。
    /// 判定を外す引数・設定は持たない（`DPOL-28`）。`SessionState` の知らない列挙子は
    /// `is_valid()` が `false` になり、`RedirectToLogin` へ倒れる（fail-closed）。
    ///
    /// 返すのは判定の意味だけで、状態コード・`Location`・描画の実行は Hub が行う
    /// （`ARUN-36`・`ASCR`）。状態の判別（Cookie の解析・DB の照会）はこの関数の外（`SESS-4`。
    /// TODO(TASK-217)）。DB に届かない間は、この関数へ渡す前に 5 状態へ丸めない
    /// （`AVLB-22`。TODO(TASK-551)）。`Expired` の 24 時間・無操作の期限は `SESS-21`（検討中）で、
    /// 確定の範囲の外。
    ///
    /// `UISF-3` との関係（この段の範囲）: 返す値は `Render` と `RedirectToLogin` だけで、
    /// 登録の入口へ送る結果を持たない。登録の入口は招待の受け取りだけという規則は #5789、
    /// 他の導線が 0 件であることの列挙と走査は #5790、画面での確認は Hub の `ASCR-65` に残る。
    /// `UISF-3` は検討中である。
    #[must_use]
    pub const fn decide(self, state: SessionState) -> AccessDecision {
        match self {
            Self::RequiresSession => {
                if state.is_valid() {
                    AccessDecision::Render
                } else {
                    AccessDecision::RedirectToLogin
                }
            }
            Self::SessionNotRequired => AccessDecision::Render,
        }
    }
}

/// 到達の判定の意味（`UISF-1`）。
///
/// 状態コード・経路の文字列は持たない。出力の実行は組込み先（Hub の `ASCR`）で、
/// 経路名の最終は持ち越し（`D-2026-10-17-54`）。
///
/// 組込み先は、知らない列挙子（`non_exhaustive` の wildcard の腕）を描画しない側へ倒す
/// （fail-closed）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccessDecision {
    /// 本人の画面を描画してよい。
    Render,
    /// ログインの誘導（既定の遷移先）へ送る。
    RedirectToLogin,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NON_VALID: [SessionState; 4] = [
        SessionState::NoCookie,
        SessionState::Revoked,
        SessionState::Expired,
        SessionState::Malformed,
    ];

    // UISF-1: 描画に至る状態は Valid の 1 つだけ。
    #[test]
    fn uisf_1_only_valid_is_valid() {
        assert!(SessionState::Valid.is_valid());
    }

    // UISF-1・SESS-4（SESS-44）: Expired を含む有効でない 4 状態は描画に進めない。
    #[test]
    fn sess_44_non_valid_states_are_not_valid() {
        for s in NON_VALID {
            assert!(!s.is_valid(), "{s:?}");
        }
    }

    // UISF-1: 規則・判定の意味の列挙子が区別できる。
    #[test]
    fn uisf_1_variants_are_distinct() {
        assert_ne!(
            ScreenAccessRule::RequiresSession,
            ScreenAccessRule::SessionNotRequired
        );
        assert_ne!(AccessDecision::Render, AccessDecision::RedirectToLogin);
    }

    const ALL_STATES: [SessionState; 5] = [
        SessionState::Valid,
        SessionState::NoCookie,
        SessionState::Revoked,
        SessionState::Expired,
        SessionState::Malformed,
    ];

    const ALL_RULES: [ScreenAccessRule; 2] = [
        ScreenAccessRule::RequiresSession,
        ScreenAccessRule::SessionNotRequired,
    ];

    // UISF-1: 認証の要る画面は Valid のときだけ描画する。
    #[test]
    fn uisf_1_requires_session_renders_only_when_valid() {
        assert_eq!(
            ScreenAccessRule::RequiresSession.decide(SessionState::Valid),
            AccessDecision::Render
        );
    }

    // SESS-44: 有効でない 4 状態の到達は拒否してログインの誘導へ送る。
    #[test]
    fn sess_44_requires_session_redirects_non_valid_states() {
        for s in NON_VALID {
            assert_eq!(
                ScreenAccessRule::RequiresSession.decide(s),
                AccessDecision::RedirectToLogin,
                "{s:?}"
            );
        }
    }

    // UISF-1: セッション不要の画面は状態を条件にしない。
    #[test]
    fn uisf_1_session_not_required_renders_in_all_states() {
        for s in ALL_STATES {
            assert_eq!(
                ScreenAccessRule::SessionNotRequired.decide(s),
                AccessDecision::Render,
                "{s:?}"
            );
        }
    }

    // UISF-1: 2 規則 x 5 状態の判定表が期待と全件一致する。
    #[test]
    fn uisf_1_decision_table_is_fixed() {
        use AccessDecision::{RedirectToLogin, Render};
        use ScreenAccessRule::{RequiresSession, SessionNotRequired};
        use SessionState::{Expired, Malformed, NoCookie, Revoked, Valid};
        let table = [
            (RequiresSession, Valid, Render),
            (RequiresSession, NoCookie, RedirectToLogin),
            (RequiresSession, Revoked, RedirectToLogin),
            (RequiresSession, Expired, RedirectToLogin),
            (RequiresSession, Malformed, RedirectToLogin),
            (SessionNotRequired, Valid, Render),
            (SessionNotRequired, NoCookie, Render),
            (SessionNotRequired, Revoked, Render),
            (SessionNotRequired, Expired, Render),
            (SessionNotRequired, Malformed, Render),
        ];
        assert_eq!(table.len(), ALL_RULES.len() * ALL_STATES.len());
        for (rule, state, want) in table {
            assert_eq!(rule.decide(state), want, "{rule:?} {state:?}");
        }
    }

    // UISF-3（この段の範囲）: 返す結果は Render とログインの誘導だけで、登録の入口へ送る結果は出ない。
    // 登録の入口は招待の受け取りだけという規則は #5789、導線の走査は #5790、画面は Hub の ASCR-65。
    #[test]
    fn uisf_3_non_render_destination_is_login_only() {
        for rule in ALL_RULES {
            for s in ALL_STATES {
                let d = rule.decide(s);
                assert!(
                    matches!(d, AccessDecision::Render | AccessDecision::RedirectToLogin),
                    "{rule:?} {s:?}"
                );
            }
        }
    }

    // DPOL-3: 同じ入力は同じ出力。const の文脈でも評価できる（DPOL-1）。
    #[test]
    fn dpol_3_decide_is_deterministic() {
        const D: AccessDecision = ScreenAccessRule::RequiresSession.decide(SessionState::Expired);
        assert_eq!(D, AccessDecision::RedirectToLogin);
        for rule in ALL_RULES {
            for s in ALL_STATES {
                assert_eq!(rule.decide(s), rule.decide(s));
            }
        }
    }
}
