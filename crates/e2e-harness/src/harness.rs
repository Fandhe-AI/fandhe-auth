//! 構成要素の組み立て・起動・停止の入口。
//!
//! 呼び出し元は E2E の試験。起動に失敗したら起動済みを逆順に停止してから誤りを返す（fail-closed の後始末）。
//! 環境変数・ファイルは読まない（DPOL-32）。

use std::fmt;

use crate::browser::Browser;
use crate::component::{E2eComponent, E2eComponentKind};
use crate::scope::PasskeyOperation;

/// 誤りの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum E2eErrorKind {
    /// 同じ種類の構成要素が 2 つ以上渡された。
    Duplicate,
    /// 起動に失敗した。
    StartFailed,
    /// 停止に失敗した。
    StopFailed,
    /// ドライバへの命令が失敗した（ドライバの誤り応答・応答の形の不一致・通信の失敗・上限超過）。
    CommandFailed,
    /// 起動前または停止後のドライバに操作した。
    NotRunning,
    /// 構成要素が対象とするブラウザが、構成のブラウザと一致しない。起動前に拒否する。
    BrowserMismatch,
}

/// ハーネスの誤り。種類と構成要素の種類だけを持ち、秘密値・URL・パス・ポートを載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct E2eError {
    kind: E2eErrorKind,
    component: E2eComponentKind,
}

impl E2eError {
    /// 誤りを作る。
    pub fn new(kind: E2eErrorKind, component: E2eComponentKind) -> Self {
        Self { kind, component }
    }

    /// 誤りの種類。
    pub fn kind(&self) -> E2eErrorKind {
        self.kind
    }

    /// 対象の構成要素の種類。
    pub fn component(&self) -> E2eComponentKind {
        self.component
    }
}

impl fmt::Display for E2eError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "e2e harness error: {:?} ({:?})",
            self.kind, self.component
        )
    }
}

impl std::error::Error for E2eError {}

/// E2E の構成。対象の操作・ブラウザ・構成要素だけを持つ。既定で構成要素を足さない。
pub struct E2eConfig {
    operation: PasskeyOperation,
    browser: Browser,
    components: Vec<Box<dyn E2eComponent>>,
}

impl E2eConfig {
    /// 対象の操作とブラウザを決めた空の構成。
    pub fn new(operation: PasskeyOperation, browser: Browser) -> Self {
        Self {
            operation,
            browser,
            components: Vec::new(),
        }
    }

    /// 構成要素を足す。順序は渡した順ではなく [`E2eComponentKind::START_ORDER`] で決まる。
    #[must_use]
    pub fn with(mut self, component: Box<dyn E2eComponent>) -> Self {
        self.components.push(component);
        self
    }
}

/// 起動の入口。
pub struct E2eHarness;

impl E2eHarness {
    /// 決まった順で起動する。重複は起動前に拒否し、途中で失敗したら起動済みを逆順に停止してから誤りを返す。
    pub fn start(config: E2eConfig) -> Result<RunningE2e, E2eError> {
        let E2eConfig {
            operation,
            browser,
            mut components,
        } = config;
        for (i, c) in components.iter().enumerate() {
            if components[..i].iter().any(|p| p.kind() == c.kind()) {
                return Err(E2eError::new(E2eErrorKind::Duplicate, c.kind()));
            }
        }
        // ドライバのブラウザと構成のブラウザが食い違う構成は起動前に拒否する（Chrome の構成に geckodriver 等）。
        for c in &components {
            // BrowserDriver は Some(config.browser) を必須とする（browser() 未実装の None を通さない）。
            // ブラウザに結び付かない要素だけが None を許す。
            let ok = match (c.kind(), c.browser()) {
                (E2eComponentKind::BrowserDriver, b) => b == Some(browser),
                (_, b) => b.is_none_or(|b| b == browser),
            };
            if !ok {
                return Err(E2eError::new(E2eErrorKind::BrowserMismatch, c.kind()));
            }
        }
        components.sort_by_key(|c| c.kind().rank());

        let mut running = RunningE2e {
            operation,
            browser,
            components: Vec::new(),
            stopped: false,
        };
        for mut c in components {
            if let Err(e) = c.start() {
                // 失敗した要素は起動済みに数えない。後始末の失敗より起動の誤りを返す。
                let _ = running.stop_all();
                return Err(e);
            }
            running.components.push(c);
        }
        Ok(running)
    }
}

/// 起動済みのハーネス。`Drop` でも停止を試みる（試験の panic で取り残さない）。
pub struct RunningE2e {
    operation: PasskeyOperation,
    browser: Browser,
    components: Vec<Box<dyn E2eComponent>>,
    stopped: bool,
}

impl RunningE2e {
    /// 対象の操作。
    pub fn operation(&self) -> PasskeyOperation {
        self.operation
    }

    /// 対象のブラウザ。
    pub fn browser(&self) -> Browser {
        self.browser
    }

    /// 起動している構成要素の種類（起動順）。
    pub fn kinds(&self) -> Vec<E2eComponentKind> {
        self.components.iter().map(|c| c.kind()).collect()
    }

    /// 逆順に停止する。最初の失敗を返しつつ、残りも停止を試みる。
    pub fn stop(mut self) -> Result<(), E2eError> {
        self.stop_all()
    }

    fn stop_all(&mut self) -> Result<(), E2eError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        let mut first = None;
        for c in self.components.iter_mut().rev() {
            if let Err(e) = c.stop() {
                first.get_or_insert(e);
            }
        }
        first.map_or(Ok(()), Err)
    }
}

impl Drop for RunningE2e {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}
