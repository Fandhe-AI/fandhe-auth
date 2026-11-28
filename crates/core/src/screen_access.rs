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
//! 同名の Cookie が 2 つある要求は、組込み先が [`SessionState::NoCookie`] 相当
//! （セッション値なし）として渡す（`SESS-4`。fail-closed）。
//!
//! ステータス: `UISF-1` は確定の範囲つきで、根拠は旧構成の PoC の証拠である
//! （`Q-2026-10-30-4` の暫定の扱い）。期限切れの扱い（`SESS-21`）は検討中。
//!
//! 後続の sub-issue: 到達の条件の検査関数（#5787）、画面ごとの区分の一覧（#5788）、
//! 登録の導線の規則（#5789）、導線の列挙と走査（#5790）、負の試験（#5791）、
//! 契約テスト（#5792）、使い方の文書（#5793）。

/// 画面の要求時の認証セッションの状態（`UISF-1` の 5 状態）。
///
/// 状態の判別（Cookie の解析・DB の照会）は組込み先と別の TASK が行い、ここでは行わない。
/// 値は区分だけで、セッション値などの秘密値は持たない（`SECR-23`）。
///
/// 描画に至る状態は [`SessionState::Valid`] の 1 つだけである（`SESS-44`）。
/// 期限切れ（`SESS-21`）の扱いは検討中。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionState {
    /// 有効な認証セッション。
    Valid,
    /// Cookie がない（同名 Cookie が 2 つある要求を含む。`SESS-4`）。
    NoCookie,
    /// 失効している。
    Revoked,
    /// 期限切れ（扱いは `SESS-21` で検討中）。
    Expired,
    /// セッション値の形が不正。
    Malformed,
}

impl SessionState {
    /// 描画に進んでよい状態かを返す。`Valid` だけが `true`（`UISF-1`・`SESS-44`）。
    ///
    /// 判定の関数（#5787）は「その他は描画」の腕を書かず、この関数を起点に組む。
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

    // SESS-44: 有効でない 4 状態は描画に進めない。
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
}
