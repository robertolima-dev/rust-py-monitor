// Global storage for HTTP request metrics.
//
// `static` in Rust is a variable that lives for the entire program lifetime
// (equivalent to a global). To allow safe mutable access across threads,
// we wrap it in `Mutex<T>`.
//
// `VecDeque::new()` / `Mutex::new()` / `AtomicUsize::new()` are all `const fn`,
// so they compile in a `static` context — no need for `lazy_static`/`OnceLock`.
//
// We use a *bounded* ring buffer (`VecDeque` capped at `MAX_REQUESTS`) instead
// of an unbounded `Vec`. A monitoring library must never grow without bound:
// an unbounded store would leak memory proportionally to traffic and eventually
// OOM the very app it is monitoring. Capping also keeps every read (aggregate /
// Prometheus scrape) O(cap) instead of O(total-requests-ever).
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default maximum number of requests kept in the store.
/// Oldest entries are evicted first once this is exceeded.
pub const DEFAULT_MAX_REQUESTS: usize = 10_000;

/// Represents a single HTTP request captured by the middleware.
/// Plain Rust type — no PyO3. The conversion to Python lives in lib.rs.
#[derive(Debug, Clone)]
pub struct RequestMetric {
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub duration_ms: f64,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Global store
//
// `Mutex<Vec<T>>` guarantees mutual exclusion: only one thread at a time
// can read or write. In a FastAPI context (async, single thread per worker)
// contention is low, but correctness is total.
//
// If the mutex becomes "poisoned" (a thread panicked while holding it),
// we recover the inner data with `into_inner()` instead of dropping the
// write — a monitoring library must never crash, nor silently stop recording,
// the app it is monitoring.
// ---------------------------------------------------------------------------
static STORE: Mutex<VecDeque<RequestMetric>> = Mutex::new(VecDeque::new());

/// Current capacity of the ring buffer. Tunable at runtime via
/// `set_max_requests()` (exposed to Python in lib.rs).
static MAX_REQUESTS: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_REQUESTS);

/// Locks the store, recovering from poisoning rather than panicking.
fn lock_store() -> std::sync::MutexGuard<'static, VecDeque<RequestMetric>> {
    STORE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Sets the maximum number of requests retained. Values < 1 are clamped to 1.
/// If the new cap is smaller than the current length, the oldest entries are
/// evicted immediately.
pub fn set_max_requests(max: usize) {
    let max = max.max(1);
    MAX_REQUESTS.store(max, Ordering::Relaxed);
    let mut store = lock_store();
    while store.len() > max {
        store.pop_front();
    }
}

/// Returns the current retention capacity.
pub fn max_requests() -> usize {
    MAX_REQUESTS.load(Ordering::Relaxed)
}

/// Records a new request in the global store.
/// Called by the Python middleware on every request.
pub fn record(method: &str, path: &str, status_code: u16, duration_ms: f64) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let metric = RequestMetric {
        // `.to_owned()` converts `&str` (borrowed reference) into `String`
        // (owned value). The store must own the data so it outlives the caller.
        method: method.to_owned(),
        path: path.to_owned(),
        status_code,
        duration_ms,
        timestamp,
    };

    // In multi-worker mode, also fold this request into the shared shard so a
    // Prometheus scrape on any worker can aggregate across all of them. No-op
    // (cheap atomics check) when multiproc is disabled.
    crate::multiproc::record(status_code, duration_ms);

    let cap = MAX_REQUESTS.load(Ordering::Relaxed);
    let mut store = lock_store();
    store.push_back(metric);
    // Evict oldest entries past the cap. This is a `while` (not an `if`) so a
    // shrunk cap is honored even if several entries are over the limit.
    while store.len() > cap {
        store.pop_front();
    }
}

/// Returns a clone of all recorded metrics, oldest first.
/// `.cloned()` is required because we release the lock before returning.
pub fn get_all() -> Vec<RequestMetric> {
    lock_store().iter().cloned().collect()
}

/// Clears the store — useful for tests and manual resets.
pub fn clear() {
    lock_store().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn cleanup() {
        clear();
    }

    #[test]
    #[serial]
    fn record_and_get_all() {
        cleanup();
        record("GET", "/ping", 200, 12.5);
        record("POST", "/users", 201, 45.0);

        let all = get_all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].method, "GET");
        assert_eq!(all[0].path, "/ping");
        assert_eq!(all[0].status_code, 200);
        assert!(all[0].duration_ms > 0.0);
        assert_eq!(all[1].status_code, 201);
        cleanup();
    }

    #[test]
    #[serial]
    fn clear_empties_store() {
        cleanup();
        record("DELETE", "/item/1", 204, 8.0);
        clear();
        assert_eq!(get_all().len(), 0);
    }

    #[test]
    #[serial]
    fn store_is_bounded_by_capacity() {
        cleanup();
        set_max_requests(5);
        for i in 0..20 {
            record("GET", "/x", 200, i as f64);
        }
        let all = get_all();
        // Never grows past the cap...
        assert_eq!(all.len(), 5);
        // ...and keeps the most recent entries (durations 15..=19), oldest first.
        assert_eq!(all[0].duration_ms, 15.0);
        assert_eq!(all[4].duration_ms, 19.0);
        // restore default for other tests
        set_max_requests(DEFAULT_MAX_REQUESTS);
        cleanup();
    }

    #[test]
    #[serial]
    fn shrinking_cap_evicts_oldest_immediately() {
        cleanup();
        set_max_requests(DEFAULT_MAX_REQUESTS);
        for i in 0..10 {
            record("GET", "/x", 200, i as f64);
        }
        set_max_requests(3);
        let all = get_all();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].duration_ms, 7.0);
        assert_eq!(all[2].duration_ms, 9.0);
        set_max_requests(DEFAULT_MAX_REQUESTS);
        cleanup();
    }

    #[test]
    #[serial]
    fn timestamp_is_populated() {
        cleanup();
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        record("GET", "/ts", 200, 1.0);
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let ts = get_all()[0].timestamp;
        assert!(ts >= before && ts <= after);
        cleanup();
    }
}
