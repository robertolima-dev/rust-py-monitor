/// Core data types for the library.
///
/// `SnapshotData` is a plain Rust type with no PyO3 dependency.
/// Keeping it separate from the Python-facing types makes the collection
/// logic independently testable without a running Python interpreter.
#[derive(Debug, Clone)]
pub struct SnapshotData {
    pub pid: u32,
    /// CPU usage of this process (0.0 – 100.0 * number of cores).
    /// Note: the first reading may be 0.0 because sysinfo needs two samples
    /// to compute the delta. Call snapshot() twice with a short interval for
    /// stable readings in production.
    pub cpu_percent: f64,
    /// RSS (Resident Set Size) in bytes — memory actually in RAM.
    pub memory_rss: u64,
    /// Total virtual memory reserved by the process, in bytes.
    pub memory_virtual: u64,
    /// Thread count (platform-dependent).
    pub threads: u64,
    /// Unix timestamp in seconds.
    pub timestamp: u64,
}
