//! Progress reporting and cancellation for the long operations.
//!
//! A scan reads more than a terabyte and an apply moves hundreds of gigabytes.
//! Both must report while they run and both must stop when the person asks.
//! Neither knows about Tauri: the caller supplies a sink and gets called back.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A flag the caller raises to stop a long operation.
///
/// Cloning shares the flag. The engine checks it between files and between
/// plan groups, never in the middle of a write, so a cancellation always leaves
/// the disk in a state the journal describes.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    /// Tests only: raise the flag at the Nth check, so a test can stop an
    /// operation at every point it can be stopped, one after another.
    #[cfg(test)]
    stop_at_check: Option<Arc<AtomicU64>>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// A token that stops the operation at its `n`th check, counting from 1.
    #[cfg(test)]
    pub fn stopping_at_check(n: u64) -> Self {
        Self { stop_at_check: Some(Arc::new(AtomicU64::new(n))), ..Self::default() }
    }

    /// Returns `Err(cancelled)` when the caller asked to stop.
    pub fn check(&self) -> crate::Result<()> {
        #[cfg(test)]
        if let Some(left) = &self.stop_at_check {
            if left.fetch_sub(1, Ordering::SeqCst) == 1 {
                self.cancel();
            }
        }
        if self.is_cancelled() {
            Err(crate::VaultError::cancelled())
        } else {
            Ok(())
        }
    }
}

/// Anything that can receive a progress update.
///
/// The engine calls this from the thread doing the work, and from several
/// worker threads while hashing, so an implementation must be cheap and must
/// tolerate being called concurrently.
pub trait ProgressSink<T>: Send + Sync {
    fn emit(&self, update: &T);
}

/// Drops every update. Used by tests that do not assert on progress.
pub struct NullSink;

impl<T> ProgressSink<T> for NullSink {
    fn emit(&self, _update: &T) {}
}

impl<T, F> ProgressSink<T> for F
where
    F: Fn(&T) + Send + Sync,
{
    fn emit(&self, update: &T) {
        self(update)
    }
}

/// Collects every update. Tests use it to assert that progress actually moved.
#[derive(Default)]
pub struct RecordingSink<T> {
    pub updates: std::sync::Mutex<Vec<T>>,
}

impl<T: Clone + Send + Sync> ProgressSink<T> for RecordingSink<T> {
    fn emit(&self, update: &T) {
        if let Ok(mut g) = self.updates.lock() {
            g.push(update.clone());
        }
    }
}

impl<T: Clone> RecordingSink<T> {
    pub fn new() -> Self {
        Self { updates: std::sync::Mutex::new(Vec::new()) }
    }

    pub fn snapshot(&self) -> Vec<T> {
        self.updates.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.updates.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Rate limits progress updates.
///
/// A scan touches tens of thousands of files. Emitting one update per file
/// would flood the IPC channel and slow the scan down. The contract promises at
/// most four updates per second.
pub struct Throttle {
    interval: Duration,
    last: std::sync::Mutex<Instant>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            // Start in the past so the first call always passes.
            last: std::sync::Mutex::new(Instant::now() - interval * 2),
        }
    }

    pub fn per_second(n: u32) -> Self {
        Self::new(Duration::from_millis((1000 / n.max(1)) as u64))
    }

    /// True when enough time passed. Resets the clock when it returns true.
    pub fn ready(&self) -> bool {
        let Ok(mut last) = self.last.lock() else { return false };
        let now = Instant::now();
        if now.duration_since(*last) >= self.interval {
            *last = now;
            true
        } else {
            false
        }
    }

    /// Forces the next [`Throttle::ready`] call to pass. Used for the final
    /// update of an operation, which must never be dropped.
    pub fn force_next(&self) {
        if let Ok(mut last) = self.last.lock() {
            *last = Instant::now() - self.interval * 2;
        }
    }
}

/// Counters several hashing threads share.
#[derive(Debug, Default)]
pub struct Counters {
    pub files_done: AtomicU64,
    pub bytes_done: AtomicU64,
    pub bytes_from_cache: AtomicU64,
    pub errors: AtomicU64,
}

impl Counters {
    pub fn add_file(&self, bytes: u64, from_cache: bool) {
        self.files_done.fetch_add(1, Ordering::Relaxed);
        self.bytes_done.fetch_add(bytes, Ordering::Relaxed);
        if from_cache {
            self.bytes_from_cache.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    pub fn add_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> (u64, u64, u64, u64) {
        (
            self.files_done.load(Ordering::Relaxed),
            self.bytes_done.load(Ordering::Relaxed),
            self.bytes_from_cache.load(Ordering::Relaxed),
            self.errors.load(Ordering::Relaxed),
        )
    }
}

/// Estimates the time left from the work already done.
///
/// Returns `None` until there is enough evidence for a number that is not
/// misleading. A wrong estimate is worse than no estimate.
pub fn estimate_remaining_ms(elapsed_ms: u64, done: u64, total: u64) -> Option<u64> {
    if done == 0 || total == 0 || done >= total || elapsed_ms < 1000 {
        return None;
    }
    let rate = done as f64 / elapsed_ms as f64;
    if rate <= 0.0 {
        return None;
    }
    Some(((total - done) as f64 / rate) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancel_token_shares_its_flag_across_clones() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled(), "cancel did not reach the clone");
        assert_eq!(b.check().unwrap_err().code, crate::ErrorCode::Cancelled);
    }

    #[test]
    fn a_cancel_token_reaches_another_thread() {
        let token = CancelToken::new();
        let worker = token.clone();
        let handle = std::thread::spawn(move || {
            let mut spins = 0u64;
            while !worker.is_cancelled() {
                spins += 1;
                if spins > 200_000_000 {
                    return false; // gave up, the flag never arrived
                }
            }
            true
        });
        std::thread::sleep(Duration::from_millis(20));
        token.cancel();
        assert!(handle.join().unwrap(), "the worker never saw the cancellation");
    }

    #[test]
    fn throttle_drops_updates_inside_its_interval() {
        let t = Throttle::new(Duration::from_millis(250));
        assert!(t.ready(), "the first call must always pass");
        assert!(!t.ready(), "a second call inside the interval must be dropped");
        assert!(!t.ready());
    }

    #[test]
    fn throttle_passes_again_after_the_interval() {
        let t = Throttle::new(Duration::from_millis(30));
        assert!(t.ready());
        std::thread::sleep(Duration::from_millis(45));
        assert!(t.ready(), "the throttle never reopened");
    }

    #[test]
    fn force_next_lets_the_final_update_through() {
        // The last update of an operation carries the final totals. Losing it to
        // the throttle would leave the interface showing a stale number.
        let t = Throttle::new(Duration::from_secs(10));
        assert!(t.ready());
        assert!(!t.ready());
        t.force_next();
        assert!(t.ready(), "the final update was dropped");
    }

    #[test]
    fn a_closure_works_as_a_sink() {
        let seen = std::sync::Arc::new(AtomicU64::new(0));
        let s = seen.clone();
        let sink = move |v: &u64| {
            s.fetch_add(*v, Ordering::SeqCst);
        };
        ProgressSink::emit(&sink, &3);
        ProgressSink::emit(&sink, &4);
        assert_eq!(seen.load(Ordering::SeqCst), 7);
    }

    #[test]
    fn recording_sink_keeps_every_update_in_order() {
        let sink: RecordingSink<u32> = RecordingSink::new();
        sink.emit(&1);
        sink.emit(&2);
        assert_eq!(sink.snapshot(), vec![1, 2]);
    }

    #[test]
    fn counters_total_correctly_across_threads() {
        let c = std::sync::Arc::new(Counters::default());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let c = c.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    c.add_file(10, false);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let (files, bytes, _, _) = c.snapshot();
        assert_eq!(files, 8000);
        assert_eq!(bytes, 80_000);
    }

    #[test]
    fn estimate_stays_silent_until_it_has_evidence() {
        assert_eq!(estimate_remaining_ms(0, 0, 100), None, "no work done yet");
        assert_eq!(estimate_remaining_ms(500, 10, 100), None, "too early to be honest");
        assert_eq!(estimate_remaining_ms(2000, 100, 100), None, "already finished");
        assert_eq!(estimate_remaining_ms(2000, 0, 100), None);
    }

    #[test]
    fn estimate_is_proportional_once_it_speaks() {
        // Half the work took 2 s, so the other half is about 2 s.
        let got = estimate_remaining_ms(2000, 50, 100).unwrap();
        assert!((1900..=2100).contains(&got), "got {got}");
    }
}
