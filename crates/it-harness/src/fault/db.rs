//! DB の遮断と遅延の注入（TASK-13 の 2/8）。試験専用で、本番のビルドには入らない。
//!
//! 役割: PostgreSQL の前に置くループバックの TCP 転送器 [`DbFaultProxy`] で、DB への経路の遮断と
//! DB の応答の遅延を結合試験から注入する。注入と同時に 1/8 の [`FaultTimeline`] へ開始と解除を記録する。
//! 呼び出し元は結合試験（試験ビルド）だけ。呼び出し先は [`crate::database::EndpointHandle`]（上流のポートの
//! 取得）と [`super::record`]（記録）。Host（TASK-716）への結線はここに無く、[`DbFaultProxy::proxied_endpoint`]
//! の口を置くだけにする（TODO(TASK-716): Host の構築時に転送器の接続情報を渡す）。
//!
//! 方式: PoC-7 の手順 6 の向き（fandhe-auth と PostgreSQL の間の転送器が応答に遅延を足す・経路を遮断する）に
//! 合わせる。外部の crate は足さず `std::net` と `std::thread` だけで書く。
//!
//! 設計上の約束:
//! - 待ち受けは `127.0.0.1` だけ。上流も `127.0.0.1` 固定で、受けるのはポートだけ（任意のホストへ転送しない）。
//! - 転送は中身を読まない。TLS は素通しで、終端も復号もしない。中身・ポート・アドレスを記録・誤り・`Debug` に
//!   出さない（SECR-11・SECR-23・SECR-26）。
//! - 記録が成功してから状態を変える。「記録したのに注入していない」も「注入したのに記録が無い」も起こさない。
//!   基準線の `FaultTimeline` への注入は記録が拒否するので、注入もされない（fail-closed）。
//! - 環境変数・設定ファイル・feature で注入を切り替える経路を作らない（DPOL-30〜32）。
//! - 遮断は既存の接続を切り、以後に accept した接続を直ちに閉じる。待ち受けは遮断中も閉じない（ポートを持ち続ける）。
//!   accept・遮断の状態の確認・切断用の複製の登録は同じロックの下で行い、`block` の状態変更と切断も同じロックの下で行う（ORST-17）。
//!   解除しても切れた接続は戻らないので、client は解除の後につなぎ直す。遅延は応答（上流から client）の向きの塊ごとに足す。
//! - 接続の確立（上流への connect。上限 5 秒）は accept のスレッドで、ロックの外で同期に行う。上流は `127.0.0.1` で
//!   通常は即座に結果が出るが、固まると後続の accept がその間止まる（試験専用の制約として受け入れる）。
//! - 刻みの値（accept 5 ms・待ちの刻み 10 ms・上流への接続の上限 5 秒・遅延の上限）は開発の値で要件の値ではない
//!   （THRT-29）。
//!
//! 順序と境界の契約（#5983。詳細は `docs/harness/fault-injection.md`。ORST-17）:
//! - `block`: ロック → `Applied` を記録 → 状態を入れて切断 → ロックを外す。記録が拒否されたら状態を変えない（fail-closed）。
//!   `Applied` の時刻は実際の遮断の開始の下限で、`block` が戻った後は新しい塊を書かず、以後の accept は上流へ届けず切る。
//! - 上流への接続の開始の保証は「事前確認の時点」までで、確認の後（ポートの取得・`connect_timeout`）に `block` が
//!   完了しても上流への TCP の試行は起こり得る。ただし client のバイトは上流へ送らず切る。
//! - `release_block`: ロック → 待ち受けの待ち行列を上限まで排出して切る → `Released` を記録 → 解く。
//!   排出の後も、解除処理がロックを解放するまで accept のスレッドは進まない（ロックの解放の後に始まった接続は、使える上流があれば上流へ届く。
//!   `Ok(())` は遮断の解除で転送の保証ではなく、上流が無い・上流への接続に失敗したときは解除の後でも閉じる）。
//!   排出は `accept()` が `WouldBlock` を返した時点で終わるので、その後の解除までに待ち行列へ入った接続（解除と重なった接続）は解除の後に転送され得る（未定）。
//!   `Ok(())` で戻った場合に限り、解除の前に待ち行列にあった接続は上流へ届かない（排出の上限を超えた分は保証外。
//!   `docs/harness/fault-injection.md`）。`Err`（`DrainFailed`・記録の拒否）では遮断のままで、新規接続も切る。
//!   待ち行列の空を確かめられない accept の失敗（`WouldBlock` 以外。fd 枯渇等）では `Released` を記録せず遮断のまま
//!   `DrainFailed` で戻る（fail-closed）。呼び出しと重なった往復は未定で、試験の合否に使わない。
//! - 遅延は順序の契約の対象外（記録してから値を入れる。重なった塊は未定）。
//!
//! 名前・形・遮断の形（直ちに閉じる。受けて黙る形は入れていない）は仮置きで、人が確認する。

use std::fmt;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{FaultKind, FaultPhase, FaultRecordError, FaultTimeline};
use crate::database::{DatabaseEndpoint, EndpointHandle};

/// accept を試す間隔（開発の値）。
const ACCEPT_TICK: Duration = Duration::from_millis(5);
/// 遅延の待ちの刻み。停止と遮断をこの間隔で見る（開発の値）。
const WAIT_TICK: Duration = Duration::from_millis(10);
/// 上流への接続の上限（開発の値）。
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 解除のときに待ち受けの待ち行列から排出する接続の上限（開発の値。THRT-29）。Linux の `somaxconn` の
/// 既定値以上にして通常の待ち行列は空にし、再接続が続く状況でも解除が期限の中で戻るように件数で打ち切る。
const MAX_DRAIN_ON_RELEASE: usize = 4096;
/// 排出中の `Interrupted` で再試行する回数の上限（開発の値。THRT-29）。切断件数とは別に数え、割り込みが続いても打ち切る。
const MAX_DRAIN_INTERRUPTS: usize = 64;
/// 遅延の上限（開発の値。試験が固まるほどの遅延を拒否する）。
const MAX_DELAY_MS: u64 = 600_000;

/// 誤りの種類。ポート・アドレス・`io::Error` の文言・パラメータの値を載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbFaultError {
    /// 待ち受けの開始（bind・スレッド）に失敗した。
    StartFailed,
    /// すでに有効な注入を重ねて開始しようとした。
    AlreadyActive,
    /// 有効でない注入を解除しようとした。
    NotActive,
    /// 遅延の値の検査に落ちた（0・ms 未満・過大）。
    InvalidParam,
    /// 記録が拒否した（基準線である等）。状態は変えていない。
    Record(FaultRecordError),
    /// 解除のときに待ち行列の排出が accept の失敗（`WouldBlock` 以外）で終えられなかった。遮断のまま戻る。
    DrainFailed,
}

impl fmt::Display for DbFaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartFailed => f.write_str("db fault error: start failed"),
            Self::AlreadyActive => f.write_str("db fault error: already active"),
            Self::NotActive => f.write_str("db fault error: not active"),
            Self::InvalidParam => f.write_str("db fault error: invalid parameter"),
            Self::Record(e) => write!(f, "db fault error: {e}"),
            Self::DrainFailed => f.write_str("db fault error: drain failed"),
        }
    }
}

impl std::error::Error for DbFaultError {}

/// 転送の上流。宛先は常に `127.0.0.1` で、ポートだけを受ける。
#[derive(Debug, Clone)]
enum Upstream {
    Port(u16),
    Endpoint(EndpointHandle),
}

impl Upstream {
    /// 接続ごとに上流のポートを引く。`None` は起動前・停止後で、その接続は閉じる（fail-closed）。
    fn port(&self) -> Option<u16> {
        match self {
            Self::Port(p) => Some(*p),
            Self::Endpoint(h) => h.get().map(|e| e.port),
        }
    }
}

/// 上流への接続の直前に置く停止点（試験の注入用。ORST-17）。
/// 試験が「接続中で止まる」状態を同期で作るための同期点だけを持つ。宛先の決定と実際の接続は転送器が行い、
/// 転送器 1 つにつき 1 つ使う（複数の転送器へ複製して共有しない。共有すると、停止の時刻と停止フラグを同じロックの下で
/// 公開した保証が成り立たない〔`note_stop` が公開するのは渡された 1 つのフラグだけ〕。`stopped_at` は転送器ごとの停止の時刻で、
/// 再利用した場合は転送器へ渡した時刻より前の記録を無視して `Expired` と分類する）。
///
/// この値は接続先も接続の結果も決めない（ループバック・指定ポートの制約を外さない）。
/// 待ちは解放・転送器の停止・上限（[`UpstreamStall::MAX_HOLD`]）のどれかで必ず解ける（`stop()`・`Drop` を固めない）。
///
/// 転送器に渡すとき（`start_*_with_stall`）に、到達の記録（[`UpstreamStall::wait_entered`] の対象）と離れた理由
/// （[`UpstreamStall::exit_reason`]）を戻す。前の転送器の到達で、今回の `wait_entered` が即座に `true` にならない（ORST-17）。
/// 解放済み（[`UpstreamStall::release`]）の停止点は転送器に渡せず、起動は `StartFailed` になる（fail-closed）。
/// 古い解放の記録が新しい保持を `Released` と読ませないため、解放済みの値は再利用せず新しく作る。
/// 解放済みの停止点を通り抜ける接続は、到達にも離れた理由にも数えない（ORST-17）。
/// 離れた理由は直近の値ではなく、転送器へ渡してから一度でも期限切れがあれば `Expired` と読む
/// （先の期限切れを、後の保持の `Released`・`Stopped` で隠さない。ORST-17・AUDB-6）。
#[derive(Clone, Default)]
pub struct UpstreamStall {
    inner: Arc<StallInner>,
}

#[derive(Default)]
struct StallInner {
    state: Mutex<StallState>,
    cv: Condvar,
}

#[derive(Default)]
struct StallState {
    entered: u64,
    released: bool,
    /// `release` が呼ばれた時刻（同じロックの下で記録する。保持の期限までに解かれたかの判定に使う）。
    released_at: Option<Instant>,
    /// 転送器の停止が通知された時刻（`note_stop` が同じロックの下で記録する。期限までに停止で解かれたかの判定に使う）。
    stopped_at: Option<Instant>,
    /// 直近の `hold` の開始時刻。これより前の `stopped_at` は別の転送器の停止の名残として無視する。
    hold_started: Option<Instant>,
    /// 直近に転送器へ渡した（`rebind`）時刻。これより前の `stopped_at` は別の転送器の停止の名残として無視する。
    /// 渡した後から `hold` に入るまでの停止は、この転送器の停止として数える（`hold_started` だけでは `Expired` に誤る）。
    bound_at: Option<Instant>,
    /// 直近に停止点を離れた理由。試験が「期限切れで抜けたのを合格にしない」ために見る（ORST-17）。
    /// `exit_reason` は `expired` が正のとき、この値より `Expired` を優先する。
    last_exit: Option<StallExit>,
    /// 今の転送器へ渡してから、`Expired` で離れた保持の回数（通り抜けは数えない）。
    /// 先の期限切れを後の保持の `Released`・`Stopped` で上書きさせないための印（ORST-17・AUDB-6）。
    expired: u64,
}

/// 停止点を離れた理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StallExit {
    /// 試験が [`UpstreamStall::release`] で明示に解いた。
    Released,
    /// 転送器の停止の通知で解けた。
    Stopped,
    /// 上限（[`UpstreamStall::MAX_HOLD`]）の期限切れで解けた。
    Expired,
}

impl fmt::Debug for UpstreamStall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpstreamStall").finish_non_exhaustive()
    }
}

impl UpstreamStall {
    /// 停止点で待つ上限。解放されなくても転送器はこの時間で先へ進む。
    pub const MAX_HOLD: Duration = Duration::from_secs(5);

    /// 新しい停止点。
    pub fn new() -> Self {
        Self::default()
    }

    /// 転送器が停止点に入るのを待つ。入れば `true`、上限までに入らなければ `false`。
    /// 解放済みの停止点を通り抜ける接続は到達に数えない（ORST-17）。
    /// `timeout` は 24 時間に丸める（巨大な値で `Instant` の加算がパニックしない）。
    pub fn wait_entered(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout.min(Duration::from_secs(86_400));
        let mut g = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while g.entered == 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            g = self
                .inner
                .cv
                .wait_timeout(g, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    /// 直近に停止点を離れた理由。まだ離れていなければ `None`。
    /// ただし今の転送器へ渡してから一度でも期限切れがあれば、後の保持の理由にかかわらず `Expired`（ORST-17・AUDB-6）。
    /// 遮断・解除の試験は `Released`（明示の解除まで保持された）、停止の試験は `Stopped` を確かめる。
    /// `Expired` は期限切れで試験の前提が崩れたことを表し、合格にしない。
    pub fn exit_reason(&self) -> Option<StallExit> {
        let g = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if g.expired > 0 {
            return Some(StallExit::Expired);
        }
        g.last_exit
    }

    /// 今の転送器へ渡してから、期限切れで離れた保持の回数。
    pub fn expired_count(&self) -> u64 {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .expired
    }

    /// 停止点で待つ接続を先へ進める。以後に入る接続も待たない。
    pub fn release(&self) {
        let mut g = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !g.released {
            g.released = true;
            g.released_at = Some(Instant::now());
        }
        self.inner.cv.notify_all();
    }

    /// 転送器に渡す時に呼ぶ。解放済みなら `false`（渡せない）。そうでなければ、前の使用の到達・離れた理由・期限切れの印を戻し、
    /// 今回の到達だけを `wait_entered` が待てるようにして `true` を返す。
    fn rebind(&self) -> bool {
        let mut g = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if g.released {
            return false;
        }
        g.entered = 0;
        g.last_exit = None;
        g.expired = 0;
        g.bound_at = Some(Instant::now());
        true
    }

    /// 転送器の停止を通知する。停止の時刻の記録と停止フラグの公開を同じロックの下で行ってから待ちを起こす。
    /// 期限の後の停止を `Stopped` と読ませず、`hold` が `Stopped` を返したときは呼び出し側も必ずフラグを true と読む
    /// （ORST-17・AUDB-6）。ロックは入れ子にしない（ここで持つのはこの状態のロックだけ）。
    fn note_stop(&self, stop: &AtomicBool) {
        let mut g = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // 現在の保持より前の記録（再利用された別の転送器の停止）は上書きする。同じ停止の 2 回目は上書きしない。
        let stale = match (g.stopped_at, g.bound_at.or(g.hold_started)) {
            (None, _) => true,
            (Some(t), Some(h)) => t < h,
            (Some(_), None) => false,
        };
        if stale {
            g.stopped_at = Some(Instant::now());
        }
        stop.store(true, Ordering::SeqCst);
        self.inner.cv.notify_all();
    }

    /// 転送器の accept のスレッドが呼ぶ。解放・転送器の停止・上限のどれかで戻る。
    fn hold(&self, stop: &AtomicBool) {
        self.hold_for(stop, Self::MAX_HOLD);
    }

    /// [`Self::hold`] の本体。上限だけ引数にする（期限切れの分類を短い待ちで試験するため）。
    fn hold_for(&self, stop: &AtomicBool, max: Duration) {
        let started = Instant::now();
        let deadline = started + max;
        let mut g = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // 解除済みの停止点を通り抜けるだけの接続は、待っていない。到達にも離れた理由にも数えない
        // （先の接続の `Expired` を、後から来た通り抜けが `Released` で上書きしない。ORST-17・AUDB-6）。
        if g.released {
            return;
        }
        g.hold_started = Some(started);
        g.entered += 1;
        self.inner.cv.notify_all();
        while !g.released && !stop.load(Ordering::SeqCst) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            g = self
                .inner
                .cv
                .wait_timeout(g, left.min(Duration::from_millis(20)))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        // 転送器へ渡す前の停止の記録は、この保持を解いたものではない（渡した後・保持の開始前の停止は数える。
        // 渡していない値は保持の開始を境にする）。
        // 使い回した停止点に、生きている別の転送器の停止が後から記録され得る。自分の停止フラグが立っていないなら、
        // その記録はこの保持を解いたものではない（`note_stop` は記録とフラグを同じロックの下で公開するので、
        // 自分の停止なら、ロックを持つここでフラグは true に見える。ORST-17・AUDB-6）。
        let since = g.bound_at.unwrap_or(started);
        let stopped_at = g
            .stopped_at
            .filter(|t| *t >= since && stop.load(Ordering::SeqCst));
        let exit = classify_exit(g.released_at, stopped_at, deadline);
        if exit == StallExit::Expired {
            g.expired = g.expired.saturating_add(1);
        }
        g.last_exit = Some(exit);
    }
}

/// 停止点を離れた理由の決定。解除・停止のどちらも期限の後なら期限切れ（解除や停止が待ちを解いた、と読ませない）。
/// どちらも期限内なら、先に記録されたほうを理由にする（停止が先に待ちを解いた後に解除が割り込んでも `Stopped`。
/// 同時刻は順序を決められないので `Stopped` に倒す）。
fn classify_exit(
    released_at: Option<Instant>,
    stopped_at: Option<Instant>,
    deadline: Instant,
) -> StallExit {
    let released = released_at.filter(|t| *t <= deadline);
    let stopped = stopped_at.filter(|t| *t <= deadline);
    match (released, stopped) {
        (Some(r), Some(s)) if s <= r => StallExit::Stopped,
        (Some(_), _) => StallExit::Released,
        (None, Some(_)) => StallExit::Stopped,
        (None, None) => StallExit::Expired,
    }
}

/// 上流への接続の型（`TcpStream::connect_timeout` と同じ引数）。差し替えるのはこの 1 呼び出しだけ。
type ConnectFn = fn(&Shared, &SocketAddr, Duration) -> io::Result<TcpStream>;

/// 転送のスレッドと注入の入口で共有する状態。
struct Shared {
    /// 上流への接続の直前の停止点（試験用。無ければ待たない）。
    stall: Option<UpstreamStall>,
    /// `TcpStream::connect_timeout` の呼び出しそのもの。停止点が無ければ本物の接続、あれば停止点で待ってから
    /// 本物の `connect_timeout` を呼ぶ。計数・ロックまわりは `dial_upstream` の 1 か所で、本番と試験で共通（ORST-17）。
    connect: ConnectFn,
    stop: AtomicBool,
    blocked: AtomicBool,
    delay_ms: AtomicU64,
    /// 遅延の解除のたびに進める世代。処理中の待ちが古い遅延を引きずらないために見る。
    delay_gen: AtomicU64,
    /// 遮断のたびに進める世代。上流への接続の確立中に遮断が入ったかを見る。
    block_gen: AtomicU64,
    accepted: AtomicU64,
    refused: AtomicU64,
    /// 上流への connect を始めようとした回数（診断用。`connect_timeout` の直前（停止点の前）に加算するので到達の確認には使わず、成否も表さない）。
    upstream_attempts: AtomicU64,
    /// 上流への connect が成功した回数（`connect_timeout` が `Ok` を返した時点で数える）。
    upstream_connected: AtomicU64,
    /// 中継を始める段で、遮断の世代の比較が外れて拒否した回数。実際に拒否した箇所（`start_relay`）でだけ数え、
    /// 停止・現在の遮断だけによる拒否は数えない。
    generation_refused: AtomicU64,
    /// 両側を切るための複製（接続 ID と組）。両方向の転送が終わった接続の分は外す。
    /// accept・遮断の状態の確認・ここへの登録は、このロックの下で一度に行う。`block` の状態変更と切断も
    /// 同じロックの下で行うので、受理した接続が遮断から漏れない（ORST-17）。
    conns: Mutex<Vec<(u64, TcpStream)>>,
    next_conn_id: AtomicU64,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl Shared {
    #[cfg(test)]
    fn new() -> Self {
        Self::with_stall(None)
    }

    fn with_stall(stall: Option<UpstreamStall>) -> Self {
        let connect: ConnectFn = if stall.is_some() {
            connect_after_stall
        } else {
            connect_real
        };
        Self {
            stall,
            connect,
            stop: AtomicBool::new(false),
            blocked: AtomicBool::new(false),
            delay_ms: AtomicU64::new(0),
            delay_gen: AtomicU64::new(0),
            block_gen: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            refused: AtomicU64::new(0),
            upstream_attempts: AtomicU64::new(0),
            upstream_connected: AtomicU64::new(0),
            generation_refused: AtomicU64::new(0),
            conns: Mutex::new(Vec::new()),
            next_conn_id: AtomicU64::new(0),
            workers: Mutex::new(Vec::new()),
        }
    }

    fn sever_all(&self) {
        let mut g = self.conns.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, s) in g.drain(..) {
            let _ = s.shutdown(Shutdown::Both);
        }
    }

    /// 遮断の状態を入れて、登録済みの接続をすべて切る。`conns` のロックを持つ側が呼ぶ。
    fn engage_block(&self, conns: &mut Vec<(u64, TcpStream)>) {
        self.blocked.store(true, Ordering::SeqCst);
        self.block_gen.fetch_add(1, Ordering::SeqCst);
        for (_, s) in conns.drain(..) {
            let _ = s.shutdown(Shutdown::Both);
        }
    }

    /// 1 つの接続の両側を切って複製を外す（転送のスレッドを作れなかったときの片付け）。
    fn sever_conn(&self, id: u64) {
        let mut g = self.conns.lock().unwrap_or_else(PoisonError::into_inner);
        g.retain(|(i, s)| {
            if *i == id {
                let _ = s.shutdown(Shutdown::Both);
                false
            } else {
                true
            }
        });
    }

    /// 終わった接続の複製を外し、fd を `stop` まで抱えないようにする。
    fn forget_conn(&self, id: u64) {
        let mut g = self.conns.lock().unwrap_or_else(PoisonError::into_inner);
        g.retain(|(i, _)| *i != id);
    }

    fn halted(&self) -> bool {
        self.stop.load(Ordering::SeqCst) || self.blocked.load(Ordering::SeqCst)
    }
}

/// DB の前に置く障害注入の転送器。`Drop` と [`DbFaultProxy::stop`] でスレッドをすべて止めて待つ。
pub struct DbFaultProxy {
    shared: Arc<Shared>,
    upstream: Upstream,
    listen_port: u16,
    accept: Option<JoinHandle<()>>,
    /// 待ち受け。accept のスレッドと共有し、解除のときの待ち行列の排出に使う。
    /// 停止時に `take` して取り外す（accept のスレッドの複製と合わせて、中継の回収を待たずに待ち受けを閉じる）。
    listener: Option<Arc<TcpListener>>,
    block_active: bool,
    /// 遮断を記録するときの種類。`DbBlock` か `ExternalStoreBlock` だけ（4/8 が後者で包む）。
    block_kind: FaultKind,
    delay_active: Option<FaultKind>,
}

impl fmt::Debug for DbFaultProxy {
    /// ポートと接続の情報を出さない（SECR-11）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbFaultProxy")
            .field("block_active", &self.block_active)
            .field("delay_active", &self.delay_active.is_some())
            .finish_non_exhaustive()
    }
}

impl DbFaultProxy {
    /// 上流を `127.0.0.1:<port>` に固定して始める（試験用の入口）。
    pub fn start_to_port(port: u16) -> Result<Self, DbFaultError> {
        Self::start(Upstream::Port(port), FaultKind::DbBlock)
    }

    /// 遮断の記録の種類を指定して始める（`external_store` の包みが使う内部の口）。
    pub(super) fn start_to_port_as(port: u16, block_kind: FaultKind) -> Result<Self, DbFaultError> {
        Self::start(Upstream::Port(port), block_kind)
    }

    /// [`Self::start_to_port_as`] の `EndpointHandle` 版。
    pub(super) fn start_for_as(
        handle: EndpointHandle,
        block_kind: FaultKind,
    ) -> Result<Self, DbFaultError> {
        Self::start(Upstream::Endpoint(handle), block_kind)
    }

    /// [`crate::database::Database`] の接続情報のハンドルに向けて始める。Database の起動前に作れる。
    /// 接続のたびにポートを読み、値が無ければその接続を閉じる。
    pub fn start_for(handle: EndpointHandle) -> Result<Self, DbFaultError> {
        Self::start(Upstream::Endpoint(handle), FaultKind::DbBlock)
    }

    fn start(upstream: Upstream, block_kind: FaultKind) -> Result<Self, DbFaultError> {
        Self::start_with(upstream, block_kind, None)
    }

    /// 上流への接続の直前に停止点を置いて、上流をポート直指定で始める（`external_store` の包みが使う内部の口）。
    pub(super) fn start_to_port_with_stall_as(
        port: u16,
        block_kind: FaultKind,
        stall: UpstreamStall,
    ) -> Result<Self, DbFaultError> {
        Self::start_with(Upstream::Port(port), block_kind, Some(stall))
    }

    fn start_with(
        upstream: Upstream,
        block_kind: FaultKind,
        stall: Option<UpstreamStall>,
    ) -> Result<Self, DbFaultError> {
        // 遮断として記録できる種類だけ受ける（fail-closed）。
        if !matches!(
            block_kind,
            FaultKind::DbBlock | FaultKind::ExternalStoreBlock
        ) {
            return Err(DbFaultError::StartFailed);
        }
        if stall.as_ref().is_some_and(|s| !s.rebind()) {
            return Err(DbFaultError::StartFailed);
        }
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| DbFaultError::StartFailed)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| DbFaultError::StartFailed)?;
        let listen_port = listener
            .local_addr()
            .map_err(|_| DbFaultError::StartFailed)?
            .port();
        let shared = Arc::new(Shared::with_stall(stall));
        let listener = Arc::new(listener);
        let accept = {
            let shared = Arc::clone(&shared);
            let upstream = upstream.clone();
            let listener = Arc::clone(&listener);
            thread::Builder::new()
                .name("it-db-fault-accept".into())
                .spawn(move || accept_loop(&listener, &shared, &upstream))
                .map_err(|_| DbFaultError::StartFailed)?
        };
        Ok(Self {
            shared,
            upstream,
            listen_port,
            accept: Some(accept),
            listener: Some(listener),
            block_active: false,
            block_kind,
            delay_active: None,
        })
    }

    /// 待ち受けのポート（`127.0.0.1`）。
    pub fn listen_port(&self) -> u16 {
        self.listen_port
    }

    /// 上流の接続情報のポートを転送器のものに差し替えた複製（パスワードは秘密値の型のまま）。
    /// 上流がポート直指定のとき、または上流の値がまだ無いときは `None`。
    /// TODO(TASK-716): Host の構築時にこの値を渡す結線を置く。
    pub fn proxied_endpoint(&self) -> Option<DatabaseEndpoint> {
        match &self.upstream {
            Upstream::Port(_) => None,
            Upstream::Endpoint(h) => h.get().map(|mut e| {
                e.port = self.listen_port;
                e
            }),
        }
    }

    /// 上流へつないで転送を始めた接続の数。
    pub fn accepted_count(&self) -> u64 {
        self.shared.accepted.load(Ordering::SeqCst)
    }

    /// 診断の計数が数える処理の段階（受け付け順）。
    /// 1. accept した接続（[`Self::refused_count`] の対象は、この後に直ちに閉じたもの）。
    /// 2. 上流への connect の試行（[`Self::upstream_attempt_count`]。connect の呼び出しの直前、停止点の前に数える）。
    /// 3. 上流への connect の成功（[`Self::upstream_connected_count`]）。
    /// 4. 世代の比較。外れれば client のバイトを送らずに拒否（[`Self::generation_refused_count`]）。
    /// 5. 中継の開始（[`Self::accepted_count`]）。
    ///
    /// 段階 1 で終わった接続（遮断中・上流なし）は `refused_count` にだけ現れる。段階 2 で終わった接続（connect の失敗）は
    /// `refused_count` と試行の計数に現れ、成功の計数には現れない。段階 3 の後でソケットの複製に失敗した接続は、
    /// `refused_count`・試行・成功の計数に現れ、世代の拒否の計数には現れない。接続の成否は 2 と 3 の差で、中継前の拒否は 4 で分ける。
    ///
    /// 試行（2）と成功（3）の差を、確定した失敗の数として読めるのは、処理が終わった後（接続の処理が済み、
    /// 転送器が止まった後など）に限る。試行は停止点に入る前に加算され、計数器は別々に読むため、稼働中の差は
    /// 処理中の接続を含む参考値で、確定した失敗の数ではない（ORST-17）。
    ///
    /// 上流への connect の試行回数（診断用）。`connect_timeout` の呼び出しの直前（停止点の前）に加算するので、停止点への到達の確認には使えない
    /// （到達は [`UpstreamStall::wait_entered`] で待つ）。成否は表さない。
    pub fn upstream_attempt_count(&self) -> u64 {
        self.shared.upstream_attempts.load(Ordering::SeqCst)
    }

    /// 上流への connect が成功した回数。接続の失敗・停止点での打ち切りは数えない（試行の計数には入る）。
    /// 成功した後に世代の不一致で拒否された接続も数える（受理済みの [`Self::accepted_count`] とは別）。
    /// 転送器全体の数。接続を 1 本に限った試験でだけ、接続ごとの証拠になる。
    pub fn upstream_connected_count(&self) -> u64 {
        self.shared.upstream_connected.load(Ordering::SeqCst)
    }

    /// 中継を始める段で遮断の世代の比較が外れ、client のバイトを上流へ送らずに拒否した回数（ORST-17）。
    /// `start_relay` の世代の比較で見つけた世代の不一致を伴う拒否は、停止・遮断と重なっても数える（`halted` と世代の不一致の両方が真でも数える）。
    /// `connect_upstream` の接続前の確認で起きる世代の不一致、世代の不一致を伴わない停止・遮断による拒否、接続の失敗、ソケットの複製の失敗、遮断中の accept での拒否は数えない。
    /// それらは [`Self::refused_count`] に出て、到達した段階の計数にも残る（接続の失敗は
    /// [`Self::upstream_attempt_count`]、上流へ接続した後の複製の失敗は [`Self::upstream_connected_count`] にも出る）。
    /// 転送器全体の数。接続を 1 本に限った試験でだけ、接続ごとの証拠になる。
    pub fn generation_refused_count(&self) -> u64 {
        self.shared.generation_refused.load(Ordering::SeqCst)
    }

    /// 転送器が accept した後に直ちに閉じた接続の数（遮断中に accept して切ったもの・上流なし・上流への接続の失敗・
    /// 確立中の遮断・解除のときに待ち行列から排出して切ったもの）。遮断中でカーネルの待ち行列にあってまだ
    /// accept していない接続は、解除の排出までは数えない。
    pub fn refused_count(&self) -> u64 {
        self.shared.refused.load(Ordering::SeqCst)
    }

    /// DB への経路を遮断する。開いている接続を切り、以後に accept した接続は直ちに切る（待ち受けは閉じない）。
    pub fn block(&mut self, timeline: &mut FaultTimeline) -> Result<(), DbFaultError> {
        if self.block_active {
            return Err(DbFaultError::AlreadyActive);
        }
        // accept のスレッドは、accept と登録を `conns` のロックの下で行う。ここでも同じロックを取ってから
        // `Applied` を記録し、通ったら続けて状態を変える（記録の時刻と実際の遮断の間に待ちを挟まない）。
        let mut conns = self
            .shared
            .conns
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        timeline
            .record_now(self.block_kind, FaultPhase::Applied)
            .map_err(DbFaultError::Record)?;
        self.shared.engage_block(&mut conns);
        drop(conns);
        self.block_active = true;
        Ok(())
    }

    /// 遮断を解除する。切れた接続は戻らないので、client はつなぎ直す。
    /// ロックを取り、待ち受けの待ち行列に残る接続を切ってから `Released` を記録して遮断を解く。
    /// 記録が拒否されたら遮断のまま戻る（排出は遮断中の accept と同じ扱いで、状態の変化ではない。ORST-17）。
    pub fn release_block(&mut self, timeline: &mut FaultTimeline) -> Result<(), DbFaultError> {
        self.release_block_with(timeline, drain_backlog)
    }

    /// `release_block` の本体。排出を差し替えられる（排出の失敗が解除を止めることを試験で確かめる。ORST-17）。
    fn release_block_with(
        &mut self,
        timeline: &mut FaultTimeline,
        drain: impl FnOnce(&TcpListener, &Shared) -> Drain,
    ) -> Result<(), DbFaultError> {
        let kind = self.block_kind;
        self.release_block_inner(timeline, drain, move |tl, _| {
            tl.record_now(kind, FaultPhase::Released)
        })
    }

    /// 本体。`record` は `Released` を積む処理で、まだ遮断中の時点で呼ぶ。記録そのものを差し替えられるので、
    /// 記録を呼ぶ時点の遮断の状態を試験で観測できる（ORST-17）。
    fn release_block_inner(
        &mut self,
        timeline: &mut FaultTimeline,
        drain: impl FnOnce(&TcpListener, &Shared) -> Drain,
        record: impl FnOnce(&mut FaultTimeline, &Shared) -> Result<(), FaultRecordError>,
    ) -> Result<(), DbFaultError> {
        if !self.block_active {
            return Err(DbFaultError::NotActive);
        }
        let conns = self
            .shared
            .conns
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // ロックを持つ間は accept のスレッドが進めない。排出の後も、解除処理がロックを解放するまで進まない。
        // `WouldBlock` 以外の失敗は待ち行列の空を確かめられていない。`Released` を記録せず遮断を保つ（fail-closed）。
        // 割り込みの再試行を使い切った場合も空を確かめられていないので同じ扱い（件数の上限到達とは分ける）。
        // 停止後は待ち受けが無い。空を確かめられないので失敗として扱う（fail-closed）。
        let Some(listener) = self.listener.as_deref() else {
            return Err(DbFaultError::DrainFailed);
        };
        if matches!(
            drain(listener, &self.shared),
            Drain::Failed(_) | Drain::InterruptsExhausted(_)
        ) {
            return Err(DbFaultError::DrainFailed);
        }
        record(timeline, &self.shared).map_err(DbFaultError::Record)?;
        self.shared.blocked.store(false, Ordering::SeqCst);
        drop(conns);
        self.block_active = false;
        Ok(())
    }

    /// DB の応答（上流から client の向き）に、塊ごとに `delay` を足す。開いている接続にも効く。
    pub fn delay(
        &mut self,
        timeline: &mut FaultTimeline,
        delay: Duration,
    ) -> Result<(), DbFaultError> {
        let kind = FaultKind::db_delay(delay).map_err(|_| DbFaultError::InvalidParam)?;
        let FaultKind::DbDelay { delay_ms } = kind else {
            return Err(DbFaultError::InvalidParam);
        };
        if delay_ms > MAX_DELAY_MS {
            return Err(DbFaultError::InvalidParam);
        }
        if self.delay_active.is_some() {
            return Err(DbFaultError::AlreadyActive);
        }
        timeline
            .record_now(kind, FaultPhase::Applied)
            .map_err(DbFaultError::Record)?;
        // 適用では世代を進めない。二重適用は AlreadyActive で防がれ、直前は解除済み（delay_ms == 0）なので
        // 取り消す古い遅延が無い。ここで進めると、新しい値を読んだ待ちが中断され遅延が漏れる。
        self.shared.delay_ms.store(delay_ms, Ordering::SeqCst);
        self.delay_active = Some(kind);
        Ok(())
    }

    /// 遅延を解除する。`Applied` と同じ値で `Released` を記録する。
    pub fn release_delay(&mut self, timeline: &mut FaultTimeline) -> Result<(), DbFaultError> {
        let Some(kind) = self.delay_active else {
            return Err(DbFaultError::NotActive);
        };
        timeline
            .record_now(kind, FaultPhase::Released)
            .map_err(DbFaultError::Record)?;
        self.shared.delay_ms.store(0, Ordering::SeqCst);
        self.shared.delay_gen.fetch_add(1, Ordering::SeqCst);
        self.delay_active = None;
        Ok(())
    }

    /// 停止する（`Drop` と同じ）。解除していない注入があっても記録は触らない。数は `FaultTimeline::finish`
    /// の `unreleased_count` に出る。
    pub fn stop(mut self) {
        self.shutdown_all();
    }

    fn shutdown_all(&mut self) {
        // 停止点があれば、停止の時刻の記録と停止フラグの公開を `note_stop` が同じロックの下で行う。
        // 停止点が無くても、下の `store` が停止フラグを立てる。
        if let Some(stall) = &self.shared.stall {
            stall.note_stop(&self.shared.stop);
        }
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
        // accept のスレッドが終わった時点で、共有していた待ち受けを取り外す。中継の回収（join）を待たずに
        // 新規接続を受け付けなくなる（#5983）。
        self.listener.take();
        self.shared.sever_all();
        let workers: Vec<_> = {
            let mut g = self
                .shared
                .workers
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            g.drain(..).collect()
        };
        for w in workers {
            let _ = w.join();
        }
    }
}

impl Drop for DbFaultProxy {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

/// accept した接続と、その時点で `conns` に登録した切断用の複製。
struct Accepted {
    client: TcpStream,
    c_read: TcpStream,
    id: u64,
    /// accept した時点（ロックの下）の遮断の世代。
    block_gen: u64,
}

/// accept 1 回。accept・遮断の状態の確認・`conns` への登録を、同じロックの下で行う（ORST-17）。
/// 遮断中に accept した接続はここで直ちに切る（`refused_count` に数える）。待ち受けは遮断中も閉じない。
fn accept_one(listener: &TcpListener, shared: &Shared) -> Option<Accepted> {
    let mut conns = shared.conns.lock().unwrap_or_else(PoisonError::into_inner);
    let (client, _) = listener.accept().ok()?;
    // 受けた接続は blocking に戻す（listener の非 blocking を引き継ぐ環境がある）。
    if client.set_nonblocking(false).is_err() || shared.halted() {
        refuse(&client, shared);
        return None;
    }
    let (Ok(c_read), Ok(c_kill)) = (client.try_clone(), client.try_clone()) else {
        refuse(&client, shared);
        return None;
    };
    // 上流への接続の前から client を遮断の対象にする（確立中の遮断を取りこぼさない）。
    let id = shared.next_conn_id.fetch_add(1, Ordering::SeqCst);
    conns.push((id, c_kill));
    Some(Accepted {
        client,
        c_read,
        id,
        block_gen: shared.block_gen.load(Ordering::SeqCst),
    })
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>, upstream: &Upstream) {
    while !shared.stop.load(Ordering::SeqCst) {
        // ロックは `accept_one` の間だけ。上流への接続と中継はロックの外で行い、`block`・`release_block` を待たせない。
        match accept_one(listener, shared) {
            Some(a) => handle_conn(a, shared, upstream),
            None => thread::sleep(ACCEPT_TICK),
        }
    }
}

/// 待ち受けの待ち行列に残る接続を上限まで accept して直ちに切る。`conns` のロックを持つ側が呼ぶ（ORST-17）。
/// 遮断中に connect が終わったがまだ accept していない接続が、解除の後に上流へ届くのを防ぐ。
/// 切った件数は `refused_count` にも数える。
fn drain_backlog(listener: &TcpListener, shared: &Shared) -> Drain {
    drain_with(shared, MAX_DRAIN_ON_RELEASE, MAX_DRAIN_INTERRUPTS, || {
        listener.accept().map(|(c, _)| c)
    })
}

/// 排出の結果。値は切った件数。
#[derive(Debug, PartialEq, Eq)]
enum Drain {
    /// `WouldBlock` で待ち行列の空を確かめた。
    Done(usize),
    /// 件数の上限で打ち切った（上限を超えた分は保証外）。
    Truncated(usize),
    /// `Interrupted` の再試行を使い切った。空を確かめられておらず、失敗として扱う。
    InterruptsExhausted(usize),
    /// `WouldBlock`・`Interrupted` 以外の失敗で、空を確かめられなかった。
    Failed(usize),
}

fn drain_with(
    shared: &Shared,
    max: usize,
    max_interrupts: usize,
    mut next: impl FnMut() -> std::io::Result<TcpStream>,
) -> Drain {
    let (mut n, mut interrupts) = (0, 0);
    while n < max {
        match next() {
            Ok(client) => {
                refuse(&client, shared);
                n += 1;
            }
            Err(e) => match e.kind() {
                ErrorKind::WouldBlock => return Drain::Done(n),
                ErrorKind::Interrupted => {
                    interrupts += 1;
                    if interrupts > max_interrupts {
                        return Drain::InterruptsExhausted(n);
                    }
                }
                _ => return Drain::Failed(n),
            },
        }
    }
    Drain::Truncated(n)
}

fn refuse(client: &TcpStream, shared: &Shared) {
    shared.refused.fetch_add(1, Ordering::SeqCst);
    let _ = client.shutdown(Shutdown::Both);
}

/// 上流への接続を確立した段階の接続。中継はまだ始めていない。
struct Connected {
    accepted: Accepted,
    up: TcpStream,
    u_write: TcpStream,
    u_kill: TcpStream,
}

/// 登録済みの接続を、上流へつないで中継する。確立中に遮断が入った場合は、世代の差で見つけて切る。
fn handle_conn(accepted: Accepted, shared: &Arc<Shared>, upstream: &Upstream) {
    if let Some(connected) = connect_upstream(accepted, shared, upstream) {
        start_relay(connected, shared);
    }
}

/// 上流への接続の段。遮断済みなら上流へつながずに終える（ORST-17）。
/// 上流への接続を始めた後に遮断と重なった場合、TCP の接続の試行は起こり得るが、次の段の世代の確認で
/// client のバイトを送らずに切る（`docs/harness/fault-injection.md` の契約。ロックを持ったままつなぐと
/// `block` が最大 5 秒待たされるので、試行そのものは防がない）。
fn connect_upstream(accepted: Accepted, shared: &Shared, upstream: &Upstream) -> Option<Connected> {
    let id = accepted.id;
    if shared.halted() || shared.block_gen.load(Ordering::SeqCst) != accepted.block_gen {
        shared.sever_conn(id);
        refuse(&accepted.client, shared);
        return None;
    }
    let Some(port) = upstream.port() else {
        shared.sever_conn(id);
        refuse(&accepted.client, shared);
        return None;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    // 停止点は本物の接続と同じ呼び出し（`shared.connect`）の内側にある。この呼び出しを `conns` のロックの下で
    // 行う回帰が入ると、試験中の停止点の待ちも `block()` を待たせて試験が落ちる（ORST-17）。
    let Some(up) = dial_upstream(&addr, shared) else {
        shared.sever_conn(id);
        refuse(&accepted.client, shared);
        return None;
    };
    let (Ok(u_write), Ok(u_kill)) = (up.try_clone(), up.try_clone()) else {
        shared.sever_conn(id);
        refuse(&accepted.client, shared);
        return None;
    };
    Some(Connected {
        accepted,
        up,
        u_write,
        u_kill,
    })
}

/// 上流への接続の呼び出し口。計数と `shared.connect` の呼び出しはここ 1 か所で、本番と試験で同じコードを通る。
/// ここを `conns` のロックの下で呼ばない（試験の停止点は `shared.connect` の中にあるので、ロックで囲む回帰は
/// 試験中の待ちも囲み、`block()` が待たされて試験が落ちる。ORST-17）。
fn dial_upstream(addr: &SocketAddr, shared: &Shared) -> Option<TcpStream> {
    shared.upstream_attempts.fetch_add(1, Ordering::SeqCst);
    let up = (shared.connect)(shared, addr, UPSTREAM_CONNECT_TIMEOUT).ok()?;
    shared.upstream_connected.fetch_add(1, Ordering::SeqCst);
    Some(up)
}

/// 本番の接続。`TcpStream::connect_timeout` そのもの。
/// この 1 行だけをロックで囲む回帰は、全 OS の停止点の試験では検出できず、Linux の試験
/// `block_returns_promptly_while_real_upstream_connect_is_stalled_linux` が本物の connect を詰まらせて検出する。
/// ここで `conns` のロックを取らない。ロックで囲む回帰を試験が検出できる境界は、この関数の上の `dial_upstream` の
/// `shared.connect` の呼び出しで、停止点はその内側にある。この関数の中は停止点より下なので、囲んでも全 OS の停止点の試験では検出できない（ORST-17）。
fn connect_real(_: &Shared, addr: &SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    TcpStream::connect_timeout(addr, timeout)
}

/// 試験用の接続。停止点で待ってから本物の `connect_timeout` を呼ぶ。停止の通知で戻った場合は上流へつながず
/// `Err`（`stop()`・`Drop` を `connect_timeout` で待たせない）。
fn connect_after_stall(
    shared: &Shared,
    addr: &SocketAddr,
    timeout: Duration,
) -> io::Result<TcpStream> {
    if let Some(stall) = &shared.stall {
        stall.hold(&shared.stop);
        if shared.stop.load(Ordering::SeqCst) {
            // 再試行の意味を持つ `Interrupted` は使わない（`pump`・`drain_with` が「もう一度試す」の意味で使う）。
            // 停止の後に connect を呼ばない点は変えない。
            return Err(io::Error::other("stopped at the upstream stall point"));
        }
    }
    TcpStream::connect_timeout(addr, timeout)
}

/// 中継を始める段。上流側の複製を登録し、世代を確かめてから転送のスレッドを起こす。
/// 遮断が入っていれば client のバイトを 1 つも上流へ送らずに切る（ORST-17）。
fn start_relay(connected: Connected, shared: &Arc<Shared>) {
    let Connected {
        accepted:
            Accepted {
                client,
                c_read,
                id,
                block_gen,
            },
        up,
        u_write,
        u_kill,
    } = connected;
    shared
        .conns
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, u_kill));
    // 読む順は `halted()`、世代の順。逆にすると、間に `block()` と `release_block()` が両方終わった
    // ときに、世代は進んだのに遮断は解けた状態を見て、遮断の間に受理した接続を見逃す（ORST-17）。
    let halted = shared.halted();
    let gen_mismatch = shared.block_gen.load(Ordering::SeqCst) != block_gen;
    if halted || gen_mismatch {
        let _ = up.shutdown(Shutdown::Both);
        shared.sever_conn(id);
        // 世代の比較が外れたときだけ数える（世代の不一致が無い停止・遮断による拒否は数えない）。
        if gen_mismatch {
            shared.generation_refused.fetch_add(1, Ordering::SeqCst);
        }
        return refuse(&client, shared);
    }
    // 両方向の転送が終わった側が複製を外す（半閉じの間は遮断で切れるよう残す）。
    let remaining = Arc::new(AtomicUsize::new(2));
    let (r1, r2) = (Arc::clone(&remaining), remaining);
    // 登録の間に遮断が入った場合に取りこぼさない。
    // 読む順は `halted()`、世代の順。逆にすると、間に `block()` と `release_block()` が両方終わった
    // ときに、世代は進んだのに遮断は解けた状態を見て、遮断の間に受理した接続を見逃す（ORST-17）。
    let halted = shared.halted();
    let gen_mismatch = shared.block_gen.load(Ordering::SeqCst) != block_gen;
    if halted || gen_mismatch {
        shared.sever_conn(id);
        if gen_mismatch {
            shared.generation_refused.fetch_add(1, Ordering::SeqCst);
        }
        return refuse(&client, shared);
    }
    let s1 = Arc::clone(shared);
    let s2 = Arc::clone(shared);
    let w1 = thread::Builder::new()
        .name("it-db-fault-up".into())
        .spawn(move || {
            pump(c_read, u_write, false, &s1);
            finish_pump(&s1, id, &r1);
        });
    let w2 = thread::Builder::new()
        .name("it-db-fault-down".into())
        .spawn(move || {
            pump(up, client, true, &s2);
            finish_pump(&s2, id, &r2);
        });
    if w1.is_err() || w2.is_err() {
        // 片方向だけ動く接続を残さない。生きている側も両側を切って終わらせる。
        shared.sever_conn(id);
        shared.refused.fetch_add(1, Ordering::SeqCst);
    } else {
        shared.accepted.fetch_add(1, Ordering::SeqCst);
    }
    let mut g = shared
        .workers
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    g.retain(|h| !h.is_finished());
    g.extend([w1, w2].into_iter().flatten());
}

/// 転送が 1 方向終わるごとに呼ぶ。最後の 1 方向が終わったら接続の複製を外す。
fn finish_pump(shared: &Shared, id: u64, remaining: &AtomicUsize) {
    if remaining.fetch_sub(1, Ordering::SeqCst) == 1 {
        shared.forget_conn(id);
    }
}

/// `from` から読んで `to` へ書く。`delay` が真のとき（応答の向き）は書く前に現在の遅延だけ待つ。
fn pump(mut from: TcpStream, mut to: TcpStream, delay: bool, shared: &Shared) {
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => {
                let _ = to.shutdown(Shutdown::Write);
                return;
            }
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        if delay {
            wait_delay(shared);
        }
        // 遮断が入っていたら、この塊は書かずに捨てる。
        if shared.halted() || to.write_all(&buf[..n]).is_err() {
            break;
        }
    }
    let _ = from.shutdown(Shutdown::Both);
    let _ = to.shutdown(Shutdown::Both);
}

/// 現在の遅延だけ刻んで眠る。停止・遮断・遅延の解除（世代の変化）が入ったら抜ける。
fn wait_delay(shared: &Shared) {
    let generation = shared.delay_gen.load(Ordering::SeqCst);
    let ms = shared.delay_ms.load(Ordering::SeqCst);
    if ms == 0 {
        return;
    }
    let deadline = Instant::now() + Duration::from_millis(ms);
    // 解除で世代が変わったら古い待ちを直ちに終える。
    while !shared.halted() && shared.delay_gen.load(Ordering::SeqCst) == generation {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        thread::sleep(WAIT_TICK.min(deadline - now));
    }
}

#[cfg(test)]
mod tests {
    //! 順序の試験。窓がマイクロ秒で外から当てられないものを、内部の口で確かめる（ORST-17）。

    use std::io::{Read, Write};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::record::UtcTimestamp;

    fn timeline() -> FaultTimeline {
        FaultTimeline::start(
            Instant::now(),
            UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        )
        .unwrap()
    }

    fn echo_upstream() -> u16 {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || {
            for s in l.incoming().flatten() {
                thread::spawn(move || {
                    let mut s = s;
                    let mut b = [0u8; 64];
                    while let Ok(n) = s.read(&mut b) {
                        if n == 0 || s.write_all(&b[..n]).is_err() {
                            break;
                        }
                    }
                });
            }
        });
        port
    }

    /// 待ち受けの accept 待ち行列の長さが `want` 以上になるまで、期限付きで待つ（Linux の `/proc/net/tcp` の rx_queue）。
    /// 待ち行列の外から見える手段が他に無いため。アドレスの表記（エンディアン）に依らないよう、
    /// 状態が LISTEN（`0A`）でポートが一致する行だけを見る。`/proc/net/tcp` が読めなければ観測できないので panic する
    /// （合格に数えない。Linux 以外ではこの試験を `ignore` にしている）。
    fn wait_backlog(port: u16, want: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let suffix = format!(":{port:04X}");
        loop {
            let text = std::fs::read_to_string("/proc/net/tcp")
                .expect("cannot observe the accept queue: /proc/net/tcp is unreadable");
            let queued = text
                .lines()
                .skip(1)
                .filter_map(|l| {
                    let f: Vec<&str> = l.split_whitespace().collect();
                    if f.get(1)?.ends_with(&suffix) && *f.get(3)? == "0A" {
                        usize::from_str_radix(f.get(4)?.split(':').nth(1)?, 16).ok()
                    } else {
                        None
                    }
                })
                .max()
                .unwrap_or(0);
            if queued >= want {
                return;
            }
            assert!(Instant::now() < deadline, "backlog never reached {want}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn connect(port: u16) -> TcpStream {
        let s = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            Duration::from_secs(5),
        )
        .unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s
    }

    /// 1 往復。応答があれば `Some`、明示の切断（EOF・リセット等）なら `None`。タイムアウトは失敗にする。
    fn echo_or_cut(s: &mut TcpStream, msg: &[u8]) -> Option<Vec<u8>> {
        let cut = |k: ErrorKind| {
            matches!(
                k,
                ErrorKind::UnexpectedEof
                    | ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::BrokenPipe
            )
        };
        if let Err(e) = s.write_all(msg) {
            assert!(cut(e.kind()), "unexpected write error: {:?}", e.kind());
            return None;
        }
        let mut buf = vec![0u8; msg.len()];
        match s.read_exact(&mut buf) {
            Ok(()) => Some(buf),
            Err(e) => {
                assert!(cut(e.kind()), "unexpected read error: {:?}", e.kind());
                None
            }
        }
    }

    /// accept して登録した後に `block()` と解除が割り込んでも、その接続は切れて上流へ届かない（ORST-17）。
    /// 登録は accept と同じロックの下なので、`block` の切断が必ず当たる。世代は accept の時点のものを引き継ぐ。
    #[test]
    fn accepted_conn_is_cut_when_block_intervenes_before_relay() {
        let shared = Arc::new(Shared::new());
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = connect(port);
        let mut accepted = None;
        for _ in 0..500 {
            accepted = accept_one(&listener, &shared);
            if accepted.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        let accepted = accepted.expect("accepted");
        // accept と中継の開始の間に、遮断と解除が入る。
        {
            let mut conns = shared.conns.lock().unwrap();
            shared.engage_block(&mut conns);
        }
        assert_eq!(
            echo_or_cut(&mut client, b"x"),
            None,
            "registered conn is cut by the block"
        );
        shared.blocked.store(false, Ordering::SeqCst);
        handle_conn(accepted, &shared, &Upstream::Port(echo_upstream()));
        assert_eq!(shared.accepted.load(Ordering::SeqCst), 0);
    }

    /// 上流への接続を始める前に遮断済みの世代なら、上流へつながずに終える（ORST-17）。
    #[test]
    fn handle_conn_does_not_connect_upstream_when_already_blocked() {
        let shared = Arc::new(Shared::new());
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let _client = connect(port);
        let mut accepted = None;
        for _ in 0..500 {
            accepted = accept_one(&listener, &shared);
            if accepted.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        let accepted = accepted.expect("accepted");
        {
            let mut conns = shared.conns.lock().unwrap();
            shared.engage_block(&mut conns);
        }
        let up = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        up.set_nonblocking(true).unwrap();
        let up_port = up.local_addr().unwrap().port();
        handle_conn(accepted, &shared, &Upstream::Port(up_port));
        thread::sleep(Duration::from_millis(50));
        assert_eq!(
            up.accept().map(|_| ()).map_err(|e| e.kind()),
            Err(ErrorKind::WouldBlock),
            "upstream must not be contacted"
        );
        assert_eq!(shared.refused.load(Ordering::SeqCst), 1);
    }

    /// 遮断中に accept した接続は、`accept_one` が直ちに切る。
    #[test]
    fn conn_accepted_while_blocked_is_refused() {
        let shared = Arc::new(Shared::new());
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        {
            let mut conns = shared.conns.lock().unwrap();
            shared.engage_block(&mut conns);
        }
        let mut client = connect(port);
        for _ in 0..500 {
            if accept_one(&listener, &shared).is_none() && shared.refused.load(Ordering::SeqCst) > 0
            {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(shared.refused.load(Ordering::SeqCst), 1);
        assert_eq!(echo_or_cut(&mut client, b"y"), None);
    }

    fn nb_listener() -> (TcpListener, u16) {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    fn accept_blocking(l: &TcpListener, shared: &Shared) -> Accepted {
        for _ in 0..500 {
            if let Some(a) = accept_one(l, shared) {
                return a;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("not accepted");
    }

    /// 非ブロッキングの待ち受けから受理する。`wait` なら握手の完了まで期限（5 秒）付きで待つ。
    /// 期限は合否を決める時間ではなく、握手遅延で flaky にしないための上限。
    fn accept_within(l: &TcpListener, wait: bool) -> std::io::Result<TcpStream> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match l.accept() {
                Ok((c, _)) => return Ok(c),
                Err(e)
                    if wait && e.kind() == ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(2));
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// 解除の排出が待ち行列の接続を切る（ORST-17）。遮断中に connect が済み、まだ accept していない接続は、
    /// 排出しなければ解除の後に accept されて上流へ届く。ここでは排出の後に accept できる接続が無く、切断になる。
    /// 待ちはカーネルが握手を終えるためだけで、合否を時間で決めない。
    #[test]
    fn release_drains_backlog_queued_while_blocked() {
        let shared = Shared::new();
        let (l, port) = nb_listener();
        {
            let mut conns = shared.conns.lock().unwrap();
            shared.engage_block(&mut conns);
        }
        let mut client = connect(port);
        {
            let _conns = shared.conns.lock().unwrap();
            // 握手の完了は期限付きで待つ（固定の待ちに頼らない）。最初の 1 件だけ待ち、以後は即 WouldBlock。
            let mut waited = false;
            let drained = drain_with(&shared, MAX_DRAIN_ON_RELEASE, MAX_DRAIN_INTERRUPTS, || {
                let r = accept_within(&l, !waited);
                waited = true;
                r
            });
            assert_eq!(drained, Drain::Done(1));
            shared.blocked.store(false, Ordering::SeqCst);
        }
        assert!(accept_one(&l, &shared).is_none(), "nothing left to accept");
        assert_eq!(echo_or_cut(&mut client, b"q"), None);
        assert_eq!(shared.refused.load(Ordering::SeqCst), 1);
        assert_eq!(shared.accepted.load(Ordering::SeqCst), 0);
    }

    /// 排出は件数で打ち切る（開発の値。THRT-29）。空なら `Done`、上限で `Truncated`。
    #[test]
    fn drain_backlog_is_bounded() {
        let shared = Shared::new();
        let (l, port) = nb_listener();
        assert_eq!(drain_backlog(&l, &shared), Drain::Done(0));
        let _c1 = connect(port);
        let _c2 = connect(port);
        // 受理は期限付きで待つ。件数で打ち切るので、待つのは受理が必要な間だけ。
        let acc = || accept_within(&l, true);
        assert_eq!(drain_with(&shared, 1, 8, acc), Drain::Truncated(1));
        let acc = || accept_within(&l, true);
        assert_eq!(drain_with(&shared, 1, 8, acc), Drain::Truncated(1));
        assert_eq!(drain_backlog(&l, &shared), Drain::Done(0));
        assert_eq!(shared.refused.load(Ordering::SeqCst), 2);
    }

    /// Interrupted が続いても件数とは別の回数で打ち切る。`WouldBlock` 以外の失敗は `Failed`（ORST-17）。
    #[test]
    fn drain_distinguishes_interrupt_and_failure() {
        let shared = Shared::new();
        let err = |k: ErrorKind| move || Err::<TcpStream, _>(std::io::Error::from(k));
        assert_eq!(
            drain_with(&shared, 4096, 3, err(ErrorKind::Interrupted)),
            Drain::InterruptsExhausted(0)
        );
        assert_eq!(
            drain_with(&shared, 4096, 3, err(ErrorKind::WouldBlock)),
            Drain::Done(0)
        );
        assert_eq!(
            drain_with(&shared, 4096, 3, err(ErrorKind::Other)),
            Drain::Failed(0)
        );
    }

    /// 公開の `release_block` が排出を呼ぶ（ORST-17）。accept のスレッドを止めて待ち行列に接続を残し、
    /// 解除を通して切断数と拒否件数を確かめる。`drain_backlog` が呼ばれなければ `refused` は 0 のまま。
    #[test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "accept queue is observable only via /proc/net/tcp"
    )]
    fn release_block_drains_queued_conns() {
        let up = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let up_port = up.local_addr().unwrap().port();
        let mut p = DbFaultProxy::start_to_port(up_port).unwrap();
        let mut tl = FaultTimeline::start(
            Instant::now(),
            UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        )
        .unwrap();
        p.block(&mut tl).unwrap();
        // accept のスレッドを止める。以後の接続はカーネルの待ち行列に残る。
        p.shared.stop.store(true, Ordering::SeqCst);
        if let Some(h) = p.accept.take() {
            h.join().unwrap();
        }
        let mut c1 = connect(p.listen_port());
        let mut c2 = connect(p.listen_port());
        assert_eq!(p.refused_count(), 0);
        // 公開の `release_block` を直接呼ぶ。排出の呼び出しが抜ければ、待ち行列の 2 本は切られず拒否件数は 0 のまま。
        // connect の戻りは待ち行列への到達を保証しないので、2 本が届いたことを期限付きで確かめてから排出する。
        wait_backlog(p.listen_port(), 2);
        p.release_block(&mut tl).unwrap();
        assert_eq!(p.refused_count(), 2);
        assert_eq!(p.accepted_count(), 0);
        assert_eq!(echo_or_cut(&mut c1, b"a"), None);
        assert_eq!(echo_or_cut(&mut c2, b"b"), None);
        let _ = tl.finish(UtcTimestamp::from_unix_seconds(1_700_000_060));
    }

    /// 排出は成功しても `Released` の記録が拒否されたら、`Record` で戻り遮断を保つ。正しいタイムラインで解除を再試行できる（ORST-17）。
    #[test]
    fn release_block_record_rejected_keeps_block_and_allows_retry() {
        let up = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut p = DbFaultProxy::start_to_port(up.local_addr().unwrap().port()).unwrap();
        let mut tl = timeline();
        p.block(&mut tl).unwrap();
        let mut baseline = FaultTimeline::baseline(
            Instant::now(),
            UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        )
        .unwrap();
        let r = p.release_block(&mut baseline);
        assert!(matches!(r, Err(DbFaultError::Record(_))), "{r:?}");
        assert!(p.shared.blocked.load(Ordering::SeqCst), "still blocked");
        assert!(p.block_active);
        p.release_block(&mut tl).unwrap();
        assert!(!p.shared.blocked.load(Ordering::SeqCst));
        assert!(!p.block_active);
        let text = tl
            .finish(UtcTimestamp::from_unix_seconds(1_700_000_060))
            .unwrap()
            .to_string();
        assert!(text.contains(" released db_block"), "{text}");
    }

    /// 排出が失敗（割り込みの使い切り・`Failed`）なら `Released` を記録せず遮断のまま `DrainFailed` で戻る。
    /// 件数の上限到達（`Truncated`）は解除できる（ORST-17）。
    #[test]
    fn release_block_fails_closed_on_drain_failure() {
        for (drain, ok) in [
            (Drain::InterruptsExhausted(0), false),
            (Drain::Failed(0), false),
            (Drain::Truncated(4096), true),
        ] {
            let up = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let mut p = DbFaultProxy::start_to_port(up.local_addr().unwrap().port()).unwrap();
            let mut tl = FaultTimeline::start(
                Instant::now(),
                UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            )
            .unwrap();
            p.block(&mut tl).unwrap();
            let r = p.release_block_with(&mut tl, |_, _| drain);
            if ok {
                assert!(r.is_ok());
                assert!(!p.shared.blocked.load(Ordering::SeqCst));
            } else {
                assert_eq!(r, Err(DbFaultError::DrainFailed));
                assert!(p.shared.blocked.load(Ordering::SeqCst), "still blocked");
                assert!(p.block_active);
            }
            let text = tl
                .finish(UtcTimestamp::from_unix_seconds(1_700_000_060))
                .unwrap()
                .to_string();
            assert_eq!(text.contains(" released db_block"), ok, "{text}");
        }
    }

    /// 上流への接続の後・中継の開始の前に遮断が入っても、client のバイトは上流へ届かない（ORST-17）。
    /// 上流への TCP の接続の試行は起こり得る（契約）。上流の側は 0 バイトで EOF か切断になる。
    #[test]
    fn upstream_gets_no_client_bytes_when_block_overlaps_connect() {
        let shared = Arc::new(Shared::new());
        let (l, port) = nb_listener();
        let mut client = connect(port);
        client.write_all(b"payload").unwrap();
        let accepted = accept_blocking(&l, &shared);
        let up_l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let up_port = up_l.local_addr().unwrap().port();
        let connected = connect_upstream(accepted, &shared, &Upstream::Port(up_port))
            .expect("upstream connected before block");
        {
            let mut conns = shared.conns.lock().unwrap();
            shared.engage_block(&mut conns);
        }
        start_relay(connected, &shared);
        let (mut up_side, _) = up_l.accept().unwrap();
        up_side
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut buf = [0u8; 16];
        match up_side.read(&mut buf) {
            Ok(n) => assert_eq!(n, 0, "no client bytes reach upstream"),
            Err(e) => assert!(
                matches!(
                    e.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
                ),
                "unexpected: {:?}",
                e.kind()
            ),
        }
        assert_eq!(echo_or_cut(&mut client, b"x"), None);
        assert_eq!(shared.refused.load(Ordering::SeqCst), 1);
        assert_eq!(shared.accepted.load(Ordering::SeqCst), 0);
    }

    /// 上流への接続の後に遮断と解除の両方が済んでも（`blocked` は偽に戻る）、世代の差だけで中継を始めず切る（ORST-17）。
    /// 世代の比較を消すと `halted()` は偽なので中継が始まり、この試験は落ちる。
    #[test]
    fn stale_generation_is_refused_after_block_and_release() {
        let shared = Arc::new(Shared::new());
        let (l, port) = nb_listener();
        let mut client = connect(port);
        client.write_all(b"payload").unwrap();
        let accepted = accept_blocking(&l, &shared);
        let up_l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let up_port = up_l.local_addr().unwrap().port();
        let connected = connect_upstream(accepted, &shared, &Upstream::Port(up_port))
            .expect("upstream connected before block");
        {
            let mut conns = shared.conns.lock().unwrap();
            shared.engage_block(&mut conns);
        }
        shared.blocked.store(false, Ordering::SeqCst);
        assert!(!shared.halted(), "released: only the generation differs");
        start_relay(connected, &shared);
        let (mut up_side, _) = up_l.accept().unwrap();
        up_side
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut buf = [0u8; 16];
        match up_side.read(&mut buf) {
            Ok(n) => assert_eq!(n, 0, "no client bytes reach upstream"),
            Err(e) => assert!(
                matches!(
                    e.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
                ),
                "unexpected: {:?}",
                e.kind()
            ),
        }
        assert_eq!(echo_or_cut(&mut client, b"x"), None);
        assert_eq!(shared.refused.load(Ordering::SeqCst), 1);
        assert_eq!(shared.accepted.load(Ordering::SeqCst), 0);
    }

    /// `block()` は `Applied` を、`conns` のロックを取った後に記録する。ロックが他で持たれている間は
    /// 記録しない（記録の時刻と実際の遮断の間に待ちを挟まない）。
    #[test]
    fn block_records_applied_only_after_taking_the_lock() {
        let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
        let mut tl = timeline();
        let shared = Arc::clone(&p.shared);
        let held = shared.conns.lock().unwrap();
        thread::scope(|sc| {
            let h = sc.spawn(|| p.block(&mut tl).unwrap());
            thread::sleep(Duration::from_millis(300));
            drop(held);
            h.join().unwrap();
        });
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let text = tl
            .finish(UtcTimestamp::from_unix_seconds(now))
            .unwrap()
            .to_string();
        let offset: u64 = text
            .split("offset_ms=")
            .nth(1)
            .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|n| n.parse().ok())
            .expect("applied offset");
        assert!(
            offset >= 250,
            "Applied recorded before taking the lock: {offset} ms"
        );
    }

    /// `Released` の記録の時点では、まだ遮断中である。記録が拒否されたら遮断のまま戻る（ORST-17）。
    /// 状態変更と記録の順序が逆転すると、記録の直前に `blocked` が false になる。
    #[test]
    fn released_is_recorded_while_still_blocked() {
        let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
        let mut tl = timeline();
        p.block(&mut tl).unwrap();
        let kind = p.block_kind;
        let mut observed = None;
        let r = p.release_block_inner(
            &mut tl,
            |_, _| Drain::Done(0),
            |tl, sh| {
                // 記録を呼ぶ時点の遮断の状態を見て、そのまま本物の記録を積む。
                observed = Some(sh.blocked.load(Ordering::SeqCst));
                tl.record_now(kind, FaultPhase::Released)
            },
        );
        assert!(r.is_ok());
        assert_eq!(observed, Some(true), "must still be blocked at record time");
        assert!(!p.shared.blocked.load(Ordering::SeqCst));
    }

    /// 停止の時刻が見える時点で、停止フラグも必ず true と読める（同じロックの下で公開する。ORST-17）。
    /// 時刻の記録とフラグの公開が別々になると、`hold` が `Stopped` を返した後に `connect_after_stall` が false を読み得る。
    /// 競合の再現は確率的で、毎回は落ちない（スケジューラ依存）。多数回の繰り返しで検出の機会を増やす。
    #[test]
    fn stop_flag_is_visible_whenever_stop_time_is() {
        for _ in 0..2000 {
            let stall = UpstreamStall::new();
            let stop = Arc::new(AtomicBool::new(false));
            let (s2, stop2) = (stall.clone(), Arc::clone(&stop));
            let reader = thread::spawn(move || {
                // 期限切れは失敗（時刻が見えないまま 5 秒を過ぎたら false を返す）。
                let limit = Instant::now() + Duration::from_secs(5);
                while Instant::now() < limit {
                    let g = s2.inner.state.lock().unwrap();
                    if g.stopped_at.is_some() {
                        return stop2.load(Ordering::SeqCst);
                    }
                }
                false
            });
            stall.note_stop(&stop);
            assert!(reader.join().unwrap(), "stop time visible before the flag");
        }
    }

    /// 再利用された停止点の古い停止の記録は、新しい保持を解いたものと読まない（ORST-17・AUDB-6）。
    #[test]
    fn stale_stop_from_previous_use_is_expired() {
        let stall = UpstreamStall::new();
        // 別の転送器の停止（時刻と、立ったままのフラグ）が残っている状況。保持はすぐ戻る。
        stall.note_stop(&AtomicBool::new(false));
        thread::sleep(Duration::from_millis(5));
        let stop = AtomicBool::new(true);
        stall.hold(&stop);
        assert_eq!(stall.exit_reason(), Some(StallExit::Expired));
    }

    /// 転送器へ渡した後・保持の開始前の停止は `Stopped`（`Expired` に誤らない）。渡す前の別の転送器の停止は数えない
    /// （ORST-17。`connect_upstream` の停止の確認を通った直後に停止する競合の再現）。
    #[test]
    fn stop_between_bind_and_hold_is_stopped() {
        let stall = UpstreamStall::new();
        stall.note_stop(&AtomicBool::new(false));
        thread::sleep(Duration::from_millis(5));
        assert!(stall.rebind());
        thread::sleep(Duration::from_millis(5));
        let stop = AtomicBool::new(false);
        stall.note_stop(&stop);
        thread::sleep(Duration::from_millis(5));
        stall.hold(&stop);
        assert_eq!(stall.exit_reason(), Some(StallExit::Stopped));
    }

    /// 使い回した停止点で、生きている古い転送器が渡した後に止まっても、自分の停止フラグが false のまま期限切れなら
    /// `Stopped` にしない（合格の側への誤分類。ORST-17・AUDB-6）。
    #[test]
    fn stop_of_old_proxy_after_rebind_does_not_stop_new_hold() {
        let stall = UpstreamStall::new();
        assert!(stall.rebind());
        assert!(stall.rebind());
        stall.note_stop(&AtomicBool::new(false));
        stall.hold_for(&AtomicBool::new(false), Duration::from_millis(60));
        assert_eq!(stall.exit_reason(), Some(StallExit::Expired));
    }

    /// 期限切れの後に `release()` し、次の接続が通り抜けても、先の `Expired` は `Released` に上書きされない
    /// （ORST-17・AUDB-6。到達の数も通り抜けでは増えない）。
    #[test]
    fn pass_through_after_release_keeps_earlier_expired() {
        let stall = UpstreamStall::new();
        assert!(stall.rebind());
        stall.hold_for(&AtomicBool::new(false), Duration::from_millis(30));
        assert_eq!(stall.exit_reason(), Some(StallExit::Expired));
        stall.release();
        stall.hold_for(&AtomicBool::new(false), Duration::from_millis(30));
        assert_eq!(stall.exit_reason(), Some(StallExit::Expired));
        let entered = stall.inner.state.lock().unwrap().entered;
        assert_eq!(entered, 1, "pass-through must not count as an arrival");
    }

    /// 渡した後でも、渡す前の別の転送器の停止だけが残る場合は `Expired`。
    #[test]
    fn stop_before_bind_is_still_expired() {
        let stall = UpstreamStall::new();
        stall.note_stop(&AtomicBool::new(false));
        thread::sleep(Duration::from_millis(5));
        assert!(stall.rebind());
        thread::sleep(Duration::from_millis(5));
        stall.hold(&AtomicBool::new(true));
        assert_eq!(stall.exit_reason(), Some(StallExit::Expired));
    }

    /// 保持の開始後の停止は、古い記録があっても `Stopped` になる。
    #[test]
    fn fresh_stop_after_stale_one_is_stopped() {
        let stall = UpstreamStall::new();
        stall.note_stop(&AtomicBool::new(false));
        thread::sleep(Duration::from_millis(5));
        let stop = Arc::new(AtomicBool::new(false));
        let (s2, stop2) = (stall.clone(), Arc::clone(&stop));
        let h = thread::spawn(move || s2.hold(&stop2));
        assert!(stall.wait_entered(Duration::from_secs(5)));
        stall.note_stop(&stop);
        h.join().unwrap();
        assert_eq!(stall.exit_reason(), Some(StallExit::Stopped));
    }

    /// 期限の後の解除・停止は `Released`・`Stopped` にならない（ORST-17・AUDB-6）。
    #[test]
    fn classify_exit_distinguishes_late_release_and_stop() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let early = Some(deadline - Duration::from_millis(10));
        let late = Some(deadline + Duration::from_millis(10));
        let c = |r, s| classify_exit(r, s, deadline);
        assert_eq!(c(early, None), StallExit::Released);
        assert_eq!(c(late, None), StallExit::Expired);
        assert_eq!(c(None, None), StallExit::Expired);
        assert_eq!(c(None, early), StallExit::Stopped);
        // 停止が期限の後に立った場合は、待機から戻っても期限切れ。
        assert_eq!(c(None, late), StallExit::Expired);
        assert_eq!(c(late, late), StallExit::Expired);
        assert_eq!(c(late, early), StallExit::Stopped);
        // 両方が期限内なら、先に記録されたほうが理由になる。
        let first = Some(deadline - Duration::from_millis(20));
        let second = Some(deadline - Duration::from_millis(10));
        assert_eq!(c(second, first), StallExit::Stopped, "stop came first");
        assert_eq!(c(first, second), StallExit::Released, "release came first");
        assert_eq!(c(first, first), StallExit::Stopped, "tie is stop");
    }

    /// 転送器に渡す時に前の使用の到達・離れた理由を戻す（ORST-17。再利用で `wait_entered` が即座に true にならない）。
    #[test]
    fn rebind_clears_previous_entry_and_exit() {
        let stall = UpstreamStall::new();
        stall.hold(&AtomicBool::new(true));
        assert!(stall.wait_entered(Duration::from_millis(1)));
        assert!(stall.exit_reason().is_some());
        assert!(stall.rebind());
        assert!(
            !stall.wait_entered(Duration::from_millis(30)),
            "stale entry"
        );
        assert_eq!(stall.exit_reason(), None);
    }

    /// `entered >= n` になるまで期限付きで待つ。`wait_entered` は `entered == 0` だけを見るため、
    /// 1 本目の後は即 `true` になり 2 本目の到達を待てない。
    fn wait_entered_at_least(stall: &UpstreamStall, n: u64) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            let entered = stall
                .inner
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entered;
            if entered >= n {
                return true;
            }
            thread::sleep(Duration::from_millis(2));
        }
        false
    }

    fn last_exit_of(stall: &UpstreamStall) -> Option<StallExit> {
        stall
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .last_exit
    }

    /// 先の期限切れを、解放前に入った後続の保持の `Released` で上書きしない（ORST-17・AUDB-6）。
    #[test]
    fn later_release_does_not_overwrite_earlier_expired() {
        let stall = UpstreamStall::new();
        assert!(stall.rebind());
        stall.hold_for(&AtomicBool::new(false), Duration::from_millis(30));
        assert_eq!(stall.exit_reason(), Some(StallExit::Expired));
        let s2 = stall.clone();
        let h = thread::spawn(move || s2.hold_for(&AtomicBool::new(false), Duration::from_secs(5)));
        assert!(wait_entered_at_least(&stall, 2));
        stall.release();
        h.join().unwrap();
        assert_eq!(last_exit_of(&stall), Some(StallExit::Released));
        assert_eq!(stall.exit_reason(), Some(StallExit::Expired));
        assert_eq!(stall.expired_count(), 1);
    }

    /// 先の期限切れを、後続の保持の `Stopped` で上書きしない（ORST-17・AUDB-6）。
    #[test]
    fn later_stop_does_not_overwrite_earlier_expired() {
        let stall = UpstreamStall::new();
        assert!(stall.rebind());
        stall.hold_for(&AtomicBool::new(false), Duration::from_millis(30));
        let stop = Arc::new(AtomicBool::new(false));
        let (s2, stop2) = (stall.clone(), Arc::clone(&stop));
        let h = thread::spawn(move || s2.hold_for(&stop2, Duration::from_secs(5)));
        assert!(wait_entered_at_least(&stall, 2));
        stall.note_stop(&stop);
        h.join().unwrap();
        assert_eq!(last_exit_of(&stall), Some(StallExit::Stopped));
        assert_eq!(stall.exit_reason(), Some(StallExit::Expired));
        assert_eq!(stall.expired_count(), 1);
    }

    /// 転送器へ渡すと期限切れの印も戻る。前の転送器の期限切れを今回の判定に持ち込まない（ORST-17）。
    #[test]
    fn rebind_clears_expired_mark() {
        let stall = UpstreamStall::new();
        assert!(stall.rebind());
        stall.hold_for(&AtomicBool::new(false), Duration::from_millis(30));
        assert_eq!(stall.expired_count(), 1);
        assert!(stall.rebind());
        assert_eq!(stall.exit_reason(), None);
        assert_eq!(stall.expired_count(), 0);
        stall.release();
        stall.hold_for(&AtomicBool::new(false), Duration::from_millis(30));
        // 解放済みは通り抜けで、数えない。
        assert_eq!(stall.expired_count(), 0);
    }

    /// 解放済みの停止点は転送器に渡せない。渡すと古い解放が `Released` と読まれるため（ORST-17・AUDB-6）。
    #[test]
    fn released_stall_is_rejected_at_start() {
        let stall = UpstreamStall::new();
        stall.release();
        assert!(!stall.rebind());
        assert!(matches!(
            DbFaultProxy::start_to_port_with_stall_as(
                echo_upstream(),
                FaultKind::DbBlock,
                stall.clone()
            ),
            Err(DbFaultError::StartFailed)
        ));
        // 解放も到達も記録されないまま（待たずに `Released` が出ない）。
        assert_eq!(stall.exit_reason(), None);
        assert!(!stall.wait_entered(Duration::from_millis(20)));
    }
}
