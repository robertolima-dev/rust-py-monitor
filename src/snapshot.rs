use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::metrics::SnapshotData;

// Persistent state reused across calls.
//
// CPU usage is a *rate*: CPU time consumed per unit of wall-clock time. It only
// has meaning between two points in time, so we must remember the previous
// sample. We keep:
//   - a `System` (reused so we don't reallocate sysinfo's internals each call),
//   - the previous `(cpu_time_ms, Instant)` pair to compute the delta.
//
// Why not `process.cpu_usage()`? On macOS, sysinfo 0.35's `cpu_usage()` relies
// on a global per-core CPU-load reading (`host_processor_info`) that does not
// yield a usable interval here, so it returns 0.0 regardless of refresh
// pattern. `accumulated_cpu_time()` (total CPU time used by the process) is
// reliable, so we derive the percentage ourselves:
//
//   cpu_percent = (cpu_time_delta_ms / wall_clock_delta_ms) * 100
//
// This is the same definition `top` uses and can exceed 100% on multi-threaded
// workloads (one busy thread per core ≈ 100% each), which is intended.
struct State {
    system: System,
    /// Previous (accumulated CPU time in ms, instant it was sampled).
    prev: Option<(u64, Instant)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Collects a snapshot of the current process.
///
/// Returns `Result<SnapshotData, String>` instead of calling `.unwrap()`.
/// If the process is not found (unlikely but possible in containerized
/// environments with PID namespaces), we return a descriptive error that
/// the Python side converts into a `RuntimeError` exception.
///
/// Note on CPU: the *first* call after process start returns 0.0 because there
/// is no previous sample to diff against. Subsequent calls report real usage.
pub fn collect() -> Result<SnapshotData, String> {
    // `std::process::id()` returns the PID of the Python process that imported us.
    let pid = std::process::id();

    // `SystemTime::now()` can fail if the system clock is before UNIX_EPOCH
    // (impossible in practice, but the compiler forces us to handle it).
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("clock error: {}", e))?
        .as_secs();

    // Recover from a poisoned lock instead of panicking — never crash the host.
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let state = guard.get_or_insert_with(|| State {
        system: System::new(),
        prev: None,
    });

    // Refresh only our process's info (cpu + memory). Avoids scanning all OS
    // processes.
    state.system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        false,
        ProcessRefreshKind::nothing().with_memory().with_cpu(),
    );

    let now = Instant::now();

    // `.ok_or_else(|| ...)` converts `Option` into `Result`.
    // `?` propagates the error upward — ownership of the String goes to the caller.
    let process = state
        .system
        .process(Pid::from_u32(pid))
        .ok_or_else(|| format!("process {} not found by sysinfo", pid))?;

    // Thread count: `tasks()` returns threads on Linux.
    // On macOS/Windows it returns None — we use 0 as a sentinel in those cases.
    let threads = process
        .tasks()
        .map(|tasks| tasks.len() as u64)
        .unwrap_or(0);

    let cpu_time_ms = process.accumulated_cpu_time();
    let memory_rss = process.memory();
    let memory_virtual = process.virtual_memory();

    // Compute CPU% from the delta against the previous sample.
    let cpu_percent = match state.prev {
        Some((prev_cpu_ms, prev_instant)) => {
            let wall_ms = now.duration_since(prev_instant).as_secs_f64() * 1000.0;
            let cpu_delta_ms = cpu_time_ms.saturating_sub(prev_cpu_ms) as f64;
            if wall_ms > 0.0 {
                (cpu_delta_ms / wall_ms) * 100.0
            } else {
                0.0
            }
        }
        // First call: no baseline yet.
        None => 0.0,
    };
    state.prev = Some((cpu_time_ms, now));

    Ok(SnapshotData {
        pid,
        cpu_percent,
        memory_rss,
        memory_virtual,
        threads,
        timestamp,
    })
}

// `#[cfg(test)]` ensures this block only exists in the test binary,
// not in the .so shipped to Python.
#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    // All snapshot tests share the global `STATE` (CPU baseline). `#[serial]`
    // prevents them from interleaving, which would corrupt the CPU delta.

    #[test]
    #[serial]
    fn snapshot_returns_valid_pid() {
        let snap = collect().expect("snapshot should succeed");
        assert_eq!(snap.pid, std::process::id());
    }

    #[test]
    #[serial]
    fn snapshot_rss_memory_is_nonzero() {
        let snap = collect().expect("snapshot should succeed");
        assert!(snap.memory_rss > 0, "RSS must be > 0, got: {}", snap.memory_rss);
    }

    #[test]
    #[serial]
    fn snapshot_timestamp_is_reasonable() {
        let snap = collect().expect("snapshot should succeed");
        // Timestamp must be after 2024-01-01 (Unix 1704067200).
        assert!(snap.timestamp > 1_704_067_200, "timestamp looks like it's in the past");
    }

    #[test]
    #[serial]
    fn snapshot_cpu_percent_is_nonnegative() {
        let snap = collect().expect("snapshot should succeed");
        // First call returns 0.0 (no baseline). Must never be negative.
        assert!(snap.cpu_percent >= 0.0);
    }

    #[test]
    #[serial]
    fn snapshot_cpu_percent_reflects_busy_work() {
        // Establish a baseline, then burn CPU on this thread.
        let _ = collect().expect("baseline snapshot");
        let t = Instant::now();
        let mut x: u64 = 0;
        while t.elapsed().as_millis() < 300 {
            x = x.wrapping_add(1);
        }
        std::hint::black_box(x);

        let snap = collect().expect("second snapshot");
        // A thread pegged for ~300ms should report clearly non-zero CPU.
        // We assert a conservative floor to avoid flakiness on loaded CI.
        assert!(
            snap.cpu_percent > 10.0,
            "expected busy CPU%, got {}",
            snap.cpu_percent
        );
    }
}
