//! 負荷の生成の部品（TASK-12 5/10）。名前・語・値はすべて仮置きで、人が確認する。
//!
//! 呼び出し元: 結合試験のハーネス（`crates/it-harness`）の上で構成要素へ負荷をかける試験（試験ビルド）。
//! 呼び出し先: 計測の部品（[`RunMeter`]・[`Trial`]）と、呼び出し元が渡す対象の閉包だけ。時計・環境変数・
//! 設定ファイルは自分では読まない（時計は [`RunMeter`] に注入済み。DPOL-32）。
//!
//! 閉ループ（固定数のワーカーが、前の試行を締めてから次の試行を取る）を仮置きで採る。開ループは作らない。
//! ワーカーは 1 度に 1 試行だけ持つので、実測の最大同時実行数は設定の並行数を超えない。
//! 誤りの記録と停止は枠の取得と同じロックで確定する（#5984。順序は [`run_closed_loop`] の doc）。
//! 対象の経路を実際に呼ぶ部分は 6/10 の担当で、ここは対象を閉包として受けるだけ。合否は付けない（ACPT-7）。

use std::cell::Cell;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, Once, PoisonError};
use std::thread;

use crate::meter::{MeterError, Outcome, RunMeter, RunSamples, Trial};

/// 並行数の上限（仮置き。人の確認待ち）。スレッドを作りすぎないための安全弁で、要件の値ではない。
pub const MAX_CONCURRENCY: u64 = 1024;

/// 負荷の生成の誤り。値・名前の中身・`io::Error` の文言・panic の中身は載せない（SECR-11・SECR-23）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// 設定が規則を満たさない（並行数・件数の範囲、件数の溢れ）。
    InvalidConfig,
    /// 本計測の段で対象が `Excluded` を返した。除外は生成器だけが付ける。
    InvalidOutcome,
    /// 計測の部品の誤り。
    Meter(MeterError),
    /// ワーカーのスレッドを作れなかった。
    SpawnFailed,
    /// ワーカーが panic した。
    WorkerPanicked,
    /// 終了時の件数が計画と合わない。
    CountMismatch,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::InvalidConfig => "invalid load config",
            Self::InvalidOutcome => "target returned excluded outcome",
            Self::Meter(_) => "meter error",
            Self::SpawnFailed => "worker spawn failed",
            Self::WorkerPanicked => "worker panicked",
            Self::CountMismatch => "trial count mismatch",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for LoadError {}

impl From<MeterError> for LoadError {
    fn from(e: MeterError) -> Self {
        Self::Meter(e)
    }
}

/// 負荷の型。閉ループだけを持つ（仮置き。開ループは作らない）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopModel {
    /// 固定数のワーカーが、締めてから次を取る。
    Closed,
}

impl LoopModel {
    /// 記録の `condition_loop` の材料にする語（仮置き）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Closed => "closed",
        }
    }
}

/// 負荷の設定。3 つの値は呼び出し元が明示で渡す。既定値・PoC の数値は置かない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadConfig {
    concurrency: u64,
    trials_planned: u64,
    warmup: u64,
}

impl LoadConfig {
    /// # Errors
    /// 並行数が 0 または [`MAX_CONCURRENCY`] 超、本計測の件数が 0、`warmup + trials_planned` が溢れるとき
    /// [`LoadError::InvalidConfig`]。
    pub fn new(concurrency: u64, trials_planned: u64, warmup: u64) -> Result<Self, LoadError> {
        if concurrency == 0 || concurrency > MAX_CONCURRENCY || trials_planned == 0 {
            return Err(LoadError::InvalidConfig);
        }
        if warmup.checked_add(trials_planned).is_none() {
            return Err(LoadError::InvalidConfig);
        }
        Ok(Self {
            concurrency,
            trials_planned,
            warmup,
        })
    }

    pub fn concurrency(&self) -> u64 {
        self.concurrency
    }

    pub fn trials_planned(&self) -> u64 {
        self.trials_planned
    }

    pub fn warmup(&self) -> u64 {
        self.warmup
    }
}

/// 1 回の「回」の結果。設定・負荷の型・計測の結果。集計は [`crate::summarize_run`] が行う。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadRun {
    pub config: LoadConfig,
    pub loop_model: LoopModel,
    pub samples: RunSamples,
}

thread_local! {
    /// このスレッドが対象の閉包を実行中か。実行中の panic は既定の hook に出力させない。
    static IN_TARGET: Cell<bool> = const { Cell::new(false) };
}

static HOOK_ONCE: Once = Once::new();

/// 対象の閉包の実行中だけ panic のメッセージとソース位置を出さない hook を、直前の hook を包んで 1 度だけ入れる。
/// 段階の実行の時計の生成（`stage::run_stages`）でも使う。
/// 抑止はスレッドローカルの印で絞るので、他のスレッドの panic は直前の hook に渡す（SECR-23）。
pub(crate) fn install_quiet_hook() {
    HOOK_ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if IN_TARGET.try_with(Cell::get).unwrap_or(false) {
                return;
            }
            prev(info);
        }));
    });
}

/// 印を立て、スコープを抜けたとき（panic 後も）入場時の値へ戻す。
/// 入れ子で呼ばれても内側が外側の抑止を解かない（SECR-23）。
pub(crate) struct QuietGuard {
    prev: bool,
}

impl QuietGuard {
    pub(crate) fn new() -> Self {
        let prev = IN_TARGET.try_with(|c| c.replace(true)).unwrap_or(false);
        Self { prev }
    }
}

impl Drop for QuietGuard {
    fn drop(&mut self) {
        let prev = self.prev;
        let _ = IN_TARGET.try_with(|c| c.set(prev));
    }
}

/// ロックの下で持つ状態。`first_error` が `Some` であることが「停止中」そのもの（別の停止の印は持たない）。
struct State {
    next: u64,
    first_error: Option<LoadError>,
}

/// ワーカー間で共有する試行の枠・最初の誤り。
///
/// 枠の取得（`claim`）と誤りの記録（`fail`）を同じ 1 つのロックで直列化する（#5984）。記録と停止は 1 つの代入で
/// 同時に確定するので、「記録したが停止の印がまだ」「停止の印を読んだが枠の更新がまだ」という窓が無い。
/// ロックは `claim`・`fail`・最後の取り出しの中だけで持つ。対象の呼び出しの間は持たない（ロックの競合を
/// 計測の区間に入れないため）。
struct Shared {
    state: Mutex<State>,
}

impl Shared {
    fn new() -> Self {
        Self {
            state: Mutex::new(State {
                next: 0,
                first_error: None,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 最初の誤りだけを残す。記録と同時に停止が確定し、以後の `claim` は `false` になる。
    fn fail(&self, e: LoadError) {
        self.fail_observing(e);
    }

    /// `fail` と同じ。記録を確定した同じロックの中で、その時点の取得済みの枠の件数を返す（#5984 の試験用）。
    fn fail_observing(&self, e: LoadError) -> u64 {
        let mut g = self.lock();
        if g.first_error.is_none() {
            g.first_error = Some(e);
        }
        g.next
    }

    /// 試行の枠を 1 つ取る。上限に達した・停止中・溢れたときは `false`。
    fn claim(&self, count: u64) -> bool {
        self.claim_after(count, || {}, || {})
    }

    /// `claim` と同じ。ロックを取る直前に `before_lock` を、停止の判定を通った直後（ロックを持ったまま、枠の更新の前）に
    /// `after_check` を呼ぶ（#5984 の試験用の同期点。通常は何もしない）。
    fn claim_after(
        &self,
        count: u64,
        before_lock: impl FnOnce(),
        after_check: impl FnOnce(),
    ) -> bool {
        before_lock();
        let mut g = self.lock();
        if g.first_error.is_some() || g.next >= count {
            return false;
        }
        after_check();
        match g.next.checked_add(1) {
            Some(n) => {
                g.next = n;
                true
            }
            None => false,
        }
    }

    /// 記録された最初の誤り。
    fn first_error(&self) -> Option<LoadError> {
        self.lock().first_error
    }
}

/// 1 つの段（暖機または本計測）を回す。`warm` のとき試行は結果にかかわらず `Excluded` で締める（開いた区間は計測の部品が破棄する）。
fn run_phase<F>(
    meter: &RunMeter,
    workers: u64,
    count: u64,
    warm: bool,
    target: &F,
) -> Result<(), LoadError>
where
    F: Fn(&mut Trial<'_>) -> Outcome + Sync,
{
    install_quiet_hook();
    let shared = Shared::new();
    let worker = || {
        while shared.claim(count) {
            let mut trial = match meter.begin_trial() {
                Ok(t) => t,
                Err(e) => return shared.fail(e.into()),
            };
            let caught = {
                let _quiet = QuietGuard::new();
                catch_unwind(AssertUnwindSafe(|| target(&mut trial)))
            };
            let outcome = match caught {
                Ok(o) => o,
                // 試行は捨てられ、`unfinished` として数えられる。panic の中身は載せない。
                Err(_) => return shared.fail(LoadError::WorkerPanicked),
            };
            let outcome = if warm {
                Outcome::Excluded
            } else if matches!(outcome, Outcome::Excluded) {
                return shared.fail(LoadError::InvalidOutcome);
            } else {
                outcome
            };
            if let Err(e) = trial.finish(outcome) {
                return shared.fail(e.into());
            }
        }
    };
    let worker = &worker;
    // 並行数が件数より多くても、件数を超える本数は立てない。
    let n = usize::try_from(workers.min(count)).map_err(|_| LoadError::InvalidConfig)?;
    thread::scope(|s| {
        let mut handles = Vec::with_capacity(n);
        for _ in 0..n {
            match thread::Builder::new()
                .name("load-worker".to_owned())
                .spawn_scoped(s, worker)
            {
                Ok(h) => handles.push(h),
                Err(_) => {
                    shared.fail(LoadError::SpawnFailed);
                    break;
                }
            }
        }
        for h in handles {
            if h.join().is_err() {
                shared.fail(LoadError::WorkerPanicked);
            }
        }
    });
    shared.first_error().map_or(Ok(()), Err)
}

/// 閉ループで対象を繰り返し呼ぶ。暖機の段（`warmup > 0` のときだけ）を回してから本計測の段を回す。
/// 2 つの段は続けて回し、重ならない。暖機の試行も通し番号を消費するため、本計測の標本の番号は
/// `warmup + 1` から始まる。`meter` は所有で受け、1 回の「回」ごとに新しい計測を使わせる。
///
/// 対象は試行の中で区間を測り、結果の種類を返す。本計測で `Excluded` を返すと誤り（fail-closed）。
///
/// 誤りの記録・停止・枠の取得の順序（#5984。人の確認待ちの仮置き、ACPT-7）:
/// - 枠の取得と誤りの記録は同じロックの下で全順序に並ぶ。記録と停止は同時に確定する。
/// - 記録より後に枠を取ろうとしたワーカーは必ず取れない。記録の後に新しく取得できる枠は 0 件（保証するのは枠の取得だけ）。
/// - 記録より前に取得済みの枠の試行は止めない。対象の呼び出しまで進んで締めるか誤りで抜ける。記録時点で未完了の
///   試行は各ワーカー最大 1 件で、設定の並行数が上限。対象の閉包の panic はワーカー内の `catch_unwind` が捕捉し、
///   その場で記録する（上の保証が及ぶ）。join で初めて記録されるのは、ワーカー内で捕捉されずにスレッドを終了させた
///   panic（`begin_trial`・`finish` 等、対象の閉包の外）だけで、検出・記録までに他のワーカーが新しく取得して進める
///   件数の累計は上限を保証しない。
///   記録の後に `begin_trial` や対象の呼び出しへ進む試行があり得る（取得済みの枠だけ）。
/// - 返す誤りは最初の 1 件だけ。
///
/// # Errors
/// 部品の誤り・スレッドの生成の失敗・panic・件数の不一致。誤りが出たら上の順序で新しい試行を止め、
/// 立てたワーカーはすべて join してから最初の誤りを返す。
pub fn run_closed_loop<F>(
    meter: RunMeter,
    config: &LoadConfig,
    target: F,
) -> Result<LoadRun, LoadError>
where
    F: Fn(&mut Trial<'_>) -> Outcome + Sync,
{
    if config.warmup > 0 {
        run_phase(&meter, config.concurrency, config.warmup, true, &target)?;
    }
    run_phase(
        &meter,
        config.concurrency,
        config.trials_planned,
        false,
        &target,
    )?;
    let samples = meter.finish()?;
    if samples.executed != config.trials_planned || samples.excluded != config.warmup {
        return Err(LoadError::CountMismatch);
    }
    Ok(LoadRun {
        config: *config,
        loop_model: LoopModel::Closed,
        samples,
    })
}

#[cfg(test)]
mod tests {
    //! #5984: 誤りの記録・停止・枠の取得の順序の単体試験。非公開の `Shared` に直接触る（試験用の公開口は作らない）。
    use super::*;
    use std::time::Duration;

    /// 同期の待ちの明示の期限。到達しなければ同期の失敗として断定で落とす。
    const DEADLINE: Duration = Duration::from_secs(30);

    fn next_of(s: &Shared) -> u64 {
        s.lock().next
    }

    #[test]
    fn fail_keeps_first_error() {
        let s = Shared::new();
        s.fail(LoadError::InvalidOutcome);
        s.fail(LoadError::WorkerPanicked);
        assert_eq!(s.first_error(), Some(LoadError::InvalidOutcome));
    }

    #[test]
    fn claim_stops_at_count() {
        let s = Shared::new();
        assert!(s.claim(3) && s.claim(3) && s.claim(3));
        assert!(!s.claim(3));
        assert_eq!(next_of(&s), 3);
    }

    #[test]
    fn claim_after_fail_does_not_advance() {
        let s = Shared::new();
        assert!(s.claim(10));
        s.fail(LoadError::SpawnFailed);
        assert!(!s.claim(10));
        assert_eq!(next_of(&s), 1);
    }

    /// 同期点を制御した順序の試験。取得者は枠のロックを取る直前の同期点で止まり、その間に実際の `fail` を呼んで
    /// 記録を確定してから再開する。再開した取得は記録の後に走るので、枠を進められず `false` になる。
    /// 停止の判定が枠の取得に無ければ、記録の後でも枠が進む。到達と再開はチャネルで通知し、待ちは明示の期限
    /// （`DEADLINE`）で打ち切る。期限に達したら同期の失敗として断定で落とす（実行順で成否は変わらない）。
    #[test]
    fn claim_resumed_after_fail_does_not_advance() {
        use std::sync::mpsc;
        let s = Shared::new();
        let s = &s;
        let (reached_tx, reached_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (claimed, was_released) = thread::scope(|scope| {
            let h = thread::Builder::new()
                .spawn_scoped(scope, move || {
                    let mut released = false;
                    let claimed = s.claim_after(
                        u64::MAX,
                        || {
                            let _ = reached_tx.send(());
                            // 記録側の明示の通知を受けた場合だけ再開する。期限切れは同期の失敗として残す。
                            released = release_rx.recv_timeout(DEADLINE).is_ok();
                        },
                        || {},
                    );
                    (claimed, released)
                })
                .expect("claimer thread must start");
            let reached = reached_rx.recv_timeout(DEADLINE).is_ok();
            if reached {
                // 実際の fail の経路で記録してから、明示の通知で止めていた取得を再開する。
                s.fail(LoadError::InvalidOutcome);
            }
            // 到達しなかったときも、取得者が期限まで待ち続けないよう通知は送る（失敗は下の断定で報告する）。
            let _ = release_tx.send(());
            let r = h.join().expect("claimer must not panic");
            assert!(reached, "claimer did not reach the sync point");
            r
        });
        assert!(was_released, "claimer was not released by the recorder");
        assert!(!claimed, "a claim succeeded after fail was recorded");
        assert_eq!(next_of(s), 0);
    }

    /// 停止の判定と枠の更新の直列化の試験。取得者は停止の判定を通った直後（更新の前）の同期点で止まり、その間に
    /// 記録者が実際の `fail_observing` を呼ぶ。判定と更新が同じロックなら記録者は取得の更新を待ち、記録を確定した
    /// 時点の件数に更新が含まれる。判定がロックの外にある実装では記録者が先に抜け、その後に更新が走るので、
    /// 記録時点の件数と最終の件数が食い違う。取得者の待ちは期限付きで、直列化が正しいと記録者は抜けないため、
    /// 正しい実装ではこの待ちは期限まで続く（`WINDOW`。成否は実行順に依らない）。
    #[test]
    fn stop_check_and_claim_update_are_serialized() {
        use std::sync::mpsc;
        const WINDOW: Duration = Duration::from_millis(200);
        let s = Shared::new();
        let s = &s;
        let (reached_tx, reached_rx) = mpsc::channel::<()>();
        let (recorded_tx, recorded_rx) = mpsc::channel::<()>();
        let (claimed, snapshot) = thread::scope(|scope| {
            let claimer = thread::Builder::new()
                .spawn_scoped(scope, move || {
                    s.claim_after(
                        u64::MAX,
                        || {},
                        || {
                            let _ = reached_tx.send(());
                            // 記録者が抜けるのを窓の間だけ待つ。抜けなければ（直列化されていれば）そのまま更新へ進む。
                            let _ = recorded_rx.recv_timeout(WINDOW);
                        },
                    )
                })
                .expect("claimer thread must start");
            let reached = reached_rx.recv_timeout(DEADLINE).is_ok();
            let recorder = thread::Builder::new()
                .spawn_scoped(scope, || {
                    let n = s.fail_observing(LoadError::InvalidOutcome);
                    let _ = recorded_tx.send(());
                    n
                })
                .expect("recorder thread must start");
            let snapshot = recorder.join().expect("recorder must not panic");
            let claimed = claimer.join().expect("claimer must not panic");
            assert!(reached, "claimer did not reach the sync point");
            (claimed, snapshot)
        });
        assert!(claimed, "the in-flight claim should have been granted");
        assert_eq!(
            next_of(s),
            snapshot,
            "a claim advanced the slot after fail was recorded"
        );
    }

    /// 記録の後に枠が 1 件も通らない。取得者は停止まで枠を取り続け、その最中に記録者が記録を確定する。
    /// 記録を確定したロックの中で観測した件数と、全員が止まった後の最終の件数が一致する。記録と枠の更新が別のロックなら、
    /// 記録の後に枠が通って一致しない。取得が進んだことは取得者からのチャネルの通知で確かめ（待ちは `DEADLINE`）、
    /// 取得者は記録で必ず止まる（起動に失敗しても記録して解放する）ので、試験は有限で終わる。競合の窓を狙うため複数回繰り返す。
    #[test]
    fn claims_in_flight_do_not_advance_after_fail_is_recorded() {
        use std::sync::mpsc;
        const CLAIMERS: usize = 4;
        const ROUNDS: usize = 200;
        const MAX_CLAIMS: u32 = 5_000_000;
        for _ in 0..ROUNDS {
            let s = Shared::new();
            let (snapshot, spawn_failed, progressed) = thread::scope(|scope| {
                let (progress_tx, progress_rx) = mpsc::channel::<()>();
                let mut hs = Vec::new();
                let mut spawn_failed = false;
                for _ in 0..CLAIMERS {
                    let tx = progress_tx.clone();
                    // 停止の判定が欠けても終わるよう反復に上限を置き、止まったかどうかを返す。
                    let r = thread::Builder::new().spawn_scoped(scope, || {
                        let tx = tx;
                        let mut notified = false;
                        (0..MAX_CLAIMS).any(|_| {
                            let ok = s.claim(u64::MAX);
                            if ok && !notified {
                                notified = true;
                                let _ = tx.send(());
                            }
                            !ok
                        })
                    });
                    match r {
                        Ok(h) => hs.push(h),
                        Err(_) => {
                            spawn_failed = true;
                            break;
                        }
                    }
                }
                drop(progress_tx);
                // 取得が進んだ通知を明示の期限まで待つ。進まなくても取得者を止めるため記録へ進むが、その回は後で失敗にする。
                let progressed = !hs.is_empty() && progress_rx.recv_timeout(DEADLINE).is_ok();
                let snapshot = s.fail_observing(LoadError::InvalidOutcome);
                let all_stopped = hs
                    .into_iter()
                    .all(|h| h.join().expect("claimer must not panic"));
                assert!(
                    all_stopped,
                    "a claimer did not stop after fail was recorded"
                );
                (snapshot, spawn_failed, progressed)
            });
            assert!(!spawn_failed, "claimer thread failed to start");
            assert!(progressed, "no claim was observed before the record");
            assert_eq!(
                next_of(&s),
                snapshot,
                "a claim succeeded after fail was recorded"
            );
        }
    }
}
