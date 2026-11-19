#![cfg(unix)]
//! TASK-13 (3/8): インスタンスの停止の注入の試験。子プロセスは試験の中で起動する `sleep` 等だけ。
//! 時間の値は試験の中だけの仮の値（THRT-29）。記録は事実の出力で合否の判定ではない（ACPT-7）。

use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fandhe_auth_it_harness::fault::{FaultRecordError, InstanceFaultError, InstanceFaultHandle};
use fandhe_auth_it_harness::record::UtcTimestamp;
use fandhe_auth_it_harness::{FaultRecord, FaultTimeline};

const LONG: Duration = Duration::from_secs(10);

fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn timeline() -> FaultTimeline {
    FaultTimeline::start(Instant::now(), t0()).unwrap()
}

fn finish(tl: FaultTimeline) -> Result<FaultRecord, FaultRecordError> {
    tl.finish(UtcTimestamp::from_unix_seconds(1_700_000_060))
}

fn sleeper() -> InstanceFaultHandle {
    InstanceFaultHandle::spawn(Command::new("sleep").arg("30")).unwrap()
}

#[test]
fn sigterm_stops_and_records() {
    let mut h = sleeper();
    let mut tl = timeline();
    h.sigterm(&mut tl).unwrap();
    assert_eq!(h.wait_exit(LONG).unwrap().signal, Some(15));
    let r = finish(tl).unwrap();
    assert_eq!(r.events().len(), 1);
    assert_eq!(r.unreleased_count(), 0);
}

#[test]
fn sigkill_stops_and_records() {
    let mut h = sleeper();
    let mut tl = timeline();
    h.sigkill(&mut tl).unwrap();
    assert_eq!(h.wait_exit(LONG).unwrap().signal, Some(9));
    assert_eq!(finish(tl).unwrap().events().len(), 1);
}

#[test]
fn ignored_sigterm_does_not_block_and_sigkill_follows() {
    let mut h =
        InstanceFaultHandle::spawn(Command::new("sh").args(["-c", "trap '' TERM; exec sleep 30"]))
            .unwrap();
    // trap の設定を待つ猶予（開発の値）
    std::thread::sleep(Duration::from_millis(300));
    let mut tl = timeline();
    let started = Instant::now();
    h.sigterm(&mut tl).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        h.wait_exit(Duration::from_millis(200)),
        Err(InstanceFaultError::ExitTimeout)
    );
    h.sigkill(&mut tl).unwrap();
    assert_eq!(h.wait_exit(LONG).unwrap().signal, Some(9));
    assert_eq!(finish(tl).unwrap().events().len(), 2);
}

#[test]
fn baseline_rejects_and_does_not_signal() {
    let mut h = sleeper();
    let mut tl = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    let rec = InstanceFaultError::Record(FaultRecordError::EventsInBaseline);
    assert_eq!(h.sigterm(&mut tl), Err(rec));
    assert_eq!(h.sigkill(&mut tl), Err(rec));
    assert_eq!(
        h.wait_exit(Duration::from_millis(100)),
        Err(InstanceFaultError::ExitTimeout)
    );
}

#[test]
fn already_exited_is_not_running_and_records_nothing() {
    let mut h = InstanceFaultHandle::spawn(&mut Command::new("true")).unwrap();
    h.wait_exit(LONG).unwrap();
    let mut tl = timeline();
    assert_eq!(h.sigterm(&mut tl), Err(InstanceFaultError::NotRunning));
    assert_eq!(h.sigkill(&mut tl), Err(InstanceFaultError::NotRunning));
    assert_eq!(
        finish(tl).err(),
        Some(FaultRecordError::NoInjectionInFaultRun)
    );
}

#[test]
fn stop_time_is_between_surrounding_clock_reads() {
    let mut h = sleeper();
    let origin = Instant::now();
    let mut tl = FaultTimeline::start(origin, t0()).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let before = origin.elapsed().as_millis() as u64;
    h.sigkill(&mut tl).unwrap();
    let after = origin.elapsed().as_millis() as u64;
    h.wait_exit(LONG).unwrap();
    let at = finish(tl).unwrap().events()[0].offset_ms;
    assert!(before <= at && at <= after, "{before} {at} {after}");
}

#[test]
fn debug_errors_and_record_do_not_leak_pid() {
    let child = Command::new("sleep").arg("30").spawn().unwrap();
    let pid = child.id().to_string();
    let mut h = InstanceFaultHandle::adopt(child);
    let mut base = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    let e = h.sigterm(&mut base).unwrap_err();
    let mut tl = timeline();
    h.sigkill(&mut tl).unwrap();
    h.wait_exit(LONG).unwrap();
    let e2 = h.sigkill(&mut tl).unwrap_err();
    let all = format!(
        "{h:?} {e} {e:?} {e2} {e2:?} {:?}",
        finish(tl).unwrap().events()
    );
    assert!(!all.contains(&pid), "pid leaked");
}

#[test]
fn drop_reaps_child() {
    let child = Command::new("sleep").arg("30").spawn().unwrap();
    let pid = child.id().to_string();
    InstanceFaultHandle::adopt(child).shutdown();
    // 回収済みなら存在しないので kill -0 は失敗する
    let out = Command::new("/bin/kill")
        .args(["-0", &pid])
        .output()
        .unwrap();
    assert!(!out.status.success());
}
