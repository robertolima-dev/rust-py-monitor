# rust-py-monitor

[![PyPI](https://img.shields.io/pypi/v/rust-py-monitor?color=e8673a&label=PyPI)](https://pypi.org/project/rust-py-monitor/)
[![Python](https://img.shields.io/pypi/pyversions/rust-py-monitor?color=4b8bbe)](https://pypi.org/project/rust-py-monitor/)
[![License](https://img.shields.io/pypi/l/rust-py-monitor?color=3fb950)](https://github.com/robertolima-dev/rust-py-monitor/blob/main/LICENSE)
[![GitHub](https://img.shields.io/github/stars/robertolima-dev/rust-py-monitor?style=flat&color=e8673a)](https://github.com/robertolima-dev/rust-py-monitor)

🌐 **[rust-py-monitor.vercel.app](https://rust-py-monitor.vercel.app/)**

High-performance Python monitoring library with a Rust core.

Collects CPU, memory, threads, and HTTP request metrics from Django and FastAPI applications with minimal overhead. Exports metrics to logs, JSON, and Prometheus.

---

## Features

- **Process snapshot** — CPU %, RSS memory, virtual memory, thread count, PID
- **FastAPI middleware** — per-request latency, method, path, status code (errors recorded even when a handler raises)
- **Django middleware** — same, for WSGI and ASGI Django apps
- **Aggregator** — total requests, error count, error rate, avg/min/max/p50/p95/p99 latency
- **Bounded store** — request history is a capped ring buffer (default 10k) — constant memory under any traffic
- **Multi-worker aggregation** — opt-in shared store merges metrics across gunicorn/uvicorn workers (`RPY_MULTIPROC_DIR`)
- **Prometheus exporter** — `/metrics` endpoint compatible with Prometheus scraper
- **Rust core** — collection and aggregation happen in Rust via PyO3; Python API stays simple

---

## Requirements

- Python 3.10+
- No mandatory runtime dependencies

Optional, installed separately:
- `fastapi` + `starlette` — for `MonitorMiddleware` and `make_fastapi_router()`
- `django` — for `MonitorMiddleware` and `django_metrics_view`

---

## Installation

```bash
pip install rust-py-monitor
```

With optional extras:

```bash
pip install "rust-py-monitor[fastapi]"
pip install "rust-py-monitor[django]"
pip install "rust-py-monitor[fastapi,django,prometheus]"
```

---

## Quick Start

```python
import rust_py_monitor

# Process snapshot
m = rust_py_monitor.snapshot()
print(m)
# Snapshot(pid=1234, cpu=0.3%, rss=45.2MB, virt=512.0MB, threads=4, ts=1718000000)

print(m.pid)            # 1234
print(m.memory_rss_mb)  # 45.2
print(m.to_dict())      # {"pid": 1234, "cpu_percent": 0.3, ...}

# Aggregated request metrics
stats = rust_py_monitor.aggregate()
print(stats.total_requests)  # 0 (no middleware active yet)
print(stats.p95_latency_ms)  # 0.0
```

---

## FastAPI

### Middleware

```python
from fastapi import FastAPI
from rust_py_monitor.fastapi import MonitorMiddleware

app = FastAPI()
app.add_middleware(MonitorMiddleware)


@app.get("/")
async def root():
    return {"status": "ok"}
```

### Prometheus endpoint

```python
from fastapi import FastAPI
from rust_py_monitor.fastapi import MonitorMiddleware
from rust_py_monitor.prometheus import make_fastapi_router

app = FastAPI()
app.add_middleware(MonitorMiddleware)
app.include_router(make_fastapi_router())          # GET /metrics
# app.include_router(make_fastapi_router("/prom")) # custom path
```

### Inspect metrics programmatically

```python
import rust_py_monitor

stats = rust_py_monitor.aggregate()
print(f"Requests: {stats.total_requests}")
print(f"Errors:   {stats.total_errors} ({stats.error_rate:.1f}%)")
print(f"p95:      {stats.p95_latency_ms:.1f}ms")
print(f"p99:      {stats.p99_latency_ms:.1f}ms")

for req in rust_py_monitor.get_requests()[-5:]:
    print(req)
    # RequestMetric(GET /api/users 200 12.34ms)
```

---

## Django

### Middleware

```python
# settings.py
MIDDLEWARE = [
    "rust_py_monitor.django.MonitorMiddleware",
    # ... other middlewares ...
]
```

### Prometheus endpoint

```python
# urls.py
from django.urls import path
from rust_py_monitor.prometheus import django_metrics_view

urlpatterns = [
    path("metrics/", django_metrics_view),
    # ...
]
```

The middleware supports both WSGI and ASGI Django applications automatically.

---

## Prometheus Output

`GET /metrics` returns:

```
# HELP rpy_requests_total Total HTTP requests recorded
# TYPE rpy_requests_total counter
rpy_requests_total 1024

# HELP rpy_errors_total Total HTTP errors (status >= 400)
# TYPE rpy_errors_total counter
rpy_errors_total 12

# HELP rpy_error_rate_percent HTTP error rate as a percentage
# TYPE rpy_error_rate_percent gauge
rpy_error_rate_percent 1.171875

# HELP rpy_latency_p95_ms P95 request latency in milliseconds
# TYPE rpy_latency_p95_ms gauge
rpy_latency_p95_ms 47.3

# HELP rpy_process_memory_rss_bytes Process RSS memory in bytes
# TYPE rpy_process_memory_rss_bytes gauge
rpy_process_memory_rss_bytes 52428800

# ... (13 metrics total)
```

**Content-Type:** `text/plain; version=0.0.4; charset=utf-8`

---

## API Reference

### `rust_py_monitor.snapshot() → Snapshot`

Captures a point-in-time snapshot of the current process.

| Property | Type | Description |
|---|---|---|
| `pid` | `int` | Process ID |
| `cpu_percent` | `float` | CPU usage (0–100 × cores). First call may return 0.0. |
| `memory_rss` | `int` | Resident Set Size in bytes |
| `memory_rss_mb` | `float` | RSS in megabytes (convenience) |
| `memory_virtual` | `int` | Virtual memory in bytes |
| `threads` | `int` | Thread count (0 on macOS/Windows) |
| `timestamp` | `int` | Unix timestamp in seconds |
| `to_dict()` | `dict` | All fields as a plain dict |

---

### `rust_py_monitor.aggregate() → AggregatedMetrics`

Computes statistics over all requests recorded since startup (or last `clear_requests()`).

| Property | Type | Description |
|---|---|---|
| `total_requests` | `int` | Total request count |
| `total_errors` | `int` | Requests with status ≥ 400 |
| `error_rate` | `float` | `total_errors / total_requests × 100` |
| `avg_latency_ms` | `float` | Mean latency |
| `min_latency_ms` | `float` | Minimum latency |
| `max_latency_ms` | `float` | Maximum latency |
| `p50_latency_ms` | `float` | Median latency |
| `p95_latency_ms` | `float` | 95th percentile latency |
| `p99_latency_ms` | `float` | 99th percentile latency |
| `to_dict()` | `dict` | All fields as a plain dict |

---

### `rust_py_monitor.get_requests() → list[RequestMetric]`

Returns all recorded requests. Each `RequestMetric` has:

| Property | Type |
|---|---|
| `method` | `str` |
| `path` | `str` |
| `status_code` | `int` |
| `duration_ms` | `float` |
| `timestamp` | `int` |
| `to_dict()` | `dict` |

---

### `rust_py_monitor.metrics_text() → str`

Returns all metrics in Prometheus text exposition format (v0.0.4).

---

### `rust_py_monitor.clear_requests()`

Clears the request store. Useful for testing and periodic resets.

---

### `rust_py_monitor.set_max_requests(n)` / `get_max_requests() → int`

The request store is a bounded ring buffer (default capacity **10 000**). Once
full, the oldest entries are evicted first, so memory never grows without bound.
Use these to tune the retention window.

---

## Multi-worker deployments (gunicorn / uvicorn)

By default each worker process keeps its own in-memory store. A Prometheus
scrape of `/metrics` reaches only one worker, so the numbers would reflect just
that worker's traffic.

Set the **`RPY_MULTIPROC_DIR`** environment variable to a writable directory to
enable shared aggregation. Each worker writes a small fixed-size shard file
(`rpy-<pid>.shard`); `aggregate()` and `metrics_text()` then merge **all** live
workers' shards at read time. Shards of dead workers are pruned automatically.

```bash
export RPY_MULTIPROC_DIR=/tmp/rpy-metrics
gunicorn -w 4 myapp:app
```

You can also configure it at runtime:

```python
import rust_py_monitor
rust_py_monitor.set_multiproc_dir("/tmp/rpy-metrics")
rust_py_monitor.multiproc_enabled()   # True
rust_py_monitor.get_multiproc_dir()   # "/tmp/rpy-metrics"
```

**Notes:**
- Counters (`total_requests`, `total_errors`) and latency **histogram buckets**
  are summed across workers. Latency percentiles (p50/p95/p99) are therefore
  **approximated from the merged histogram** rather than computed exactly.
- `get_requests()` always returns the **local** process's recent requests only.
- Process metrics (CPU/memory/threads) reflect the worker that served the
  scrape.

---

## Building from Source

Requires Rust and [maturin](https://github.com/PyO3/maturin).

```bash
pip install maturin
git clone https://github.com/robertolima-dev/rust-py-monitor
cd rust-py-monitor

# Development build (installs into current Python environment)
maturin develop

# Release wheel
maturin build --release
```

### Running tests

```bash
# Rust unit tests
cargo test

# Python integration tests
pip install pytest pytest-asyncio httpx fastapi django
pytest tests/
```

---

## Architecture

```
Python API (rust_py_monitor)
    ├── snapshot()          ──► src/snapshot.rs   (sysinfo crate)
    ├── aggregate()         ──► src/aggregator.rs (pure Rust math)
    ├── get_requests()      ──► src/request_metrics.rs (static Mutex<VecDeque>, bounded)
    ├── metrics_text()      ──► src/prometheus.rs (text formatter)
    ├── set_multiproc_dir() ──► src/multiproc.rs  (mmap shard per worker)
    │
    ├── fastapi.MonitorMiddleware  ──► record_request() ──► Rust store
    ├── django.MonitorMiddleware   ──► record_request() ──► Rust store
    └── prometheus.make_fastapi_router() / django_metrics_view
```

The Rust core is compiled to a native `.so` / `.pyd` extension module by [maturin](https://github.com/PyO3/maturin) and [PyO3](https://pyo3.rs). The Python layer is thin — it just routes calls and provides framework-specific adapters.

---

## License

MIT — see [LICENSE](LICENSE).
