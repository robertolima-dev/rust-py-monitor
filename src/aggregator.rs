// Computation of aggregated statistics over the request metrics store.
//
// This module is plain Rust — no PyO3. The conversion to Python objects
// happens in lib.rs via `From<AggregatedMetrics> for PyAggregatedMetrics`.

use crate::multiproc;
use crate::request_metrics;

/// Aggregated statistics over all requests recorded so far.
#[derive(Debug, Clone)]
pub struct AggregatedMetrics {
    pub total_requests: u64,
    /// Requests with status_code >= 400.
    pub total_errors: u64,
    /// Error percentage: (total_errors / total_requests) * 100.
    pub error_rate: f64,
    pub avg_latency_ms: f64,
    pub min_latency_ms: f64,
    pub max_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
}

/// Returns zeroed values when no requests have been recorded.
/// `Default` is a Rust trait — equivalent to a no-argument constructor.
impl Default for AggregatedMetrics {
    fn default() -> Self {
        AggregatedMetrics {
            total_requests: 0,
            total_errors: 0,
            error_rate: 0.0,
            avg_latency_ms: 0.0,
            min_latency_ms: 0.0,
            max_latency_ms: 0.0,
            p50_latency_ms: 0.0,
            p95_latency_ms: 0.0,
            p99_latency_ms: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Percentile calculation
//
// We use nearest-rank index interpolation:
//   idx = round((p / 100) * (n - 1))
//
// Example with n=100 values [1..=100]:
//   p95 → idx = round(0.95 * 99) = round(94.05) = 94 → sorted[94] = 95
//   p99 → idx = round(0.99 * 99) = round(98.01) = 98 → sorted[98] = 99
//
// Precondition: `sorted` must be sorted and non-empty.
// ---------------------------------------------------------------------------
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    // `.min(len - 1)` guards against rounding past the last index.
    sorted[idx.min(sorted.len() - 1)]
}

/// Reads the global store and computes all statistics in a single pass.
///
/// In multi-worker mode (`RPY_MULTIPROC_DIR` set) the statistics are merged
/// across all live worker shards; latency percentiles are then approximated
/// from the merged histogram. Otherwise we use the in-process store, which
/// yields exact percentiles.
pub fn compute() -> AggregatedMetrics {
    if let Some(m) = multiproc::merged() {
        return aggregate_from_merged(m);
    }

    let metrics = request_metrics::get_all();

    if metrics.is_empty() {
        return AggregatedMetrics::default();
    }

    let total_requests = metrics.len() as u64;

    // `filter` + `count`: one iteration to count errors.
    let total_errors = metrics
        .iter()
        .filter(|m| m.status_code >= 400)
        .count() as u64;

    let error_rate = (total_errors as f64 / total_requests as f64) * 100.0;

    // `.map` + `.sum()`: extracts durations and sums them in one pass.
    let sum: f64 = metrics.iter().map(|m| m.duration_ms).sum();
    let avg_latency_ms = sum / total_requests as f64;

    // We need a sorted Vec for min, max, and percentiles.
    // `collect` moves the durations into a new Vec — necessary because we
    // need to sort and `metrics` may have other fields we don't want to copy.
    let mut durations: Vec<f64> = metrics.iter().map(|m| m.duration_ms).collect();

    // `total_cmp` is the total ordering for f64, introduced in Rust 1.62.
    // Unlike `partial_cmp`, it never returns None — handles NaN deterministically
    // without needing `.unwrap()`.
    durations.sort_by(|a, b| a.total_cmp(b));

    let min_latency_ms = durations[0];
    let max_latency_ms = durations[durations.len() - 1];
    let p50_latency_ms = percentile(&durations, 50.0);
    let p95_latency_ms = percentile(&durations, 95.0);
    let p99_latency_ms = percentile(&durations, 99.0);

    AggregatedMetrics {
        total_requests,
        total_errors,
        error_rate,
        avg_latency_ms,
        min_latency_ms,
        max_latency_ms,
        p50_latency_ms,
        p95_latency_ms,
        p99_latency_ms,
    }
}

/// Builds `AggregatedMetrics` from a merged multi-worker histogram.
/// Percentiles are approximated via `histogram_quantile` over the merged
/// buckets — exact percentiles are not recoverable across processes.
fn aggregate_from_merged(m: multiproc::Merged) -> AggregatedMetrics {
    if m.total == 0 {
        return AggregatedMetrics::default();
    }

    let total_requests = m.total;
    let total_errors = m.errors;
    let error_rate = (total_errors as f64 / total_requests as f64) * 100.0;

    let avg_latency_ms = (m.sum_us as f64 / total_requests as f64) / 1000.0;
    let min_latency_ms = if m.min_us == u64::MAX {
        0.0
    } else {
        m.min_us as f64 / 1000.0
    };
    let max_latency_ms = m.max_us as f64 / 1000.0;

    AggregatedMetrics {
        total_requests,
        total_errors,
        error_rate,
        avg_latency_ms,
        min_latency_ms,
        max_latency_ms,
        p50_latency_ms: multiproc::histogram_quantile(0.50, &m.buckets, max_latency_ms),
        p95_latency_ms: multiproc::histogram_quantile(0.95, &m.buckets, max_latency_ms),
        p99_latency_ms: multiproc::histogram_quantile(0.99, &m.buckets, max_latency_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_metrics;
    use serial_test::serial;

    fn cleanup() {
        request_metrics::clear();
    }

    // `#[serial]` serializes all marked tests within the same test binary.
    // Required because we share the `static STORE`.
    // Without it, `cargo test` runs tests in parallel and one test's cleanup()
    // can wipe data that another test just inserted.
    #[test]
    #[serial]
    fn empty_store_returns_zeros() {
        cleanup();
        let a = compute();
        assert_eq!(a.total_requests, 0);
        assert_eq!(a.total_errors, 0);
        assert_eq!(a.avg_latency_ms, 0.0);
        assert_eq!(a.p95_latency_ms, 0.0);
    }

    #[test]
    #[serial]
    fn single_ok_request() {
        cleanup();
        request_metrics::record("GET", "/", 200, 10.0);
        let a = compute();
        assert_eq!(a.total_requests, 1);
        assert_eq!(a.total_errors, 0);
        assert_eq!(a.error_rate, 0.0);
        assert_eq!(a.avg_latency_ms, 10.0);
        assert_eq!(a.min_latency_ms, 10.0);
        assert_eq!(a.max_latency_ms, 10.0);
        cleanup();
    }

    #[test]
    #[serial]
    fn error_counting() {
        cleanup();
        request_metrics::record("GET", "/ok", 200, 5.0);
        request_metrics::record("GET", "/nf", 404, 3.0);
        request_metrics::record("POST", "/err", 500, 8.0);

        let a = compute();
        assert_eq!(a.total_requests, 3);
        assert_eq!(a.total_errors, 2); // 404 and 500
        // 2/3 * 100 ≈ 66.67
        assert!((a.error_rate - 66.666_666).abs() < 0.01);
        cleanup();
    }

    #[test]
    #[serial]
    fn latency_average() {
        cleanup();
        request_metrics::record("GET", "/a", 200, 10.0);
        request_metrics::record("GET", "/b", 200, 20.0);
        request_metrics::record("GET", "/c", 200, 30.0);

        let a = compute();
        assert_eq!(a.avg_latency_ms, 20.0);
        assert_eq!(a.min_latency_ms, 10.0);
        assert_eq!(a.max_latency_ms, 30.0);
        cleanup();
    }

    #[test]
    #[serial]
    fn percentiles_with_100_values() {
        cleanup();
        // Insert 100 requests with durations 1ms..=100ms
        for i in 1u32..=100 {
            request_metrics::record("GET", "/x", 200, i as f64);
        }
        let a = compute();
        // p50: sorted[round(0.5 * 99)] = sorted[50] = 51ms
        assert_eq!(a.p50_latency_ms, 51.0);
        // p95: sorted[round(0.95 * 99)] = sorted[94] = 95ms
        assert_eq!(a.p95_latency_ms, 95.0);
        // p99: sorted[round(0.99 * 99)] = sorted[98] = 99ms
        assert_eq!(a.p99_latency_ms, 99.0);
        cleanup();
    }

    #[test]
    fn percentile_helper_direct() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&values, 0.0), 1.0);
        assert_eq!(percentile(&values, 100.0), 5.0);
        assert_eq!(percentile(&values, 50.0), 3.0);
        assert_eq!(percentile(&[], 95.0), 0.0);
    }
}
