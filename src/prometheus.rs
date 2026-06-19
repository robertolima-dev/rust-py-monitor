// Prometheus exposition format v0.0.4 exporter.
//
// Format spec: https://prometheus.io/docs/instrumenting/exposition_formats/
//
//   # HELP <name> <description>
//   # TYPE <name> counter|gauge
//   <name> <value>
//
// Each metric family is separated by a blank line for readability.
// The Content-Type must be "text/plain; version=0.0.4; charset=utf-8".

use std::fmt::Write;

use crate::aggregator;
use crate::snapshot;

/// Content-Type header value expected by Prometheus scrapers.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Renders all current metrics as a Prometheus text exposition string.
///
/// Reads from the global request store (via aggregator) and from the
/// current process snapshot. Process metrics are best-effort: if
/// snapshot::collect() fails they are silently omitted rather than
/// returning an error — the scraper will still receive HTTP metrics.
pub fn render() -> String {
    let agg = aggregator::compute();

    // Pre-allocate ~2 KB — avoids most reallocations for typical outputs.
    let mut out = String::with_capacity(2048);

    // --- HTTP request metrics ---
    push_metric(
        &mut out,
        "rpy_requests_total",
        "counter",
        "Total HTTP requests recorded",
        agg.total_requests as f64,
    );
    push_metric(
        &mut out,
        "rpy_errors_total",
        "counter",
        "Total HTTP errors (status >= 400)",
        agg.total_errors as f64,
    );
    push_metric(
        &mut out,
        "rpy_error_rate_percent",
        "gauge",
        "HTTP error rate as a percentage",
        agg.error_rate,
    );
    push_metric(
        &mut out,
        "rpy_latency_avg_ms",
        "gauge",
        "Average request latency in milliseconds",
        agg.avg_latency_ms,
    );
    push_metric(
        &mut out,
        "rpy_latency_min_ms",
        "gauge",
        "Minimum request latency in milliseconds",
        agg.min_latency_ms,
    );
    push_metric(
        &mut out,
        "rpy_latency_max_ms",
        "gauge",
        "Maximum request latency in milliseconds",
        agg.max_latency_ms,
    );
    push_metric(
        &mut out,
        "rpy_latency_p50_ms",
        "gauge",
        "P50 request latency in milliseconds",
        agg.p50_latency_ms,
    );
    push_metric(
        &mut out,
        "rpy_latency_p95_ms",
        "gauge",
        "P95 request latency in milliseconds",
        agg.p95_latency_ms,
    );
    push_metric(
        &mut out,
        "rpy_latency_p99_ms",
        "gauge",
        "P99 request latency in milliseconds",
        agg.p99_latency_ms,
    );

    // --- Process metrics (best-effort) ---
    if let Ok(snap) = snapshot::collect() {
        push_metric(
            &mut out,
            "rpy_process_cpu_percent",
            "gauge",
            "Process CPU usage percentage",
            snap.cpu_percent,
        );
        push_metric(
            &mut out,
            "rpy_process_memory_rss_bytes",
            "gauge",
            "Process RSS memory in bytes",
            snap.memory_rss as f64,
        );
        push_metric(
            &mut out,
            "rpy_process_memory_virtual_bytes",
            "gauge",
            "Process virtual memory in bytes",
            snap.memory_virtual as f64,
        );
        push_metric(
            &mut out,
            "rpy_process_threads",
            "gauge",
            "Number of process threads",
            snap.threads as f64,
        );
    }

    out
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Appends one metric family (HELP + TYPE + value line) to `out`.
///
/// `writeln!` on `&mut String` uses `std::fmt::Write` and never returns `Err`
/// because writing to a `String` is infallible. We use `let _ =` to make the
/// intent clear — this is not silently ignoring a real error.
fn push_metric(out: &mut String, name: &str, kind: &str, help: &str, value: f64) {
    let _ = writeln!(out, "# HELP {} {}", name, help);
    let _ = writeln!(out, "# TYPE {} {}", name, kind);
    let _ = writeln!(out, "{} {}", name, fmt_value(value));
    out.push('\n');
}

/// Formats a float for the Prometheus exposition format.
///
/// Rust's default `Display` prints `inf` / `-inf` / `NaN`, but Prometheus
/// requires the exact tokens `+Inf`, `-Inf`, `NaN`. Finite values fall through
/// to the normal formatter (e.g. counters render `6` rather than `6.0`).
fn fmt_value(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "+Inf".to_string() } else { "-Inf".to_string() }
    } else {
        value.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_metrics;
    use serial_test::serial;

    fn cleanup() {
        request_metrics::clear();
    }

    #[test]
    #[serial]
    fn render_contains_expected_metric_names() {
        cleanup();
        let text = render();
        assert!(text.contains("rpy_requests_total"));
        assert!(text.contains("rpy_errors_total"));
        assert!(text.contains("rpy_latency_p95_ms"));
        assert!(text.contains("rpy_latency_p99_ms"));
        assert!(text.contains("rpy_process_memory_rss_bytes"));
        assert!(text.contains("rpy_process_cpu_percent"));
    }

    #[test]
    fn each_metric_family_has_help_and_type() {
        let text = render();
        let help_count = text.lines().filter(|l| l.starts_with("# HELP")).count();
        let type_count = text.lines().filter(|l| l.starts_with("# TYPE")).count();
        assert_eq!(
            help_count, type_count,
            "every metric must have both # HELP and # TYPE lines"
        );
        assert!(help_count > 0);
    }

    #[test]
    #[serial]
    fn render_reflects_recorded_requests() {
        cleanup();
        for _ in 0..5 {
            request_metrics::record("GET", "/x", 200, 10.0);
        }
        request_metrics::record("GET", "/fail", 500, 5.0);

        let text = render();
        // Counter values are formatted as integers (f64 with .0 stripped by Rust)
        assert!(
            text.contains("rpy_requests_total 6"),
            "expected 6 total, got:\n{}",
            text
        );
        assert!(
            text.contains("rpy_errors_total 1"),
            "expected 1 error, got:\n{}",
            text
        );
        cleanup();
    }

    #[test]
    fn fmt_value_uses_prometheus_tokens() {
        assert_eq!(fmt_value(6.0), "6");
        assert_eq!(fmt_value(f64::NAN), "NaN");
        assert_eq!(fmt_value(f64::INFINITY), "+Inf");
        assert_eq!(fmt_value(f64::NEG_INFINITY), "-Inf");
    }

    #[test]
    #[serial]
    fn render_empty_store_outputs_zero_counts() {
        cleanup();
        let text = render();
        assert!(text.contains("rpy_requests_total 0"));
        assert!(text.contains("rpy_errors_total 0"));
    }
}
