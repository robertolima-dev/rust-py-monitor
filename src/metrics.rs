/// Core data types for the library.
///
/// `SnapshotData` is a plain Rust type with no PyO3 dependency.
/// Keeping it separate from the Python-facing types makes the collection
/// logic independently testable without a running Python interpreter.
#[derive(Debug, Clone)]
pub struct SnapshotData {
    pub pid: u32,
    /// CPU usage of this process (0.0 – 100.0 * number of cores).
    /// Measured as the delta since the previous `snapshot()` call (the
    /// underlying `System` is kept alive between calls). The very first call
    /// after process start returns ~0.0 since there is no prior sample.
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
