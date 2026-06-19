// Multi-worker metric aggregation via shared shard files.
//
// Problem: in a gunicorn/uvicorn deployment with N worker processes, each
// process has its own in-memory store (the `static STORE` in request_metrics).
// A Prometheus scrape of `/metrics` reaches only *one* worker, so it sees only
// that worker's slice of the traffic and the numbers oscillate per scrape.
//
// Solution (opt-in via the `RPY_MULTIPROC_DIR` env var, mirroring the
// `prometheus_client` multiprocess design): every worker memory-maps a small,
// fixed-size shard file named `rpy-<pid>.shard` and updates it on each request.
// At scrape time the renderer merges *all* shard files in the directory.
//
// Mergeability is the key constraint. Counters (totals, errors) sum trivially.
// Latency cannot be merged from per-process percentiles, so we record a
// *histogram* of latency buckets (counts per bucket), which is mergeable, and
// derive approximate p50/p95/p99 from the merged buckets at read time — exactly
// what `histogram_quantile` does in PromQL.
//
// Concurrency: each worker writes only to its own shard, so there is no
// cross-process write contention. The shard is an overlay of `AtomicU64`
// fields, so reads from other processes are per-field consistent (aligned
// 64-bit atomics) without any cross-process lock. Whole-snapshot consistency is
// not guaranteed, but monitoring tolerates eventual consistency.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use memmap2::{Mmap, MmapMut, MmapOptions};

// ---------------------------------------------------------------------------
// Shard layout
//
// The shard file is a flat array of `AtomicU64`. Fixed indices name the fields.
// ---------------------------------------------------------------------------

const FORMAT_VERSION: u64 = 1;

const I_VERSION: usize = 0;
const I_PID: usize = 1;
const I_TOTAL: usize = 2;
const I_ERRORS: usize = 3;
const I_SUM_US: usize = 4;
const I_MIN_US: usize = 5;
const I_MAX_US: usize = 6;
const I_HEARTBEAT: usize = 7;
const I_BUCKETS_START: usize = 8;

/// Upper bounds (inclusive, in milliseconds) of the latency histogram buckets.
/// A final implicit overflow bucket counts everything above the last bound.
pub const BUCKET_BOUNDS_MS: [f64; 13] = [
    1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0,
];

/// Number of histogram buckets = finite bounds + 1 overflow bucket.
pub const NUM_BUCKETS: usize = BUCKET_BOUNDS_MS.len() + 1;

/// Total number of u64 slots, and the resulting file size in bytes.
const SHARD_LEN_U64: usize = I_BUCKETS_START + NUM_BUCKETS;
const SHARD_BYTES: usize = SHARD_LEN_U64 * 8;

/// Sentinel stored in the min-latency slot when no sample has been recorded.
const MIN_EMPTY: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Per-process state
// ---------------------------------------------------------------------------

enum State {
    /// Not yet initialized — env var not read yet.
    Uninit,
    /// Multiproc disabled (env var unset/empty).
    Disabled,
    /// Multiproc enabled: directory + this process's writable shard.
    Enabled { dir: PathBuf, writer: MmapMut },
}

static STATE: Mutex<State> = Mutex::new(State::Uninit);

/// Reinterprets a byte slice as a slice of `AtomicU64`.
///
/// Safety: the backing memory comes from `mmap`, which is page-aligned (so the
/// 8-byte alignment of `AtomicU64` is satisfied) and `len` is a multiple of 8.
/// `AtomicU64` has the same layout as `u64`; concurrent access is exactly what
/// atomics are for.
fn as_atomics(bytes: &[u8]) -> &[AtomicU64] {
    debug_assert_eq!(bytes.len() % 8, 0);
    debug_assert_eq!(bytes.as_ptr() as usize % 8, 0);
    unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const AtomicU64, bytes.len() / 8) }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Maps the bucket index for a given latency in milliseconds.
fn bucket_index(duration_ms: f64) -> usize {
    for (i, &bound) in BUCKET_BOUNDS_MS.iter().enumerate() {
        if duration_ms <= bound {
            return i;
        }
    }
    NUM_BUCKETS - 1 // overflow bucket
}

/// Opens (creating if needed) this process's shard and initializes its header.
fn open_writer(dir: &Path) -> std::io::Result<MmapMut> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("rpy-{}.shard", std::process::id()));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    file.set_len(SHARD_BYTES as u64)?;
    let mmap = unsafe { MmapOptions::new().len(SHARD_BYTES).map_mut(&file)? };

    let slots = as_atomics(&mmap);
    // A freshly created file is zero-filled (version == 0). Initialize the
    // header and reset counters. If the file already carries our format and pid
    // (e.g. re-entry within the same process), we leave the counters intact.
    let same = slots[I_VERSION].load(Ordering::Relaxed) == FORMAT_VERSION
        && slots[I_PID].load(Ordering::Relaxed) == std::process::id() as u64;
    if !same {
        for slot in slots.iter() {
            slot.store(0, Ordering::Relaxed);
        }
        slots[I_VERSION].store(FORMAT_VERSION, Ordering::Relaxed);
        slots[I_PID].store(std::process::id() as u64, Ordering::Relaxed);
        slots[I_MIN_US].store(MIN_EMPTY, Ordering::Relaxed);
        slots[I_HEARTBEAT].store(now_secs(), Ordering::Relaxed);
    }
    Ok(mmap)
}

/// Lazily initializes state from the environment on first access.
fn ensure_init(state: &mut State) {
    if let State::Uninit = state {
        match std::env::var("RPY_MULTIPROC_DIR") {
            Ok(dir) if !dir.trim().is_empty() => {
                let dir = PathBuf::from(dir);
                match open_writer(&dir) {
                    Ok(writer) => *state = State::Enabled { dir, writer },
                    // If we can't set up the shard, degrade to disabled rather
                    // than crash the host application.
                    Err(_) => *state = State::Disabled,
                }
            }
            _ => *state = State::Disabled,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API used by request_metrics / aggregator
// ---------------------------------------------------------------------------

/// Records one request into this process's shard, if multiproc is enabled.
/// Cheap: a handful of relaxed atomic ops on mapped memory, no syscalls.
pub fn record(status_code: u16, duration_ms: f64) {
    let mut state = STATE.lock().unwrap_or_else(|p| p.into_inner());
    ensure_init(&mut state);
    if let State::Enabled { writer, .. } = &mut *state {
        let slots = as_atomics(writer);
        let us = (duration_ms.max(0.0) * 1000.0) as u64;

        slots[I_TOTAL].fetch_add(1, Ordering::Relaxed);
        if status_code >= 400 {
            slots[I_ERRORS].fetch_add(1, Ordering::Relaxed);
        }
        slots[I_SUM_US].fetch_add(us, Ordering::Relaxed);
        slots[I_MIN_US].fetch_min(us, Ordering::Relaxed);
        slots[I_MAX_US].fetch_max(us, Ordering::Relaxed);
        slots[I_BUCKETS_START + bucket_index(duration_ms)].fetch_add(1, Ordering::Relaxed);
        slots[I_HEARTBEAT].store(now_secs(), Ordering::Relaxed);
    }
}

/// Merged view across all live shards. `None` when multiproc is disabled, in
/// which case callers fall back to the in-process exact computation.
pub struct Merged {
    pub total: u64,
    pub errors: u64,
    pub sum_us: u64,
    /// `u64::MAX` when no samples were recorded.
    pub min_us: u64,
    pub max_us: u64,
    pub buckets: [u64; NUM_BUCKETS],
}

/// Reads and merges every live shard in the multiproc directory.
/// Returns `None` if multiproc is disabled.
pub fn merged() -> Option<Merged> {
    let dir = {
        let mut state = STATE.lock().unwrap_or_else(|p| p.into_inner());
        ensure_init(&mut state);
        match &*state {
            State::Enabled { dir, .. } => dir.clone(),
            _ => return None,
        }
    };

    let mut m = Merged {
        total: 0,
        errors: 0,
        sum_us: 0,
        min_us: MIN_EMPTY,
        max_us: 0,
        buckets: [0; NUM_BUCKETS],
    };

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Some(m), // dir vanished — report zeros, never crash
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_shard = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("rpy-") && n.ends_with(".shard"))
            .unwrap_or(false);
        if !is_shard {
            continue;
        }
        merge_one(&path, &mut m);
    }

    Some(m)
}

/// Merges a single shard file into `m`, pruning it if its owner is dead.
fn merge_one(path: &Path, m: &mut Merged) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    // Guard against truncated/garbage files.
    match file.metadata() {
        Ok(meta) if meta.len() as usize >= SHARD_BYTES => {}
        _ => return,
    }
    let mmap = match unsafe { Mmap::map(&file) } {
        Ok(mm) => mm,
        Err(_) => return,
    };
    let slots = as_atomics(&mmap[..SHARD_BYTES]);

    if slots[I_VERSION].load(Ordering::Relaxed) != FORMAT_VERSION {
        return;
    }

    let pid = slots[I_PID].load(Ordering::Relaxed) as u32;
    if !pid_alive(pid) {
        // Owner is gone: drop its mapping and remove the stale file so dead
        // workers don't accumulate. Best-effort.
        drop(mmap);
        let _ = std::fs::remove_file(path);
        return;
    }

    m.total += slots[I_TOTAL].load(Ordering::Relaxed);
    m.errors += slots[I_ERRORS].load(Ordering::Relaxed);
    m.sum_us = m.sum_us.saturating_add(slots[I_SUM_US].load(Ordering::Relaxed));

    let min = slots[I_MIN_US].load(Ordering::Relaxed);
    if min != MIN_EMPTY {
        m.min_us = m.min_us.min(min);
    }
    m.max_us = m.max_us.max(slots[I_MAX_US].load(Ordering::Relaxed));

    for (i, b) in m.buckets.iter_mut().enumerate() {
        *b += slots[I_BUCKETS_START + i].load(Ordering::Relaxed);
    }
}

/// Approximate quantile from merged histogram buckets (à la `histogram_quantile`).
/// `max_ms` is used for the open-ended overflow bucket. Returns 0.0 for an
/// empty histogram.
pub fn histogram_quantile(q: f64, buckets: &[u64; NUM_BUCKETS], max_ms: f64) -> f64 {
    let total: u64 = buckets.iter().sum();
    if total == 0 {
        return 0.0;
    }
    let rank = q * total as f64;
    let mut cum_before: u64 = 0;
    for i in 0..NUM_BUCKETS {
        let count = buckets[i];
        let cum = cum_before + count;
        if cum as f64 >= rank {
            let lower = if i == 0 { 0.0 } else { BUCKET_BOUNDS_MS[i - 1] };
            // Overflow bucket has no finite upper bound: use observed max.
            let upper = if i < BUCKET_BOUNDS_MS.len() {
                BUCKET_BOUNDS_MS[i]
            } else {
                max_ms.max(lower)
            };
            if count == 0 {
                return lower;
            }
            let frac = (rank - cum_before as f64) / count as f64;
            return lower + (upper - lower) * frac;
        }
        cum_before = cum;
    }
    max_ms
}

// ---------------------------------------------------------------------------
// Liveness check
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // kill(pid, 0) sends no signal but performs the permission/existence check.
    // 0 => process exists and we may signal it.
    // EPERM => process exists but we lack permission (still alive).
    // ESRCH => no such process (dead).
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if ret == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    // No portable liveness check; keep all shards (no auto-pruning).
    true
}

// ---------------------------------------------------------------------------
// Runtime configuration (also drives tests)
// ---------------------------------------------------------------------------

/// Enables multiproc mode pointing at `dir`, or disables it when `None`.
/// Resets this process's shard. Intended for explicit setup and tests; the
/// usual path is the `RPY_MULTIPROC_DIR` environment variable.
pub fn set_dir(dir: Option<&str>) {
    let mut state = STATE.lock().unwrap_or_else(|p| p.into_inner());
    match dir {
        Some(d) if !d.trim().is_empty() => {
            let dir = PathBuf::from(d);
            match open_writer(&dir) {
                Ok(writer) => *state = State::Enabled { dir, writer },
                Err(_) => *state = State::Disabled,
            }
        }
        _ => *state = State::Disabled,
    }
}

/// Returns the active multiproc directory, or `None` if disabled.
pub fn dir() -> Option<String> {
    let mut state = STATE.lock().unwrap_or_else(|p| p.into_inner());
    ensure_init(&mut state);
    match &*state {
        State::Enabled { dir, .. } => Some(dir.to_string_lossy().into_owned()),
        _ => None,
    }
}

/// True when multiproc aggregation is active.
pub fn enabled() -> bool {
    dir().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rpy-mp-test-{}-{}-{}",
            tag,
            std::process::id(),
            now_secs()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_fake_shard(path: &Path, pid: u32, total: u64, errors: u64, bucket: usize) {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        file.set_len(SHARD_BYTES as u64).unwrap();
        let mmap = unsafe { MmapOptions::new().len(SHARD_BYTES).map_mut(&file).unwrap() };
        let slots = as_atomics(&mmap);
        slots[I_VERSION].store(FORMAT_VERSION, Ordering::Relaxed);
        slots[I_PID].store(pid as u64, Ordering::Relaxed);
        slots[I_TOTAL].store(total, Ordering::Relaxed);
        slots[I_ERRORS].store(errors, Ordering::Relaxed);
        slots[I_MIN_US].store(5_000, Ordering::Relaxed);
        slots[I_MAX_US].store(5_000, Ordering::Relaxed);
        slots[I_BUCKETS_START + bucket].store(total, Ordering::Relaxed);
        mmap.flush().unwrap();
    }

    #[test]
    #[serial]
    fn disabled_by_default_returns_none() {
        set_dir(None);
        assert!(merged().is_none());
        assert!(!enabled());
    }

    #[test]
    #[serial]
    fn records_into_own_shard() {
        let dir = tmpdir("own");
        set_dir(Some(dir.to_str().unwrap()));
        record(200, 10.0);
        record(500, 20.0);
        let m = merged().expect("enabled");
        assert_eq!(m.total, 2);
        assert_eq!(m.errors, 1);
        assert_eq!(m.min_us, 10_000);
        assert_eq!(m.max_us, 20_000);
        set_dir(None);
    }

    #[test]
    #[serial]
    fn merges_multiple_live_shards() {
        let dir = tmpdir("merge");
        set_dir(Some(dir.to_str().unwrap()));
        record(200, 10.0); // our own shard: 1 request

        // A second "worker" whose pid is alive (reuse our pid in the field).
        let alive = std::process::id();
        write_fake_shard(&dir.join("rpy-other.shard"), alive, 4, 2, 3);

        let m = merged().expect("enabled");
        assert_eq!(m.total, 5); // 1 + 4
        assert_eq!(m.errors, 2); // 0 + 2
        set_dir(None);
    }

    #[test]
    #[serial]
    fn prunes_dead_worker_shard() {
        let dir = tmpdir("dead");
        set_dir(Some(dir.to_str().unwrap()));
        record(200, 10.0);

        // A very high pid that is positive (so `kill` doesn't interpret it as a
        // process group) but practically never a live process → pruned. pid 0
        // would target the whole process group, and negative pids are groups.
        let dead_pid = 1_000_000_000u32;
        let dead_path = dir.join("rpy-dead.shard");
        write_fake_shard(&dead_path, dead_pid, 999, 999, 5);

        let m = merged().expect("enabled");
        assert_eq!(m.total, 1, "dead worker's counts must be excluded");
        assert!(!dead_path.exists(), "dead shard should be removed");
        set_dir(None);
    }

    #[test]
    fn histogram_quantile_basic() {
        // 100 samples all in the 5..=10ms bucket (index 3, bounds 5..10).
        let mut b = [0u64; NUM_BUCKETS];
        b[3] = 100;
        let p95 = histogram_quantile(0.95, &b, 10.0);
        assert!(p95 >= 5.0 && p95 <= 10.0, "p95={}", p95);
        // empty
        assert_eq!(histogram_quantile(0.95, &[0; NUM_BUCKETS], 0.0), 0.0);
    }

    #[test]
    fn bucket_index_boundaries() {
        assert_eq!(bucket_index(0.5), 0); // <= 1ms
        assert_eq!(bucket_index(1.0), 0);
        assert_eq!(bucket_index(1.5), 1); // <= 2ms
        assert_eq!(bucket_index(10_000.0), 12); // <= 10000ms
        assert_eq!(bucket_index(99_999.0), NUM_BUCKETS - 1); // overflow
    }
}
