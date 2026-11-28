//! 認証の画面の応答が満たすヘッダの要求値の型（仮称 `SecurityHeaders`）。
//!
//! 役割: 画面の応答に付けるヘッダの規則を、組込み先（Hub）が読める型で持つ。
//! 本体は画面を描画せず HTTP も受けないため、ヘッダの付与は行わない（`D-2026-10-30-1`・`A-2026-10-30-11`）。
//! 付与の実行は Hub の担当で、CSP・フレーム・補助ヘッダは `ASCR-3`・`ASCR-4`、
//! キャッシュのヘッダは `ARUN-48` で引く。Hub の値・本文は写さない。
//!
//! 根拠: `UISF-10`・`UISF-22`・`UISF-23`・`UISF-36`、
//! `CSRF-28`・`CSRF-29`・`CSRF-30`・`CSRF-31`・`CSRF-32`・`CSRF-51`。
//!
//! ステータス: `UISF-10`・`UISF-22`・`UISF-36` は検討中。`UISF-23` は確定だが、確定の範囲は
//! 旧構成の PoC-9 の範囲で、新しい構成では確かめていない（`Q-2026-10-30-4`）。ここで確定と読まない（ACPT-7）。
//! 型の名前は spec 上の仮称で、確定は人が行う。
//!
//! 検証を外す設定・feature は作らない（`CSRF-7`・`DPOL-30`）。型にも「付けない」「緩める」値を置かない。
//! 外からの文字列をヘッダの値に入れる口も置かない。値は列挙型の固定の語だけから作る。
//!
//! 現状は 1/6（#5804）の骨組みで、公開のコンストラクタは無い。必須の指令の検査は #5805、
//! wasm の画面の例外は #5806、フレーム・補助ヘッダの規則は #5807 で足す。
//! `CSRF-24` のキャッシュのヘッダは `ResponsePolicy`（仮称。`TASK-530`）が持ち、
//! `UISF-36` の確認で併せて引く扱いは #5807 で決める。

/// 画面がブラウザ側の WebAuthn の wasm を読むか。
///
/// 例外を足してよいのは wasm を読む画面だけ（`CSRF-29`・`UISF-23`・`UISF-36`）。判定は #5806 で作る。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScreenScripting {
    /// wasm を読まない画面。
    Standard,
    /// wasm を読む画面。`'wasm-unsafe-eval'` と `connect-src 'self'` を足す対象。
    LoadsWebAuthnWasm,
}

/// CSP の指令の名前（`CSRF-28`・`CSRF-29`・`CSRF-32`・`CSRF-51`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CspDirective {
    /// `default-src`
    DefaultSrc,
    /// `script-src`
    ScriptSrc,
    /// `style-src`
    StyleSrc,
    /// `connect-src`
    ConnectSrc,
    /// `form-action`
    FormAction,
    /// `frame-ancestors`
    FrameAncestors,
    /// `base-uri`
    BaseUri,
}

impl CspDirective {
    /// ヘッダに書く指令の名前。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DefaultSrc => "default-src",
            Self::ScriptSrc => "script-src",
            Self::StyleSrc => "style-src",
            Self::ConnectSrc => "connect-src",
            Self::FormAction => "form-action",
            Self::FrameAncestors => "frame-ancestors",
            Self::BaseUri => "base-uri",
        }
    }
}

/// CSP の指令に置ける値。外から任意の文字列を入れる口は無い。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CspSource {
    /// `'none'`
    None,
    /// `'self'`
    SelfOrigin,
    /// `'wasm-unsafe-eval'`。wasm を読む画面だけ（`CSRF-29`）。
    WasmUnsafeEval,
    /// `'unsafe-inline'`。`style-src` の暫定に限る（`CSRF-30`）。`script-src` には使わない。
    UnsafeInline,
}

impl CspSource {
    /// ヘッダに書く値の語。
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::None => "'none'",
            Self::SelfOrigin => "'self'",
            Self::WasmUnsafeEval => "'wasm-unsafe-eval'",
            Self::UnsafeInline => "'unsafe-inline'",
        }
    }
}

/// `style-src` の構成。組込み先が明示して選ぶ（`Default` は実装しない）。
///
/// `'unsafe-inline'` を外す時期は値の決定で人が行う（`UISF-10`・`CSRF-30` は検討中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StyleSrcMode {
    /// `'self'` だけ。`UISF-10` の厳格な構成。
    SelfOnly,
    /// `'self'` に `'unsafe-inline'` を足す暫定。builder の提供まで（`CSRF-30`）。
    InterimUnsafeInline,
}

/// CSP の指令と値の組の入れ物。必須の指令の組み立てと検査は #5805 で足す。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentSecurityPolicy {
    directives: Vec<(CspDirective, Vec<CspSource>)>,
}

impl ContentSecurityPolicy {
    /// 指令と値の組を順に返す。
    #[must_use]
    pub fn directives(&self) -> &[(CspDirective, Vec<CspSource>)] {
        &self.directives
    }
}

/// `X-Frame-Options` の値。`DENY` だけを表す（`CSRF-31`・`UISF-22`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameOptions {
    /// `DENY`
    Deny,
}

impl FrameOptions {
    /// ヘッダの名前。
    pub const HEADER_NAME: &'static str = "X-Frame-Options";

    /// ヘッダの値。
    #[must_use]
    pub const fn value(self) -> &'static str {
        match self {
            Self::Deny => "DENY",
        }
    }
}

/// `X-Content-Type-Options` の値。`nosniff` だけを表す（`CSRF-32`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContentTypeOptions {
    /// `nosniff`
    Nosniff,
}

impl ContentTypeOptions {
    /// ヘッダの名前。
    pub const HEADER_NAME: &'static str = "X-Content-Type-Options";

    /// ヘッダの値。
    #[must_use]
    pub const fn value(self) -> &'static str {
        match self {
            Self::Nosniff => "nosniff",
        }
    }
}

/// `Referrer-Policy` の値。`no-referrer` だけを表す（`CSRF-32`。`UISF-36` の招待のリンクの値の漏れに関わる）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReferrerPolicy {
    /// `no-referrer`
    NoReferrer,
}

impl ReferrerPolicy {
    /// ヘッダの名前。
    pub const HEADER_NAME: &'static str = "Referrer-Policy";

    /// ヘッダの値。
    #[must_use]
    pub const fn value(self) -> &'static str {
        match self {
            Self::NoReferrer => "no-referrer",
        }
    }
}

/// 認証の画面の応答が満たすヘッダの要求値（仮称）。
///
/// フィールドは private。検査を通らないと作れない形にするため、公開のコンストラクタは置かない。
/// 作る関数は #5805・#5806・#5807 で足す。付与の実行は Hub（`ASCR-3`・`ASCR-4`）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecurityHeaders {
    csp: ContentSecurityPolicy,
    frame_options: FrameOptions,
    content_type_options: ContentTypeOptions,
    referrer_policy: ReferrerPolicy,
}

impl SecurityHeaders {
    /// CSP の要求値。
    #[must_use]
    pub fn csp(&self) -> &ContentSecurityPolicy {
        &self.csp
    }

    /// `X-Frame-Options` の要求値。
    #[must_use]
    pub fn frame_options(&self) -> FrameOptions {
        self.frame_options
    }

    /// `X-Content-Type-Options` の要求値。
    #[must_use]
    pub fn content_type_options(&self) -> ContentTypeOptions {
        self.content_type_options
    }

    /// `Referrer-Policy` の要求値。
    #[must_use]
    pub fn referrer_policy(&self) -> ReferrerPolicy {
        self.referrer_policy
    }
}

// TODO(#5805): 必須の指令の組み立てと、欠けたときの検査（UISF-22）。SecurityHeaders を作る関数もここで足す。
// TODO(#5806): wasm を読む画面だけの例外（UISF-23・CSRF-29）。
// TODO(#5807): フレームのヘッダとその他のヘッダの規則（UISF-36）。
