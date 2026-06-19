use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

mod aggregator;
mod metrics;
mod multiproc;
mod prometheus;
mod request_metrics;
mod snapshot;

use aggregator::AggregatedMetrics;
use metrics::SnapshotData;
use request_metrics::RequestMetric;

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Steps 2-5: snapshot
    m.add_function(wrap_pyfunction!(hello, m)?)?;
    m.add_function(wrap_pyfunction!(py_snapshot, m)?)?;
    m.add_class::<Snapshot>()?;

    // Step 6: request metrics
    m.add_function(wrap_pyfunction!(record_request, m)?)?;
    m.add_function(wrap_pyfunction!(get_requests, m)?)?;
    m.add_function(wrap_pyfunction!(clear_requests, m)?)?;
    m.add_function(wrap_pyfunction!(set_max_requests, m)?)?;
    m.add_function(wrap_pyfunction!(get_max_requests, m)?)?;
    m.add_class::<PyRequestMetric>()?;

    // Multi-worker aggregation (Step 7)
    m.add_function(wrap_pyfunction!(set_multiproc_dir, m)?)?;
    m.add_function(wrap_pyfunction!(get_multiproc_dir, m)?)?;
    m.add_function(wrap_pyfunction!(multiproc_enabled, m)?)?;

    // Step 8: aggregator
    m.add_function(wrap_pyfunction!(py_aggregate, m)?)?;
    m.add_class::<PyAggregatedMetrics>()?;

    // Step 9: Prometheus exporter
    m.add_function(wrap_pyfunction!(py_metrics_text, m)?)?;
    m.add("PROMETHEUS_CONTENT_TYPE", prometheus::CONTENT_TYPE)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Snapshot (Steps 4-5)
// ---------------------------------------------------------------------------

// `#[pyclass(frozen)]` makes the object immutable from Python.
// `name = "Snapshot"` sets the class name visible in Python.
#[pyclass(frozen, name = "Snapshot")]
#[derive(Clone)]
pub struct Snapshot {
    pid: u32,
    cpu_percent: f64,
    memory_rss: u64,
    memory_virtual: u64,
    threads: u64,
    timestamp: u64,
}

// `From` trait: idiomatic Rust conversion between types.
// Implementing `From<SnapshotData> for Snapshot` gives us `Snapshot::from(data)`
// and `data.into()` for free via the blanket `Into` impl.
// This keeps `SnapshotData` free of PyO3 dependencies.
impl From<SnapshotData> for Snapshot {
    fn from(d: SnapshotData) -> Self {
        Snapshot {
            pid: d.pid,
            cpu_percent: d.cpu_percent,
            memory_rss: d.memory_rss,
            memory_virtual: d.memory_virtual,
            threads: d.threads,
            timestamp: d.timestamp,
        }
    }
}

#[pymethods]
impl Snapshot {
    // `#[getter]` turns a Rust method into a Python property: `m.pid`
    #[getter] fn pid(&self) -> u32 { self.pid }
    #[getter] fn cpu_percent(&self) -> f64 { self.cpu_percent }
    #[getter] fn memory_rss(&self) -> u64 { self.memory_rss }
    #[getter] fn memory_virtual(&self) -> u64 { self.memory_virtual }
    #[getter] fn threads(&self) -> u64 { self.threads }
    #[getter] fn timestamp(&self) -> u64 { self.timestamp }

    #[getter]
    fn memory_rss_mb(&self) -> f64 {
        self.memory_rss as f64 / 1024.0 / 1024.0
    }

    fn __repr__(&self) -> String {
        format!(
            "Snapshot(pid={}, cpu={:.1}%, rss={:.1}MB, virt={:.1}MB, threads={}, ts={})",
            self.pid,
            self.cpu_percent,
            self.memory_rss as f64 / 1024.0 / 1024.0,
            self.memory_virtual as f64 / 1024.0 / 1024.0,
            self.threads,
            self.timestamp,
        )
    }

    // The lifetime `'py` ensures the PyDict does not outlive the GIL token.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        d.set_item("pid", self.pid)?;
        d.set_item("cpu_percent", self.cpu_percent)?;
        d.set_item("memory_rss", self.memory_rss)?;
        d.set_item("memory_virtual", self.memory_virtual)?;
        d.set_item("threads", self.threads)?;
        d.set_item("timestamp", self.timestamp)?;
        Ok(d)
    }
}

#[pyfunction]
fn hello() -> PyResult<String> {
    // `env!("CARGO_PKG_VERSION")` is resolved at compile time from Cargo.toml,
    // so this string can never drift out of sync with the crate version.
    Ok(format!(
        "rust_py_monitor v{} — Rust core running!",
        env!("CARGO_PKG_VERSION")
    ))
}

#[pyfunction]
#[pyo3(name = "snapshot")]
fn py_snapshot() -> PyResult<Snapshot> {
    snapshot::collect()
        .map(Snapshot::from)
        .map_err(PyRuntimeError::new_err)
}

// ---------------------------------------------------------------------------
// RequestMetric (Step 6)
// ---------------------------------------------------------------------------

/// Python-facing version of `request_metrics::RequestMetric`.
///
/// Pattern: plain Rust type for internal logic, Python type (Py-prefixed)
/// as the interface. The `From` conversion ensures no field is missed
/// when the internal struct changes.
#[pyclass(frozen, name = "RequestMetric")]
#[derive(Clone)]
pub struct PyRequestMetric {
    method: String,
    path: String,
    status_code: u16,
    duration_ms: f64,
    timestamp: u64,
}

impl From<RequestMetric> for PyRequestMetric {
    fn from(m: RequestMetric) -> Self {
        PyRequestMetric {
            method: m.method,
            path: m.path,
            status_code: m.status_code,
            duration_ms: m.duration_ms,
            timestamp: m.timestamp,
        }
    }
}

#[pymethods]
impl PyRequestMetric {
    #[getter] fn method(&self) -> &str { &self.method }
    #[getter] fn path(&self) -> &str { &self.path }
    #[getter] fn status_code(&self) -> u16 { self.status_code }
    #[getter] fn duration_ms(&self) -> f64 { self.duration_ms }
    #[getter] fn timestamp(&self) -> u64 { self.timestamp }

    fn __repr__(&self) -> String {
        format!(
            "RequestMetric({} {} {} {:.2}ms)",
            self.method, self.path, self.status_code, self.duration_ms
        )
    }

    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        d.set_item("method", &self.method)?;
        d.set_item("path", &self.path)?;
        d.set_item("status_code", self.status_code)?;
        d.set_item("duration_ms", self.duration_ms)?;
        d.set_item("timestamp", self.timestamp)?;
        Ok(d)
    }
}

// ---------------------------------------------------------------------------
// Module functions — Step 6
// ---------------------------------------------------------------------------

/// Records a request. Called by the Python middleware on every request.
/// `&str` in #[pyfunction]: PyO3 borrows the Python str without copying.
/// Inside `record()` the `.to_owned()` makes the copy needed for the store.
#[pyfunction]
fn record_request(method: &str, path: &str, status_code: u16, duration_ms: f64) {
    request_metrics::record(method, path, status_code, duration_ms);
}

/// Returns all recorded metrics as a Python list.
/// PyO3 converts `Vec<PyRequestMetric>` into `list[RequestMetric]` automatically.
#[pyfunction]
fn get_requests() -> Vec<PyRequestMetric> {
    request_metrics::get_all()
        .into_iter()
        .map(PyRequestMetric::from)
        .collect()
}

/// Clears the store — for tests and manual resets.
#[pyfunction]
fn clear_requests() {
    request_metrics::clear();
}

/// Sets the maximum number of requests retained in the ring buffer.
/// Once exceeded, the oldest entries are evicted first. Values < 1 clamp to 1.
#[pyfunction]
fn set_max_requests(max: usize) {
    request_metrics::set_max_requests(max);
}

/// Returns the current retention capacity of the request store.
#[pyfunction]
fn get_max_requests() -> usize {
    request_metrics::max_requests()
}

// ---------------------------------------------------------------------------
// Multi-worker aggregation (Step 7)
// ---------------------------------------------------------------------------

/// Enables shared multi-worker aggregation, writing this process's shard into
/// `dir`. Pass `None` to disable. Usually configured via the
/// `RPY_MULTIPROC_DIR` environment variable instead.
#[pyfunction]
#[pyo3(signature = (dir=None))]
fn set_multiproc_dir(dir: Option<&str>) {
    multiproc::set_dir(dir);
}

/// Returns the active multiproc directory, or `None` when disabled.
#[pyfunction]
fn get_multiproc_dir() -> Option<String> {
    multiproc::dir()
}

/// True when multi-worker aggregation is active.
#[pyfunction]
fn multiproc_enabled() -> bool {
    multiproc::enabled()
}

// ---------------------------------------------------------------------------
// AggregatedMetrics (Step 8)
// ---------------------------------------------------------------------------

/// Python-facing version of `aggregator::AggregatedMetrics`.
/// Same pattern: plain Rust type for logic, Python type for interface.
#[pyclass(frozen, name = "AggregatedMetrics")]
#[derive(Clone)]
pub struct PyAggregatedMetrics {
    total_requests: u64,
    total_errors: u64,
    error_rate: f64,
    avg_latency_ms: f64,
    min_latency_ms: f64,
    max_latency_ms: f64,
    p50_latency_ms: f64,
    p95_latency_ms: f64,
    p99_latency_ms: f64,
}

impl From<AggregatedMetrics> for PyAggregatedMetrics {
    fn from(a: AggregatedMetrics) -> Self {
        PyAggregatedMetrics {
            total_requests: a.total_requests,
            total_errors: a.total_errors,
            error_rate: a.error_rate,
            avg_latency_ms: a.avg_latency_ms,
            min_latency_ms: a.min_latency_ms,
            max_latency_ms: a.max_latency_ms,
            p50_latency_ms: a.p50_latency_ms,
            p95_latency_ms: a.p95_latency_ms,
            p99_latency_ms: a.p99_latency_ms,
        }
    }
}

#[pymethods]
impl PyAggregatedMetrics {
    #[getter] fn total_requests(&self) -> u64 { self.total_requests }
    #[getter] fn total_errors(&self) -> u64 { self.total_errors }
    #[getter] fn error_rate(&self) -> f64 { self.error_rate }
    #[getter] fn avg_latency_ms(&self) -> f64 { self.avg_latency_ms }
    #[getter] fn min_latency_ms(&self) -> f64 { self.min_latency_ms }
    #[getter] fn max_latency_ms(&self) -> f64 { self.max_latency_ms }
    #[getter] fn p50_latency_ms(&self) -> f64 { self.p50_latency_ms }
    #[getter] fn p95_latency_ms(&self) -> f64 { self.p95_latency_ms }
    #[getter] fn p99_latency_ms(&self) -> f64 { self.p99_latency_ms }

    fn __repr__(&self) -> String {
        format!(
            "AggregatedMetrics(total={}, errors={}, error_rate={:.1}%, \
             avg={:.2}ms, p50={:.2}ms, p95={:.2}ms, p99={:.2}ms)",
            self.total_requests,
            self.total_errors,
            self.error_rate,
            self.avg_latency_ms,
            self.p50_latency_ms,
            self.p95_latency_ms,
            self.p99_latency_ms,
        )
    }

    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        d.set_item("total_requests", self.total_requests)?;
        d.set_item("total_errors", self.total_errors)?;
        d.set_item("error_rate", self.error_rate)?;
        d.set_item("avg_latency_ms", self.avg_latency_ms)?;
        d.set_item("min_latency_ms", self.min_latency_ms)?;
        d.set_item("max_latency_ms", self.max_latency_ms)?;
        d.set_item("p50_latency_ms", self.p50_latency_ms)?;
        d.set_item("p95_latency_ms", self.p95_latency_ms)?;
        d.set_item("p99_latency_ms", self.p99_latency_ms)?;
        Ok(d)
    }
}

#[pyfunction]
#[pyo3(name = "aggregate")]
fn py_aggregate() -> PyAggregatedMetrics {
    PyAggregatedMetrics::from(aggregator::compute())
}

// ---------------------------------------------------------------------------
// Prometheus exporter (Step 9)
// ---------------------------------------------------------------------------

/// Returns all current metrics formatted as Prometheus text exposition format.
/// Reads from the global request store and the current process snapshot.
#[pyfunction]
#[pyo3(name = "metrics_text")]
fn py_metrics_text() -> String {
    prometheus::render()
}
