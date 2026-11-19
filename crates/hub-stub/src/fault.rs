//! Hub の stub の応答への遅れと拒否の注入（TASK-7・#66）。試験専用で、実物の Hub ではない。
//!
//! 役割: TASK-13（障害注入のハーネス）が「Hub の stub の応答の遅れ・拒否」を作るための注入点。
//! 呼び出し元は試験とハーネスで、次の 4 つの口を [`HubFaultInjector`] 越しに呼ぶ。
//! - Hub の JWKS の配信（[`crate::jwks::HubJwksEndpoint::serve`]）。OCL-36 の失敗の種類
//!   （接続の失敗・タイムアウト・解析の失敗）を作る。
//! - 失効の通知の受け口（[`crate::logout::LogoutTokenReceiver::receive`]）。Hub の停止を挟んだ再送（RVPG-28）を作る。
//! - 本人トークンの提示の受け口（[`crate::presentation::IdentityTokenReceiver::present`]）。
//! - 生存確認の受け口（[`crate::heartbeat::HeartbeatReceiver::receive`]）。受け取らないことで途絶を作る（Hub AUTHC-21）。
//!
//! 対象外の口: Hub が呼ぶ側の `assertion::AuthPort`・`feed::ChangeFeedPort`・`jwks::AuthJwksPort`。
//! これらの実装が返すのは `fandhe-auth` 側の応答で、Hub の stub の応答ではない。試験側の偽の口が拒否を
//! 既に返せる（`PortResponse`・`FeedResponse`・`AuthJwksResponse::Rejected`）。
//!
//! 設計上の約束:
//! - sans-IO・同期。stub は待たず、時計も読まない（DPOL-32）。遅れの `Duration` は [`Wait`] のフックへ
//!   渡すだけで、実際に待つか偽の時計を進めるかは試験とハーネス（TASK-13）が決める。遅れの値は試験用の
//!   仮の値で要件の値ではない。stub での測定を数値の根拠にしない（THRT-29・A-2026-10-28-1）。
//! - 拒否を注入したら、遅れがあれば先に [`Wait`] を呼び、内側の受け口を呼ばずに返す。内側の状態は変えない
//!   （jti を既受信にしない・最終受信時刻を進めない・JWKS のキャッシュと版を変えない）。拒否の後の再送が
//!   重複扱いにならずに受理されること（RVPG-28）と、途絶が作れることを保つ。
//! - 遅れだけの規則では、内側を呼んで結果を得てから [`Wait`] を呼び、結果をそのまま返す。
//! - 注入で成功を作る経路を持たない（AVLB-18・AVLB-22・DPOL-30 の向き）。`Status` の値は検査しないが、
//!   `Status(200)` を含めどの値も `Err` で返す。受理の判定は常に内側の既存の検証が行う。
//! - 呼び出しの回数は口ごとの通算（1 始まり）。`first == 0` は 1 と同じに扱い、範囲の検査はしない。
//!   規則は先頭から見て最初に当たった 1 件だけを使う。
//! - 記録（[`InjectionLog`]）は 4 つの口の呼び出しを呼んだ順に 1 本へ積む。注入なしは `injected: None` で、
//!   基準線（[`HubFaultConfig::baseline`]）も同じ形で流せる。持つのは種別・回数・時刻・遅れ・拒否の種類だけで、
//!   token・`sid`・`jti`・`instance`・本文は持たない（SECR-23〜27）。借りた値は内側へ渡すだけにする。
//! - 記録の `at` は各入口が受けた引数 `now` の写しで、ハーネスの時計との対応づけは TASK-13 の担当。
//!
//! 設定の置き場: 設定は `HubStubConfig` に足さず、この module の型で受ける。
//! `HubStubConfig` は既定値を持たず、全試験がリテラルで組み立てているため、欄を足すと全てに波及する。
//!
//! 書き手の読み（spec に定めが無い。実物との差は `docs/stubs/hub-stub.md` に書いた〔#69〕）:
//! - 拒否を HTTP の状態コード・接続の失敗で表すのは仮定である。
//! - `MalformedBody` の応答は状態 200・`max_age: 0`・`age: 0`・`from_cache: false` とし、版は内側の現在の版を写す。
//! - 「内側が適用した後で応答だけを失う」形は作らない。

use std::time::Duration;

use crate::heartbeat::{Heartbeat, HeartbeatReceiver};
use crate::jwks::{HubJwksEndpoint, JwksResponse};
use crate::logout::{LogoutReceipt, LogoutRejection, LogoutTokenReceiver};
use crate::presentation::{IdentityTokenReceiver, PresentationRejection, ReceivedIdentity};

/// `MalformedBody` で返す固定の本文。JSON として閉じていないので解析に失敗する。
const MALFORMED_BODY: &str = "{\"keys\":[";

/// 口ごとの通算の呼び出し回数（1 始まり、両端を含む）の範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallWindow {
    pub first: u32,
    /// `None` は以降すべて。
    pub last: Option<u32>,
}

impl CallWindow {
    /// `n` 回目の呼び出しが範囲に入るか。`first == 0` は 1 と同じに扱う。
    pub fn contains(&self, n: u32) -> bool {
        n >= self.first.max(1) && self.last.is_none_or(|l| n <= l)
    }
}

/// 1 つの規則。呼び出しの範囲に当たったら、遅れと拒否を与える。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultRule<R> {
    pub calls: CallWindow,
    pub delay: Option<Duration>,
    pub refusal: Option<R>,
}

/// 提示・失効の通知・生存確認の口の拒否。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Unreachable,
    Status(u16),
}

/// JWKS の配信の口の拒否。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwksRefusal {
    Unreachable,
    Status(u16),
    /// 状態 200 で、JSON として解析できない固定の本文を返す（OCL-36 の解析の失敗）。
    MalformedBody,
}

/// 4 つの口の注入の設定。既定値は持たず、基準線は [`HubFaultConfig::baseline`] で明示する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubFaultConfig {
    pub presentation: Vec<FaultRule<Refusal>>,
    pub logout: Vec<FaultRule<Refusal>>,
    pub heartbeat: Vec<FaultRule<Refusal>>,
    pub jwks: Vec<FaultRule<JwksRefusal>>,
}

impl HubFaultConfig {
    /// 注入なし（基準線）。
    pub fn baseline() -> Self {
        Self {
            presentation: Vec::new(),
            logout: Vec::new(),
            heartbeat: Vec::new(),
            jwks: Vec::new(),
        }
    }
}

/// 遅れの渡し先。stub は待たず、ここへ値を渡すだけにする。
pub trait Wait {
    fn wait(&mut self, delay: Duration);
}

impl<F: FnMut(Duration)> Wait for F {
    fn wait(&mut self, delay: Duration) {
        self(delay);
    }
}

/// 呼び出し元へ返す注入の失敗。検証による拒否とは型で区別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectedRefusal {
    Unreachable,
    Status(u16),
}

/// 呼び出しの種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubCallKind {
    Presentation,
    Logout,
    Heartbeat,
    Jwks,
}

/// 記録に残す拒否の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalRecord {
    Unreachable,
    Status(u16),
    MalformedBody,
}

impl From<Refusal> for RefusalRecord {
    fn from(r: Refusal) -> Self {
        match r {
            Refusal::Unreachable => Self::Unreachable,
            Refusal::Status(s) => Self::Status(s),
        }
    }
}

impl From<JwksRefusal> for RefusalRecord {
    fn from(r: JwksRefusal) -> Self {
        match r {
            JwksRefusal::Unreachable => Self::Unreachable,
            JwksRefusal::Status(s) => Self::Status(s),
            JwksRefusal::MalformedBody => Self::MalformedBody,
        }
    }
}

impl RefusalRecord {
    /// 呼び出し元へ返す形。`MalformedBody` は JWKS の口だけが扱い、他では起きない。
    /// 起きた場合も成功にせず、接続の失敗へ倒す。
    fn injected(self) -> InjectedRefusal {
        match self {
            Self::Status(s) => InjectedRefusal::Status(s),
            Self::Unreachable | Self::MalformedBody => InjectedRefusal::Unreachable,
        }
    }
}

/// 注入した内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectedFault {
    pub delay: Option<Duration>,
    pub refusal: Option<RefusalRecord>,
}

/// 1 回の呼び出しの記録。秘密値・識別子・本文は持たない（SECR-23〜27）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectionRecord {
    pub kind: HubCallKind,
    /// 口ごとの通算の回数（1 始まり）。
    pub call_index: u32,
    /// 入口が受けた `now` の写し。
    pub at: u64,
    /// 注入なしは `None`。
    pub injected: Option<InjectedFault>,
}

/// 呼んだ順の記録。外からは読むだけ。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InjectionLog {
    records: Vec<InjectionRecord>,
}

impl InjectionLog {
    pub fn records(&self) -> &[InjectionRecord] {
        &self.records
    }
}

type Plan = (Option<Duration>, Option<RefusalRecord>);

/// 規則の先頭から見て、`n` 回目に最初に当たった 1 件の (遅れ, 拒否)。
fn pick<R: Copy + Into<RefusalRecord>>(rules: &[FaultRule<R>], n: u32) -> Option<Plan> {
    rules
        .iter()
        .find(|r| r.calls.contains(n))
        .map(|r| (r.delay, r.refusal.map(Into::into)))
}

/// 4 つの口の注入器。内側の受け口は呼び出しごとに借りる。
pub struct HubFaultInjector<W: Wait> {
    faults: HubFaultConfig,
    wait: W,
    counts: [u32; 4],
    log: InjectionLog,
}

impl<W: Wait> std::fmt::Debug for HubFaultInjector<W> {
    /// 件数だけを出す（SECR-23〜27）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let injected = self
            .log
            .records
            .iter()
            .filter(|r| r.injected.is_some())
            .count();
        f.debug_struct("HubFaultInjector")
            .field("records", &self.log.records.len())
            .field("injected", &injected)
            .finish()
    }
}

impl<W: Wait> HubFaultInjector<W> {
    pub fn new(faults: HubFaultConfig, wait: W) -> Self {
        Self {
            faults,
            wait,
            counts: [0; 4],
            log: InjectionLog::default(),
        }
    }

    /// 設定を差し替える。呼び出しの回数は通算のまま戻さない。
    pub fn set_faults(&mut self, faults: HubFaultConfig) {
        self.faults = faults;
    }

    /// 呼んだ順の記録。
    pub fn log(&self) -> &InjectionLog {
        &self.log
    }

    /// 回数を進め、当たった規則を引き、記録を 1 件積む。
    fn begin(&mut self, kind: HubCallKind, now: u64) -> Plan {
        let slot = match kind {
            HubCallKind::Presentation => 0,
            HubCallKind::Logout => 1,
            HubCallKind::Heartbeat => 2,
            HubCallKind::Jwks => 3,
        };
        self.counts[slot] = self.counts[slot].saturating_add(1);
        let n = self.counts[slot];
        let hit = match kind {
            HubCallKind::Presentation => pick(&self.faults.presentation, n),
            HubCallKind::Logout => pick(&self.faults.logout, n),
            HubCallKind::Heartbeat => pick(&self.faults.heartbeat, n),
            HubCallKind::Jwks => pick(&self.faults.jwks, n),
        };
        let (delay, refusal) = hit.unwrap_or((None, None));
        self.log.records.push(InjectionRecord {
            kind,
            call_index: n,
            at: now,
            injected: hit.map(|_| InjectedFault { delay, refusal }),
        });
        (delay, refusal)
    }

    fn wait_for(&mut self, delay: Option<Duration>) {
        if let Some(d) = delay {
            self.wait.wait(d);
        }
    }

    /// 本人トークンの提示。拒否の注入は内側を呼ばず、内側の `received` を変えない。
    pub fn present(
        &mut self,
        inner: &mut IdentityTokenReceiver,
        token: &str,
        now: u64,
    ) -> Result<Result<ReceivedIdentity, PresentationRejection>, InjectedRefusal> {
        let (delay, refusal) = self.begin(HubCallKind::Presentation, now);
        if let Some(r) = refusal {
            self.wait_for(delay);
            return Err(r.injected());
        }
        let out = inner.present(token, now);
        self.wait_for(delay);
        Ok(out)
    }

    /// 失効の通知。拒否の注入は jti を既受信にしない（RVPG-28）。
    pub fn receive_logout(
        &mut self,
        inner: &mut LogoutTokenReceiver,
        token: &str,
        now: u64,
    ) -> Result<Result<LogoutReceipt, LogoutRejection>, InjectedRefusal> {
        let (delay, refusal) = self.begin(HubCallKind::Logout, now);
        if let Some(r) = refusal {
            self.wait_for(delay);
            return Err(r.injected());
        }
        let out = inner.receive(token, now);
        self.wait_for(delay);
        Ok(out)
    }

    /// 生存確認。拒否の注入は最終受信時刻を進めず、内側の記録も積まない（途絶を作る）。
    pub fn receive_heartbeat(
        &mut self,
        inner: &mut HeartbeatReceiver,
        heartbeat: &Heartbeat,
        now: u64,
    ) -> Result<(), InjectedRefusal> {
        let (delay, refusal) = self.begin(HubCallKind::Heartbeat, now);
        if let Some(r) = refusal {
            self.wait_for(delay);
            return Err(r.injected());
        }
        inner.receive(heartbeat, now);
        self.wait_for(delay);
        Ok(())
    }

    /// Hub の JWKS の配信。拒否の注入はキャッシュ・版・記録を変えない（OCL-36）。
    pub fn serve_jwks(
        &mut self,
        inner: &mut HubJwksEndpoint,
        now: u64,
    ) -> Result<JwksResponse, InjectedRefusal> {
        let (delay, refusal) = self.begin(HubCallKind::Jwks, now);
        match refusal {
            Some(RefusalRecord::MalformedBody) => {
                self.wait_for(delay);
                Ok(JwksResponse {
                    status: 200,
                    body: MALFORMED_BODY.to_owned(),
                    max_age: 0,
                    age: 0,
                    version: inner.current_version(),
                    from_cache: false,
                })
            }
            Some(r) => {
                self.wait_for(delay);
                Err(r.injected())
            }
            None => {
                let out = inner.serve(now);
                self.wait_for(delay);
                Ok(out)
            }
        }
    }
}
