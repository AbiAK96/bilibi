//! Database time attributed to whatever is being observed (a request, a live
//! event, a job). Perf installs a `Counters` around the observed work with
//! `scope`; the audited write boundary and `timed` add to it. Nothing else here
//! reads clocks or touches the database, and outside a scope every call is a no-op.
//!
//! tokio-postgres has no telemetry hook, so this counts audited transactions
//! (begin, owner write plus capture, record, commit) and any query a caller
//! wraps in `timed`. Wrap a hot read path in `timed` rather than reading the
//! clock by hand.
use std::{
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

tokio::task_local! {
    static ACTIVE: Arc<Counters>;
}

/// Totals for one observation. Lock-free and safe to share.
#[derive(Default)]
pub struct Counters {
    count: AtomicU64,
    micros: AtomicU64,
}
impl Counters {
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
    /// Whole milliseconds, rounded up so that a measured query never reads as free.
    pub fn millis(&self) -> u64 {
        self.micros.load(Ordering::Relaxed).div_ceil(1000)
    }
}

/// Runs `work` with `counters` active for every `note` made by it, including
/// awaits on the same task. A task spawned by `work` is not covered.
pub fn scope<F: Future>(counters: Arc<Counters>, work: F) -> impl Future<Output = F::Output> {
    ACTIVE.scope(counters, work)
}

/// Adds one database round trip of `elapsed` to the active scope, if any.
pub fn note(elapsed: Duration) {
    let _ = ACTIVE.try_with(|counters| {
        counters.count.fetch_add(1, Ordering::Relaxed);
        // A task would need centuries of database time to wrap this.
        let micros = u64::try_from(elapsed.as_micros()).unwrap_or(0);
        counters.micros.fetch_add(micros, Ordering::Relaxed);
    });
}

/// Awaits `query` and notes its wall time, whether it succeeds or fails.
pub async fn timed<T>(query: impl Future<Output = T>) -> T {
    let started = Instant::now();
    let result = query.await;
    note(started.elapsed());
    result
}
