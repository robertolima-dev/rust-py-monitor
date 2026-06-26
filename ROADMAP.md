# Roadmap — rust-py-monitor

Direction for `rust-py-monitor`: a high-performance Python monitoring library with
a Rust core (PyO3 + maturin). The library is mature (v0.2.0) — process snapshots,
FastAPI/Django middlewares, the latency aggregator, the bounded ring-buffer store,
multi-worker aggregation, and the Prometheus exporter are shipped. This document
tracks what is done and the **directional** ideas under consideration.

> Status legend: ✅ shipped · 🔜 planned (next) · 💡 idea (no version yet) · ⚠️ note

> ⚠️ Beyond "shipped", items below are **inferred directions, not commitments**.
> Confirm priorities with the maintainer before planning a release around them.

---

## Shipped — up to v0.2.0

- ✅ `snapshot()` — CPU %, RSS, virtual memory, thread count, PID (with
  `memory_rss_mb` / `to_dict()` conveniences).
- ✅ FastAPI and Django middlewares — per-request latency, method, path, status
  (errors recorded even when the handler raises).
- ✅ `aggregate()` — total requests, errors, error rate, avg/min/max and
  p50/p95/p99 latency.
- ✅ **Bounded store** — capped ring buffer (default 10k), tunable via
  `set_max_requests()` → constant memory under any traffic.
- ✅ **Multi-worker aggregation** — opt-in mmap shard per worker
  (`RPY_MULTIPROC_DIR`) merged at read time for gunicorn/uvicorn.
- ✅ Prometheus exporter (`/metrics`, text exposition v0.0.4) with
  `make_fastapi_router()` and `django_metrics_view`.

---

## Directional ideas (no version assigned — confirm before planning)

- 💡 **Simple alerts** — thresholds for high CPU / high memory, mirroring the
  `rust-node-monitor` roadmap (shared concept across the two monitors).
- 💡 **GC metrics** — Python GC collections/pauses alongside CPU/memory.
- 💡 **Per-route / labeled metrics** — break latency and error counts down by
  route (with cardinality controls), exported as Prometheus labels.
- 💡 **More exporters / sinks** — JSON push, StatsD, OpenTelemetry, or an
  ImmutableLog integration for health/audit events.
- 💡 **Exact multi-worker percentiles** — today merged percentiles are
  approximated from the summed histogram; a finer-grained histogram could
  tighten that.
- 💡 **Flask middleware** — parity with the FastAPI/Django adapters.
- 💡 **Benchmarks suite** — middleware overhead and aggregation throughput.

---

## Known limitations (by design, for now)

- `snapshot()` may report `cpu_percent: 0.0` on the first call (needs two samples
  for a delta).
- `threads` is `0` on macOS/Windows.
- In multi-worker mode, latency percentiles are **approximated** from the merged
  histogram; `get_requests()` returns only the local process's recent requests.
- Process metrics in a scrape reflect the worker that served it.

---

## Contributing to the roadmap

Versions and ordering are indicative and may shift. Bump the version in **both**
`Cargo.toml` and `pyproject.toml` (kept in sync) plus `__version__`, ship tests
(`cargo test` + `pytest`), then tag `vX.Y.Z` to trigger the release workflow
(Trusted Publishing / OIDC to PyPI).
