//! 画面の応答のヘッダの要求値の語の試験（TASK-772 1/6）。
//! 根拠: CSRF-28・CSRF-29・CSRF-31・CSRF-32・UISF-10。

use fandhe_auth_core::security_headers::{
    ContentTypeOptions, CspDirective, CspSource, FrameOptions, ReferrerPolicy, ScreenScripting,
    StyleSrcMode,
};

#[test]
fn directive_names_match_requirements() {
    assert_eq!(CspDirective::ScriptSrc.name(), "script-src");
    assert_eq!(CspDirective::StyleSrc.name(), "style-src");
    assert_eq!(CspDirective::FormAction.name(), "form-action");
    assert_eq!(CspDirective::FrameAncestors.name(), "frame-ancestors");
    assert_eq!(CspDirective::DefaultSrc.name(), "default-src");
    assert_eq!(CspDirective::BaseUri.name(), "base-uri");
    assert_eq!(CspDirective::ConnectSrc.name(), "connect-src");
}

#[test]
fn source_tokens_match_requirements() {
    assert_eq!(CspSource::None.token(), "'none'");
    assert_eq!(CspSource::SelfOrigin.token(), "'self'");
    assert_eq!(CspSource::WasmUnsafeEval.token(), "'wasm-unsafe-eval'");
    assert_eq!(CspSource::UnsafeInline.token(), "'unsafe-inline'");
}

#[test]
fn auxiliary_header_values_are_fixed() {
    assert_eq!(FrameOptions::Deny.value(), "DENY");
    assert_eq!(FrameOptions::HEADER_NAME, "X-Frame-Options");
    assert_eq!(ContentTypeOptions::Nosniff.value(), "nosniff");
    assert_eq!(ReferrerPolicy::NoReferrer.value(), "no-referrer");
}

#[test]
fn style_mode_and_scripting_are_explicit_choices() {
    // UISF-10・CSRF-30: Default は実装しない。2 値が区別できることだけを確かめる。
    assert_ne!(StyleSrcMode::SelfOnly, StyleSrcMode::InterimUnsafeInline);
    assert_ne!(
        ScreenScripting::Standard,
        ScreenScripting::LoadsWebAuthnWasm
    );
}
