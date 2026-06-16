// Global storage for HTTP request metrics.
//
// `static` in Rust is a variable that lives for the entire program lifetime
// (equivalent to a global). To allow safe mutable access across threads,
// we wrap it in `Mutex<T>`.
//
// `Mutex::new(Vec::new())` compiles in a `static` context because both are
// `const fn` since Rust 1.63 — no need for `lazy_static` or `OnceLock`.
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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
// we handle it silently — a monitoring library must never crash the app
// it is monitoring.
// ---------------------------------------------------------------------------
static STORE: Mutex<Vec<RequestMetric>> = Mutex::new(Vec::new());

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

    if let Ok(mut store) = STORE.lock() {
        store.push(metric);
    }
}

/// Returns a clone of all recorded metrics.
/// `.clone()` is required because we release the lock before returning.
pub fn get_all() -> Vec<RequestMetric> {
    STORE.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Clears the store — useful for tests and manual resets.
pub fn clear() {
    if let Ok(mut store) = STORE.lock() {
        store.clear();
    }
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
