//! `fandhe-backend` の部品の差し替え点を 1 か所に集める境界。
//!
//! 対象は `docs/interim/README.md` の台帳の行 1〜6。この crate の他の module は、これらの部品をこの module の
//! 子 module からだけ参照し、`fandhe-backend` の該当部品を直接 `use` しない。差し替えで書き換える箇所を、
//! 下の子 module に限るためである。
//!
//! 暫定版と提供版を feature・環境変数・設定で実行時に切り替える経路は作らない（DPOL-30〜32・CSRF-7）。
//! 差し替えはコードの変更と版の固定だけで行い、前に再試験を流す（CG-6）。手順は #86、版の固定の記録の形式は
//! #85、空の実装での動作確認は #88 で作る。検査を通してしまう既定の実装は、本番のコードに持たない。
//!
//! 各子 module は枠だけで、中身は `TASK-523` ほか各 TASK が作る。シグネチャは中身の TASK で決める。

pub mod csrf_gate;
pub mod form_limit;
pub mod local_redirect;
pub mod no_store;
pub mod set_cookie;
pub mod trusted_proxy;
