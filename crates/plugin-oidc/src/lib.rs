//! 外部ログイン Plugin の OIDC 共通部品を置く同梱 Plugin の crate。
//!
//! 置き場の根拠は `LIB-14`（段階 1 の同梱 Plugin）と `PLG-26`。本体（`core`・`secret`・`db-tests`・
//! `client`）には含めず、`plugin-` 接頭辞のディレクトリで区別する。crate 名は仮称で、spec 上は未決
//! （暫定。人が確定する）。
//!
//! 依存の向き: Plugin から本体の公開 API へ向ける一方向だけ。本体からこの crate へ依存させない
//! （`PLG-26`・`LIB-2`）。
//!
//! 役割の区別: 自社 Provider と RP 向けの `fandhe-auth-client` とは別の役割・別の型・別の設定にする。
//! 外部 OIDC の検証設定は自社発行用と共有しない（`PLG-28`・`LIB-19`）。
//!
//! 外向きの通信は本体の外向き通信の口を通し、独自の HTTP クライアントを持たない（`PLG-22`・`HOST-9`）。
//! Google は共通部品の設定として扱い、専用の SDK に依存しない（`PLG-27`）。署名・暗号は検証済みの
//! 既存実装に任せ、自前で実装しない（`DPOL-47`・`PLG-8`）。

#![forbid(unsafe_code)]

// TODO(TASK-739): discovery・JWKS の取得・code_challenge の計算と Google の設定を実装する（PLG-26・PLG-27）。
// TODO(TASK-730): 認可コードフローの PKCE と nonce を実装する。
// TODO(TASK-731): ID トークンの検証（署名・iss・aud・azp・exp・iat）を実装する（PLG-8〜PLG-10）。
// TODO(TASK-714): 本体からこの crate への依存が 0 件であることの検査を置く（LIB-2）。
