//! Honest facts about this Rust process: resident memory, threads, descriptors
//! and CPU from `/proc` on Linux, Tokio's own scheduler counters, and the
//! database pool. A fact the platform cannot report is `None`, never zero.
use crate::sample::Process;
use bilibi_database::Db;

/// Linux reports process CPU in clock ticks; USER_HZ is 100 on every
/// mainstream architecture, so a tick is 10 ms.
const MS_PER_TICK: i64 = 10;

/// One reading. The `/proc` reads are small but blocking, so they run off the
/// async workers.
pub async fn snapshot(db: &Db) -> Process {
    let mut process = tokio::task::spawn_blocking(proc_facts)
        .await
        .unwrap_or_default();
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let metrics = handle.metrics();
        process.run_queue = i64::try_from(metrics.global_queue_depth()).ok();
        process.alive_tasks = i64::try_from(metrics.num_alive_tasks()).ok();
    }
    let status = db.status();
    process.pool_in_use = i64::try_from(status.size.saturating_sub(status.available)).ok();
    process.pool_waiting = i64::try_from(status.waiting).ok();
    process
}

#[cfg(target_os = "linux")]
fn proc_facts() -> Process {
    let mut process = Process::default();
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let kilobytes = |field: &str| -> Option<i64> {
            status
                .lines()
                .find_map(|line| line.strip_prefix(field))?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        };
        process.memory_bytes = kilobytes("VmRSS:").map(|kb| kb.saturating_mul(1024));
        process.threads = kilobytes("Threads:");
    }
    process.open_fds = std::fs::read_dir("/proc/self/fd")
        .ok()
        .and_then(|entries| i64::try_from(entries.count()).ok());
    process.cpu_ms = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|stat| {
            // The command name is parenthesised and may hold spaces; the
            // numbered fields start after its last ")". utime and stime are
            // fields 14 and 15, i.e. 11 and 12 after the state field.
            let fields: Vec<_> = stat[stat.rfind(')')? + 1..].split_whitespace().collect();
            let ticks = |index: usize| fields.get(index)?.parse::<i64>().ok();
            Some((ticks(11)? + ticks(12)?).saturating_mul(MS_PER_TICK))
        });
    process
}

#[cfg(not(target_os = "linux"))]
fn proc_facts() -> Process {
    Process::default()
}
