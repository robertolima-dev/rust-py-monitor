use std::time::{SystemTime, UNIX_EPOCH};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::metrics::SnapshotData;

/// Collects a snapshot of the current process.
///
/// Returns `Result<SnapshotData, String>` instead of calling `.unwrap()`.
/// If the process is not found (unlikely but possible in containerized
/// environments with PID namespaces), we return a descriptive error that
/// the Python side converts into a `RuntimeError` exception.
pub fn collect() -> Result<SnapshotData, String> {
    // `std::process::id()` returns the PID of the Python process that imported us.
    let pid = std::process::id();

    // Create an empty `System` and request only our process's info.
    // `ProcessRefreshKind::nothing().with_memory().with_cpu()` avoids
    // scanning all OS processes, reducing overhead.
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        false,
        ProcessRefreshKind::nothing().with_memory().with_cpu(),
    );

    // `SystemTime::now()` can fail if the system clock is before UNIX_EPOCH
    // (impossible in practice, but the compiler forces us to handle it).
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("clock error: {}", e))?
        .as_secs();

    // `.ok_or_else(|| ...)` converts `Option` into `Result`.
    // `?` propagates the error upward — ownership of the String goes to the caller.
    let process = sys
        .process(Pid::from_u32(pid))
        .ok_or_else(|| format!("process {} not found by sysinfo", pid))?;

    // Thread count: `tasks()` returns threads on Linux.
    // On macOS/Windows it returns None — we use 0 as a sentinel in those cases.
    let threads = process
        .tasks()
        .map(|tasks| tasks.len() as u64)
        .unwrap_or(0);

    Ok(SnapshotData {
        pid,
        cpu_percent: process.cpu_usage() as f64,
        memory_rss: process.memory(),
        memory_virtual: process.virtual_memory(),
        threads,
        timestamp,
    })
}

// `#[cfg(test)]` ensures this block only exists in the test binary,
// not in the .so shipped to Python.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_returns_valid_pid() {
        let snap = collect().expect("snapshot should succeed");
        assert_eq!(snap.pid, std::process::id());
    }

    #[test]
    fn snapshot_rss_memory_is_nonzero() {
        let snap = collect().expect("snapshot should succeed");
        assert!(snap.memory_rss > 0, "RSS must be > 0, got: {}", snap.memory_rss);
    }

    #[test]
    fn snapshot_timestamp_is_reasonable() {
        let snap = collect().expect("snapshot should succeed");
        // Timestamp must be after 2024-01-01 (Unix 1704067200).
        assert!(snap.timestamp > 1_704_067_200, "timestamp looks like it's in the past");
    }

    #[test]
    fn snapshot_cpu_percent_is_nonnegative() {
        let snap = collect().expect("snapshot should succeed");
        // First call may return 0.0 (sysinfo needs two samples).
        // Must never be negative.
        assert!(snap.cpu_percent >= 0.0);
    }
}
