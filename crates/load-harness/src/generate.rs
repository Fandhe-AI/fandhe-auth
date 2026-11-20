//! 負荷の生成の部品（TASK-12 5/10）。名前・語・値はすべて仮置きで、人が確認する。
//!
//! 呼び出し元: 結合試験のハーネス（`crates/it-harness`）の上で構成要素へ負荷をかける試験（試験ビルド）。
//! 呼び出し先: 計測の部品（[`RunMeter`]・[`Trial`]）と、呼び出し元が渡す対象の閉包だけ。時計・環境変数・
//! 設定ファイルは自分では読まない（時計は [`RunMeter`] に注入済み。DPOL-32）。
//!
//! 閉ループ（固定数のワーカーが、前の試行を締めてから次の試行を取る）を仮置きで採る。開ループは作らない。
//! ワーカーは 1 度に 1 試行だけ持つので、実測の最大同時実行数は設定の並行数を超えない。
//! 対象の経路を名前・区間と組で呼ぶ部分は [`crate::target`]（6/10）の担当で、ここは対象を閉包として受けるだけ。合否は付けない（ACPT-7）。

use std::cell::Cell;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// 呼び出し元の停止の印が立ち、新しい試行を止めた（[`run_closed_loop_until`]）。
    Cancelled,
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
            Self::Cancelled => "load cancelled",
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
/// 抑止はスレッドローカルの印で絞るので、他のスレッドの panic は直前の hook に渡す（SECR-23）。
fn install_quiet_hook() {
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

/// 印を立て、スコープを抜けたとき（panic 後も）下ろす。
struct QuietGuard;

impl QuietGuard {
    fn new() -> Self {
        IN_TARGET.with(|c| c.set(true));
        Self
    }
}

impl Drop for QuietGuard {
    fn drop(&mut self) {
        let _ = IN_TARGET.try_with(|c| c.set(false));
    }
}

/// ワーカー間で共有する試行の枠・停止の印・最初の誤り。
struct Shared<'a> {
    /// 次の枠の番号。枠の割り当てと内部の停止の確認は、この Mutex の中で 1 つの区間として行う。
    next: Mutex<u64>,
    stop: AtomicBool,
    /// 呼び出し元が立てる停止の印。立っている間は新しい枠を取らせない。
    cancel: &'a AtomicBool,
    /// `claim` が `cancel` の立っていることを観測した事実。呼び出し元が後で印を戻しても残す。
    cancelled: AtomicBool,
    first_error: Mutex<Option<LoadError>>,
}

impl<'a> Shared<'a> {
    fn new(cancel: &'a AtomicBool) -> Self {
        Self {
            next: Mutex::new(0),
            stop: AtomicBool::new(false),
            cancel,
            cancelled: AtomicBool::new(false),
            first_error: Mutex::new(None),
        }
    }

    /// 最初の誤りだけを残し、他のワーカーが新しい枠を取らないよう停止の印を立てる。
    fn fail(&self, e: LoadError) {
        let mut g = self
            .first_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if g.is_none() {
            *g = Some(e);
        }
        drop(g);
        // 枠の割り当てと同じ Mutex の中で立て、立てた後に枠が取れないことを 1 つの順序で決める。
        let _slots = self.next.lock().unwrap_or_else(PoisonError::into_inner);
        self.stop.store(true, Ordering::SeqCst);
    }

    /// 起動側がワーカーを立てる前に、停止の印を確認して観測の事実を残す。立っていれば `true`。
    /// `claim` と同じ Mutex の中で行い、観測済みの停止は印が戻っても残る。
    fn observe_cancel(&self) -> bool {
        let _slots = self.next.lock().unwrap_or_else(PoisonError::into_inner);
        if self.cancelled.load(Ordering::SeqCst) {
            return true;
        }
        if self.cancel.load(Ordering::SeqCst) {
            self.cancelled.store(true, Ordering::SeqCst);
            return true;
        }
        false
    }

    /// 試行の枠を 1 つ取る。上限に達した・停止中・溢れたときは `false`。
    ///
    /// 停止の確認（内部の `stop`・呼び出し元の `cancel`）と枠の加算は同じ Mutex の中で行う。
    /// 内部の停止（`fail`）は同じ Mutex の中で立つので、立った後に枠が取れることは無い。
    /// 呼び出し元の `cancel` は外の `AtomicBool` なので、確認の瞬間が停止との先後の境になる
    /// （確認より後に立った印は、その枠には効かない）。
    fn claim(&self, count: u64) -> bool {
        let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
        // 観測済みの停止は、呼び出し元が印を戻しても以後の枠を取らせない。
        if self.cancelled.load(Ordering::SeqCst) {
            return false;
        }
        if self.cancel.load(Ordering::SeqCst) {
            self.cancelled.store(true, Ordering::SeqCst);
            return false;
        }
        if self.stop.load(Ordering::SeqCst) || *next >= count {
            return false;
        }
        match next.checked_add(1) {
            Some(v) => {
                *next = v;
                true
            }
            None => false,
        }
    }
}

/// 1 つの段（暖機または本計測）を回す。`warm` のとき試行は結果にかかわらず `Excluded` で締める（開いた区間は計測の部品が破棄する）。
fn run_phase<F>(
    meter: &RunMeter,
    workers: u64,
    count: u64,
    warm: bool,
    cancel: &AtomicBool,
    target: &F,
) -> Result<(), LoadError>
where
    F: Fn(&mut Trial<'_>) -> Outcome + Sync,
{
    install_quiet_hook();
    let shared = Shared::new(cancel);
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
            // 立てる前に停止を確認する。停止中は残りのワーカーを立てず、`Cancelled` を返させる。
            if shared.observe_cancel() {
                break;
            }
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
    let err = *shared
        .first_error
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(e) = err {
        return Err(e);
    }
    // 停止の印で枠の取得が止まった段は、件数が計画に届かない。次の段へ進ませない。
    // 呼び出し元が join 後に印を戻していても、観測済みの停止は落とさない。
    if shared.cancelled.load(Ordering::SeqCst) {
        return Err(LoadError::Cancelled);
    }
    Ok(())
}

/// 閉ループで対象を繰り返し呼ぶ。暖機の段（`warmup > 0` のときだけ）を回してから本計測の段を回す。
/// 2 つの段は続けて回し、重ならない。暖機の試行も通し番号を消費するため、本計測の標本の番号は
/// `warmup + 1` から始まる。`meter` は所有で受け、1 回の「回」ごとに新しい計測を使わせる。
///
/// 対象は試行の中で区間を測り、結果の種類を返す。本計測で `Excluded` を返すと誤り（fail-closed）。
///
/// # Errors
/// 部品の誤り・スレッドの生成の失敗・panic・件数の不一致。誤りが出たら新しい試行を止め、
/// 立てたワーカーはすべて join してから最初の誤りを返す。
pub fn run_closed_loop<F>(
    meter: RunMeter,
    config: &LoadConfig,
    target: F,
) -> Result<LoadRun, LoadError>
where
    F: Fn(&mut Trial<'_>) -> Outcome + Sync,
{
    run_closed_loop_until(meter, config, &AtomicBool::new(false), target)
}

/// [`run_closed_loop`] に、呼び出し元の停止の印を足したもの。`cancel` が立つと、暖機・本計測のどちらでも
/// 新しい試行の枠を取らせず、次の段へ進まず、立てたワーカーを join してから最初の誤りを返す。
/// 誤りが無く印だけが立っていたときは [`LoadError::Cancelled`]。印は対象が結果（`Outcome`）とは別の経路で
/// 生成器へ伝えるための道で、暖機の段が結果を問わず `Excluded` で締める規則は変えない。
/// 枠を取り終えて走っている試行は、印が立っても締めるまで続く（途中で止めない）。枠の取得は印の確認の瞬間に決まり、確認より後に立った印は、その確認を通った枠には効かない。
///
/// # Errors
/// [`run_closed_loop`] と同じ。加えて停止の印が立ったとき [`LoadError::Cancelled`]。
pub fn run_closed_loop_until<F>(
    meter: RunMeter,
    config: &LoadConfig,
    cancel: &AtomicBool,
    target: F,
) -> Result<LoadRun, LoadError>
where
    F: Fn(&mut Trial<'_>) -> Outcome + Sync,
{
    if config.warmup > 0 {
        run_phase(
            &meter,
            config.concurrency,
            config.warmup,
            true,
            cancel,
            &target,
        )?;
    }
    run_phase(
        &meter,
        config.concurrency,
        config.trials_planned,
        false,
        cancel,
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
    use super::*;

    /// 観測済みの停止は、呼び出し元が印を戻しても以後の枠を取らせない（`run_closed_loop_until` の停止契約）。
    #[test]
    fn observed_cancel_blocks_claims_after_reset() {
        let cancel = AtomicBool::new(false);
        let shared = Shared::new(&cancel);
        assert!(shared.claim(10));
        cancel.store(true, Ordering::SeqCst);
        assert!(!shared.claim(10));
        cancel.store(false, Ordering::SeqCst);
        assert!(!shared.claim(10));
        assert!(shared.cancelled.load(Ordering::SeqCst));
    }

    /// 起動側の確認: 立っている印を観測し、印が戻っても観測済みとして残る。
    /// 立っていれば `run_phase` はワーカーを 1 本も立てない（スレッド生成の失敗より `Cancelled` が先）。
    #[test]
    fn observe_cancel_sees_preset_and_persists() {
        let cancel = AtomicBool::new(false);
        let shared = Shared::new(&cancel);
        assert!(!shared.observe_cancel());
        cancel.store(true, Ordering::SeqCst);
        assert!(shared.observe_cancel());
        cancel.store(false, Ordering::SeqCst);
        assert!(shared.observe_cancel());
        assert!(!shared.claim(10));
    }
}
