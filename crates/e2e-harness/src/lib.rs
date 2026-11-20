//! passkey の E2E ハーネスの枠を置く crate（TASK-11）。試験専用で、本番のビルドには入らない。
//!
//! 役割: passkey の登録と認証（`api-passkey.md`・PKEY）の 2 操作を、Chrome・Firefox の WebDriver の
//! 仮想 authenticator で通す E2E の、範囲・構成要素の起動と停止の入口・「仮想は実機に数えない」印の型を 1 か所に置く。
//! 呼び出し元は E2E の試験（試験ビルド）だけ。実体（ブラウザのドライバ・RP 側）は [`E2eComponent`] を実装して
//! 後続の段（2/6 以降）が置く。
//!
//! 範囲外: 画面の E2E と画面の識別子（Hub の `ASCR`。`LGUI-35` → `ASCR-65`）、適合性試験の実行環境（Hub の `ARUN-20`）、
//! Safari と実機（`TASK-784`・`TASK-75`〜`78`）、結合試験（`TASK-10`。it-harness）、計測と負荷（`TASK-12`）。
//!
//! 依存の向き: どの crate もこの crate に依存しない（LIB-2・LIB-4）。段階 1 のリリースのメンバーではない
//! （LIB-14・LIB-15）。名前・置き場は仮称で、人が確定する。外部依存は `serde_json` だけ（2026-11-20 の利用者の決定。
//! 試験専用の crate に限り直接依存として採用）。
//!
//! 安全: 検証を外す意味の分岐・feature は作らない（DPOL-30〜32）。誤りの値に秘密値・URL・パスを載せない
//! （SECR-11・SECR-23）。仮想 authenticator の合格を実機・Safari の合格に数えない（ACPT-7・ACPT-8）。
//!
//! 2/6 の実体: Chrome（chromedriver）の `chrome::ChromeDriverComponent` と、passkey の登録 1 件の手順
//! `registration::register_once`。chromedriver を新しいプロセスグループで起動して配下の Chrome ごと止めるため、
//! この 2 つは unix（Linux・macOS 等）に限る（`cfg(unix)`）。
//! 3/6 の実体: Firefox（geckodriver）の `firefox::GeckoDriverComponent`。起動・停止は Chrome と共通の
//! [`driver_process`] に切り出した（振る舞いは 2/6 のまま）。
//! RP 側の実体は `TASK-716` 待ちで置かない。構成の下書きは `docs/harness/e2e.md`。

#![forbid(unsafe_code)]

pub mod browser;
#[cfg(unix)]
pub mod chrome;
pub mod component;
#[cfg(unix)]
pub mod driver_process;
#[cfg(unix)]
pub mod firefox;
pub mod harness;
#[cfg(unix)]
pub mod registration;
pub mod scope;
pub mod virtual_authenticator;
pub mod webdriver;

pub use browser::{AuthenticatorKind, Browser, Evidence};
#[cfg(unix)]
pub use chrome::{ChromeDriverComponent, ChromeDriverConfig, ChromeHandle};
pub use component::{E2eComponent, E2eComponentKind};
#[cfg(unix)]
pub use driver_process::DriverHandle;
#[cfg(unix)]
pub use firefox::{FirefoxHandle, GeckoDriverComponent, GeckoDriverConfig};
pub use harness::{E2eConfig, E2eError, E2eErrorKind, E2eHarness, RunningE2e};
#[cfg(unix)]
pub use registration::{RegistrationErrorName, RegistrationOutcome, register_once};
pub use scope::PasskeyOperation;
pub use virtual_authenticator::{Protocol, Transport, VirtualAuthenticatorOptions};
pub use webdriver::{AuthenticatorId, CredentialSummary, WebDriverClient, WebDriverSession};
