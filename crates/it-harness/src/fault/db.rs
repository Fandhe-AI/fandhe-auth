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
//! - 遮断は既存の接続を切り、待ち受けのソケットを閉じる（新しい接続はカーネルが拒否する）。
//!   解除は同じポートで作り直す。解除しても切れた接続は戻らないので、client は解除の後につなぎ直す。遅延は応答（上流から client）の向きの塊ごとに足す。
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
    /// 解除の前の確かめ（試し bind）に失敗した（ポートが取られた等）。遮断のまま、`Released` は記録していない。
    RebindFailed,
    /// `Released` の記録の後に、待ち受けの作り直しに失敗した。遮断のままで、その記録は正しい記録として使わない。
    RebindFailedAfterRecord,
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
            Self::RebindFailed => f.write_str("db fault error: rebind failed"),
            Self::RebindFailedAfterRecord => {
                f.write_str("db fault error: rebind failed after record")
            }
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
    /// 待ち受け。`None` は遮断中（ソケットを閉じている）。accept 1 回とその接続の処理（`handle_conn`）は
    /// このロックの下で行い、`block`・`release_block` も同じロックの下で差し替える（ORST-17）。
    listener: Mutex<Option<TcpListener>>,
    stop: AtomicBool,
    blocked: AtomicBool,
    delay_ms: AtomicU64,
    /// 遅延の解除のたびに進める世代。処理中の待ちが古い遅延を引きずらないために見る。
    delay_gen: AtomicU64,
    /// 遮断のたびに進める世代。上流への接続の確立中に遮断が入ったかを見る。
    block_gen: AtomicU64,
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
        // 遮断として記録できる種類だけ受ける（fail-closed）。
        if !matches!(
            block_kind,
            FaultKind::DbBlock | FaultKind::ExternalStoreBlock
        ) {
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
        let shared = Arc::new(Shared {
            listener: Mutex::new(Some(listener)),
            stop: AtomicBool::new(false),
            blocked: AtomicBool::new(false),
            delay_ms: AtomicU64::new(0),
            delay_gen: AtomicU64::new(0),
            block_gen: AtomicU64::new(0),
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
                .spawn(move || accept_loop(&shared, &upstream))
                .map_err(|_| DbFaultError::StartFailed)?
        };
        Ok(Self {
            shared,
            upstream,
            listen_port,
            accept: Some(accept),
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

    /// 転送器が accept した後に直ちに閉じた接続の数（上流なし・上流への接続の失敗・確立中の遮断）。
    /// 遮断中の接続は待ち受けが閉じているのでカーネルの中で拒否され、ここでは数えない。
    pub fn refused_count(&self) -> u64 {
        self.shared.refused.load(Ordering::SeqCst)
    }

    /// DB への経路を遮断する。開いている接続を切り、待ち受けを閉じる（新しい接続はカーネルが拒否する）。
    pub fn block(&mut self, timeline: &mut FaultTimeline) -> Result<(), DbFaultError> {
        if self.block_active {
            return Err(DbFaultError::AlreadyActive);
        }
        // accept のスレッドは、ロックを accept 1 回の間だけ持つ（上流への接続はロックの外）。ここでロックを取ってから
        // `Applied` を記録し、記録が通ったら続けて状態を変える。記録の時刻と実際の遮断の間に待ちを挟まない。
        let mut slot = self
            .shared
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        timeline
            .record_now(self.block_kind, FaultPhase::Applied)
            .map_err(DbFaultError::Record)?;
        // 待ち受けのソケットを閉じる。以後の connect はカーネルが拒否し、待ち行列に残っていた未 accept の
        // 接続もリセットされる（ORST-17）。確立中の接続は世代の差で無効になり、`sever_all` が切る。
        *slot = None;
        self.shared.blocked.store(true, Ordering::SeqCst);
        self.shared.block_gen.fetch_add(1, Ordering::SeqCst);
        self.shared.sever_all();
        drop(slot);
        self.block_active = true;
        Ok(())
    }

    /// 遮断を解除する。切れた接続は戻らないので、client はつなぎ直す。
    ///
    /// 手順（ORST-17）: (1) 同じポートで試し bind して確かめ、すぐ閉じる（待ち行列に入った接続はリセットされる）。
    /// (2) `Released` を記録する。(3) 待ち受けを作り直して再開する。実際の待ち受けは記録の後に始まるので、
    /// 記録より前に確立した接続は上流へ届かない。(1) の失敗は [`DbFaultError::RebindFailed`]（記録なし・遮断のまま）。
    /// (3) の失敗は [`DbFaultError::RebindFailedAfterRecord`]（記録済み・遮断のまま。その記録は正しい記録として使わない）。
    pub fn release_block(&mut self, timeline: &mut FaultTimeline) -> Result<(), DbFaultError> {
        self.release_block_with(timeline, || {})
    }

    /// [`Self::release_block`] の本体。`after_probe` は試し bind の待ち受けが生きている間（記録の前）に呼ぶ。
    /// 順序の試験が、その間に始めた接続が届かないことを確かめるための口で、本番の経路は空の関数を渡す。
    fn release_block_with(
        &mut self,
        timeline: &mut FaultTimeline,
        after_probe: impl FnOnce(),
    ) -> Result<(), DbFaultError> {
        if !self.block_active {
            return Err(DbFaultError::NotActive);
        }
        let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, self.listen_port))
            .map_err(|_| DbFaultError::RebindFailed)?;
        if probe.local_addr().ok().map(|a| a.port()) != Some(self.listen_port) {
            return Err(DbFaultError::RebindFailed);
        }
        after_probe();
        // 閉じると、待ち行列にあった接続はカーネルがリセットする。
        drop(probe);
        timeline
            .record_now(self.block_kind, FaultPhase::Released)
            .map_err(DbFaultError::Record)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, self.listen_port))
            .map_err(|_| DbFaultError::RebindFailedAfterRecord)?;
        if listener.local_addr().ok().map(|a| a.port()) != Some(self.listen_port)
            || listener.set_nonblocking(true).is_err()
        {
            return Err(DbFaultError::RebindFailedAfterRecord);
        }
        {
            let mut slot = self
                .shared
                .listener
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            self.shared.blocked.store(false, Ordering::SeqCst);
            *slot = Some(listener);
        }
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

fn accept_loop(shared: &Arc<Shared>, upstream: &Upstream) {
    while !shared.stop.load(Ordering::SeqCst) {
        // ロックは accept 1 回と、その時点の遮断の世代・状態の読み取りの間だけ持つ。上流への接続と中継は
        // ロックの外で行い、`block`・`release_block` を待たせない。眠りもロックの外。
        let accepted = {
            let slot = shared
                .listener
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match slot.as_ref().map(TcpListener::accept) {
                Some(Ok((client, _))) => Some((client, shared.block_gen.load(Ordering::SeqCst))),
                _ => None,
            }
        };
        match accepted {
            Some((client, block_gen)) => handle_conn(client, block_gen, shared, upstream),
            None => thread::sleep(ACCEPT_TICK),
        }
    }
}

fn refuse(client: &TcpStream, shared: &Shared) {
    shared.refused.fetch_add(1, Ordering::SeqCst);
    let _ = client.shutdown(Shutdown::Both);
}

/// `block_gen` は accept した時点（ロックの下）の遮断の世代。確立中に遮断が入って解除されても、世代の差で見つける。
fn handle_conn(client: TcpStream, block_gen: u64, shared: &Arc<Shared>, upstream: &Upstream) {
    // 受けた接続は blocking に戻す（listener の非 blocking を引き継ぐ環境がある）。
    if client.set_nonblocking(false).is_err() || shared.halted() {
        return refuse(&client, shared);
    }
    let Some(port) = upstream.port() else {
        return refuse(&client, shared);
    };
    let (Ok(c_read), Ok(c_kill)) = (client.try_clone(), client.try_clone()) else {
        return refuse(&client, shared);
    };
    // 上流への接続の前から client を遮断の対象にする（確立中の遮断を取りこぼさない）。
    let id = shared.next_conn_id.fetch_add(1, Ordering::SeqCst);
    shared
        .conns
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, c_kill));
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(up) = TcpStream::connect_timeout(&addr, UPSTREAM_CONNECT_TIMEOUT) else {
        shared.sever_conn(id);
        return refuse(&client, shared);
    };
    let (Ok(u_write), Ok(u_kill)) = (up.try_clone(), up.try_clone()) else {
        shared.sever_conn(id);
        return refuse(&client, shared);
    };
    shared
        .conns
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, u_kill));
    if shared.halted() || shared.block_gen.load(Ordering::SeqCst) != block_gen {
        let _ = up.shutdown(Shutdown::Both);
        shared.sever_conn(id);
        return refuse(&client, shared);
    }
    // 両方向の転送が終わった側が複製を外す（半閉じの間は遮断で切れるよう残す）。
    let remaining = Arc::new(AtomicUsize::new(2));
    let (r1, r2) = (Arc::clone(&remaining), remaining);
    // 登録の間に遮断が入った場合に取りこぼさない。
    if shared.halted() || shared.block_gen.load(Ordering::SeqCst) != block_gen {
        shared.sever_conn(id);
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

    /// 遮断の間は待ち受けのポートを手放すので、同じ試験プロセスの他の試験が `bind(0)` で同じポートを取ると
    /// 解除が失敗する（方式の限界。`docs/harness/fault-injection.md`）。このため試験どうしを直列にする。
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
    }

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

    /// ORST-17: 試し bind の待ち受けが生きている間（`Released` の記録の前）に確立した接続は、
    /// 記録の後に再開した待ち受けから上流へ届かない。後から始めた接続は届く。
    #[test]
    fn conn_established_before_release_record_never_reaches_upstream() {
        let _serial = serial();
        let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
        let mut tl = timeline();
        p.block(&mut tl).unwrap();
        let port = p.listen_port();
        let mut early = None;
        // 他のプロセスが解除の前の確かめの前にポートを取ることがある（方式の限界）。その失敗は遮断のまま何も変えない
        // ので、取られていなくなるまで再試行する。それ以外の誤りは失敗にする。
        for _ in 0..500 {
            match p.release_block_with(&mut tl, || early = Some(connect(port))) {
                Ok(()) => break,
                Err(DbFaultError::RebindFailed) => thread::sleep(Duration::from_millis(10)),
                Err(e) => panic!("release failed: {e:?}"),
            }
        }
        let mut early = early.expect("connect during probe");
        assert_eq!(echo_or_cut(&mut early, b"e"), None, "early conn is reset");
        assert_eq!(p.accepted_count(), 0);
        let mut later = connect(port);
        assert_eq!(echo_or_cut(&mut later, b"l"), Some(b"l".to_vec()));
    }

    /// `block()` は `Applied` を、リスナーのロックを取った後に記録する。ロックが他で持たれている間は
    /// 記録しない（記録の時刻と実際の遮断の間に待ちを挟まない）。
    #[test]
    fn block_records_applied_only_after_taking_the_lock() {
        let _serial = serial();
        let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
        let mut tl = timeline();
        let shared = Arc::clone(&p.shared);
        let held = shared.listener.lock().unwrap();
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
}
