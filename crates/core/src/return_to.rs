//! ログアウト・ログイン完了後の戻り先（`return_to`）の検査。
//!
//! 役割: 呼び出し元（Hub）が受け取った戻り先の候補を、同一オリジンのパスの型で振り分ける本体の判定
//! （`UISF-6`・`CSRF-33`・`CSRF-34`・`CSRF-35`、`LIB-1` の (k)、責務表の区分は「型・判定」`D-2026-11-21-7`）。
//! 型を満たす値は入力のバイト列のまま通し、満たさない値と値が無い場合は、呼び出し元が渡す既定の遷移先に落とす。
//!
//! 担わないもの: 303 と `Location` の出力、認可の途中の `/authorize/resume` への遷移は組込み先（Hub）が行う
//! （`D-2026-10-30-1`）。本体は「どのパスへ戻すか」の判定だけを返す。
//!
//! 呼び出し元への契約（`CSRF-35`）: 値の中で percent デコードしない。受け付けた値を再デコードして
//! `Location` を作ってはならない。[`ReturnTo::as_str`] の値をそのまま使う。
//!
//! 既定の遷移先は経路ごとに Hub が持つ値で、本体は本番の値を固定しない（`A-2026-10-28-1`〜`14`）。
//! 呼び出し元が [`ReturnTo::default_destination`] で型として作って渡す。
//!
//! ステータス: `UISF-6` の確定の範囲は旧構成の PoC-12 であり、新しい構成では確かめていない
//! （`D-2026-10-30-2`、`ACPT-7`）。
//!
//! 後続: 負の試験は #5823、契約テストは #5828、文書は #5829。

// TODO(TASK-533): 型（CSRF-33〜35）の置き場が決まったら `is_same_origin_path` を一本化する。

/// 同一オリジンのパスの型を満たした戻り先。
///
/// 外から直接作れない。値は入力のバイト列と完全に一致する（`CSRF-34`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReturnTo(String);

impl ReturnTo {
    /// 入力のバイト列をそのまま返す。デコードも正規化もしない（`CSRF-35`）。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 経路ごとの既定の遷移先を型として作る。型を満たさない値は `None`（既定の設定の誤りで止める）。
    #[must_use]
    pub fn default_destination(path: &str) -> Option<Self> {
        is_same_origin_path(path).then(|| Self(path.to_owned()))
    }
}

/// 判定の結果。既定に落ちたかどうかを組込み先が区別できるようにする。
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReturnToDecision {
    /// 候補が型を満たし、そのまま通った。
    Requested(ReturnTo),
    /// 候補が無い、または型を満たさず、既定の遷移先に落ちた。
    Default(ReturnTo),
}

impl ReturnToDecision {
    /// 戻す先。
    #[must_use]
    pub fn target(&self) -> &ReturnTo {
        match self {
            Self::Requested(t) | Self::Default(t) => t,
        }
    }
}

/// 戻り先の候補を検査する（`UISF-6`・`CSRF-33`）。失敗の種類は外に出さない。
#[must_use]
pub fn validate_return_to(candidate: Option<&str>, default: &ReturnTo) -> ReturnToDecision {
    match candidate {
        Some(c) if is_same_origin_path(c) => ReturnToDecision::Requested(ReturnTo(c.to_owned())),
        _ => ReturnToDecision::Default(default.clone()),
    }
}

/// `CSRF-33` の型: 先頭が `/`、2 バイト目が `/` でも `\` でもなく、`\` と制御文字（Cc）を含まない。
fn is_same_origin_path(s: &str) -> bool {
    let b = s.as_bytes();
    if b.first() != Some(&b'/') {
        return false;
    }
    if matches!(b.get(1), Some(b'/' | b'\\')) {
        return false;
    }
    !s.contains('\\') && !s.chars().any(char::is_control)
}
