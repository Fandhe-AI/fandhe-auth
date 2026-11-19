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
//! - 遮断は既存の接続を切り、新しい接続を直ちに閉じる。解除しても切れた接続は戻らないので、client は
//!   解除の後につなぎ直す。遅延は応答（上流から client）の向きの塊ごとに足す。
//! - 接続の確立（上流への connect。上限 5 秒）は accept のスレッドで同期に行う。上流は `127.0.0.1` で
//!   通常は即座に結果が出るが、固まると後続の accept がその間止まる（試験専用の制約として受け入れる）。
//! - 刻みの値（accept 5 ms・待ちの刻み 10 ms・上流への接続の上限 5 秒・遅延の上限）は開発の値で要件の値ではない
//!   （THRT-29）。
//!
//! 名前・形・遮断の形（直ちに閉じる。受けて黙る形は入れていない）は仮置きで、人が確認する。

use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
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
}

impl fmt::Display for DbFaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartFailed => f.write_str("db fault error: start failed"),
            Self::AlreadyActive => f.write_str("db fault error: already active"),
            Self::NotActive => f.write_str("db fault error: not active"),
            Self::InvalidParam => f.write_str("db fault error: invalid parameter"),
            Self::Record(e) => write!(f, "db fault error: {e}"),
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

/// 転送のスレッドと注入の入口で共有する状態。
struct Shared {
    stop: AtomicBool,
    blocked: AtomicBool,
    delay_ms: AtomicU64,
    accepted: AtomicU64,
    refused: AtomicU64,
    /// 両側を切るための複製（接続 ID と組）。両方向の転送が終わった接続の分は外す。
    conns: Mutex<Vec<(u64, TcpStream)>>,
    next_conn_id: AtomicU64,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl Shared {
    fn sever_all(&self) {
        let mut g = self.conns.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, s) in g.drain(..) {
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
    block_active: bool,
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
        Self::start(Upstream::Port(port))
    }

    /// [`crate::database::Database`] の接続情報のハンドルに向けて始める。Database の起動前に作れる。
    /// 接続のたびにポートを読み、値が無ければその接続を閉じる。
    pub fn start_for(handle: EndpointHandle) -> Result<Self, DbFaultError> {
        Self::start(Upstream::Endpoint(handle))
    }

    fn start(upstream: Upstream) -> Result<Self, DbFaultError> {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| DbFaultError::StartFailed)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| DbFaultError::StartFailed)?;
        let listen_port = listener
            .local_addr()
            .map_err(|_| DbFaultError::StartFailed)?
            .port();
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            blocked: AtomicBool::new(false),
            delay_ms: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            refused: AtomicU64::new(0),
            conns: Mutex::new(Vec::new()),
            next_conn_id: AtomicU64::new(0),
            workers: Mutex::new(Vec::new()),
        });
        let accept = {
            let shared = Arc::clone(&shared);
            let upstream = upstream.clone();
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
            block_active: false,
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

    /// 遮断・上流なしのために直ちに閉じた接続の数。
    pub fn refused_count(&self) -> u64 {
        self.shared.refused.load(Ordering::SeqCst)
    }

    /// DB への経路を遮断する。開いている接続を切り、新しい接続を直ちに閉じる。
    pub fn block(&mut self, timeline: &mut FaultTimeline) -> Result<(), DbFaultError> {
        if self.block_active {
            return Err(DbFaultError::AlreadyActive);
        }
        timeline
            .record_now(FaultKind::DbBlock, FaultPhase::Applied)
            .map_err(DbFaultError::Record)?;
        self.shared.blocked.store(true, Ordering::SeqCst);
        self.shared.sever_all();
        self.block_active = true;
        Ok(())
    }

    /// 遮断を解除する。切れた接続は戻らないので、client はつなぎ直す。
    pub fn release_block(&mut self, timeline: &mut FaultTimeline) -> Result<(), DbFaultError> {
        if !self.block_active {
            return Err(DbFaultError::NotActive);
        }
        timeline
            .record_now(FaultKind::DbBlock, FaultPhase::Released)
            .map_err(DbFaultError::Record)?;
        self.shared.blocked.store(false, Ordering::SeqCst);
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
        self.delay_active = None;
        Ok(())
    }

    /// 停止する（`Drop` と同じ）。解除していない注入があっても記録は触らない。数は `FaultTimeline::finish`
    /// の `unreleased_count` に出る。
    pub fn stop(mut self) {
        self.shutdown_all();
    }

    fn shutdown_all(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
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

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>, upstream: &Upstream) {
    while !shared.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((client, _)) => handle_conn(client, shared, upstream),
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted => {
                thread::sleep(ACCEPT_TICK);
            }
            Err(_) => thread::sleep(ACCEPT_TICK),
        }
    }
}

fn refuse(client: &TcpStream, shared: &Shared) {
    shared.refused.fetch_add(1, Ordering::SeqCst);
    let _ = client.shutdown(Shutdown::Both);
}

fn handle_conn(client: TcpStream, shared: &Arc<Shared>, upstream: &Upstream) {
    // 受けた接続は blocking に戻す（listener の非 blocking を引き継ぐ環境がある）。
    if client.set_nonblocking(false).is_err() || shared.halted() {
        return refuse(&client, shared);
    }
    let Some(port) = upstream.port() else {
        return refuse(&client, shared);
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(up) = TcpStream::connect_timeout(&addr, UPSTREAM_CONNECT_TIMEOUT) else {
        return refuse(&client, shared);
    };
    let (Ok(c_read), Ok(u_write), Ok(c_kill), Ok(u_kill)) = (
        client.try_clone(),
        up.try_clone(),
        client.try_clone(),
        up.try_clone(),
    ) else {
        return refuse(&client, shared);
    };
    let id = shared.next_conn_id.fetch_add(1, Ordering::SeqCst);
    {
        let mut g = shared.conns.lock().unwrap_or_else(PoisonError::into_inner);
        g.push((id, c_kill));
        g.push((id, u_kill));
    }
    // 両方向の転送が終わった側が複製を外す（半閉じの間は遮断で切れるよう残す）。
    let remaining = Arc::new(AtomicUsize::new(2));
    let (r1, r2) = (Arc::clone(&remaining), remaining);
    // 登録の間に遮断が入った場合に取りこぼさない。
    if shared.halted() {
        shared.sever_all();
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

/// 現在の遅延だけ刻んで眠る。停止と遮断が入ったら抜ける。
fn wait_delay(shared: &Shared) {
    let ms = shared.delay_ms.load(Ordering::SeqCst);
    if ms == 0 {
        return;
    }
    let deadline = Instant::now() + Duration::from_millis(ms);
    while !shared.halted() {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        thread::sleep(WAIT_TICK.min(deadline - now));
    }
}
