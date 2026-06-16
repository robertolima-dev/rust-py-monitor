"""
Aggregator tests.
"""
import pytest

import rust_py_monitor
from rust_py_monitor import AggregatedMetrics
from rust_py_monitor._core import clear_requests, record_request


@pytest.fixture(autouse=True)
def reset():
    clear_requests()
    yield
    clear_requests()


# ---------------------------------------------------------------------------
# Type and structure
# ---------------------------------------------------------------------------

def test_aggregate_returns_aggregated_metrics():
    stats = rust_py_monitor.aggregate()
    assert isinstance(stats, AggregatedMetrics)


def test_aggregate_has_all_attributes():
    stats = rust_py_monitor.aggregate()
    _ = stats.total_requests
    _ = stats.total_errors
    _ = stats.error_rate
    _ = stats.avg_latency_ms
    _ = stats.min_latency_ms
    _ = stats.max_latency_ms
    _ = stats.p50_latency_ms
    _ = stats.p95_latency_ms
    _ = stats.p99_latency_ms


def test_aggregate_is_immutable():
    stats = rust_py_monitor.aggregate()
    try:
        stats.total_requests = 0  # type: ignore[misc]
        assert False, "should be immutable"
    except AttributeError:
        pass


# ---------------------------------------------------------------------------
# Empty store
# ---------------------------------------------------------------------------

def test_aggregate_empty_store_returns_zeros():
    stats = rust_py_monitor.aggregate()
    assert stats.total_requests == 0
    assert stats.total_errors == 0
    assert stats.error_rate == 0.0
    assert stats.avg_latency_ms == 0.0
    assert stats.p95_latency_ms == 0.0
    assert stats.p99_latency_ms == 0.0


# ---------------------------------------------------------------------------
# Counts
# ---------------------------------------------------------------------------

def test_aggregate_counts_total_requests():
    for _ in range(5):
        record_request("GET", "/x", 200, 10.0)
    assert rust_py_monitor.aggregate().total_requests == 5


def test_aggregate_counts_4xx_errors():
    record_request("GET", "/ok", 200, 5.0)
    record_request("GET", "/nf", 404, 3.0)
    record_request("GET", "/nf2", 422, 2.0)

    stats = rust_py_monitor.aggregate()
    assert stats.total_requests == 3
    assert stats.total_errors == 2


def test_aggregate_counts_5xx_errors():
    record_request("POST", "/ok", 201, 5.0)
    record_request("POST", "/err", 500, 8.0)

    stats = rust_py_monitor.aggregate()
    assert stats.total_errors == 1


def test_aggregate_error_rate_is_correct():
    record_request("GET", "/ok", 200, 5.0)
    record_request("GET", "/ok", 200, 5.0)
    record_request("GET", "/nf", 404, 3.0)
    record_request("GET", "/err", 500, 8.0)

    stats = rust_py_monitor.aggregate()
    # 2 errors out of 4 requests = 50%
    assert abs(stats.error_rate - 50.0) < 0.01


# ---------------------------------------------------------------------------
# Latency
# ---------------------------------------------------------------------------

def test_aggregate_avg_latency():
    record_request("GET", "/a", 200, 10.0)
    record_request("GET", "/b", 200, 20.0)
    record_request("GET", "/c", 200, 30.0)

    stats = rust_py_monitor.aggregate()
    assert abs(stats.avg_latency_ms - 20.0) < 0.01


def test_aggregate_min_max_latency():
    record_request("GET", "/fast", 200, 5.0)
    record_request("GET", "/mid", 200, 50.0)
    record_request("GET", "/slow", 200, 200.0)

    stats = rust_py_monitor.aggregate()
    assert abs(stats.min_latency_ms - 5.0) < 0.01
    assert abs(stats.max_latency_ms - 200.0) < 0.01


def test_aggregate_percentiles_with_10_values():
    # Insert 10 requests with durations 10ms, 20ms, ..., 100ms
    for i in range(1, 11):
        record_request("GET", f"/r{i}", 200, float(i * 10))

    stats = rust_py_monitor.aggregate()
    # Sorted values: [10, 20, 30, 40, 50, 60, 70, 80, 90, 100]
    # p50: idx = round(0.5 * 9) = round(4.5) — may be 4 or 5 depending on rounding
    # We check a reasonable range for p50
    assert 40.0 <= stats.p50_latency_ms <= 60.0
    # p95: idx = round(0.95 * 9) = round(8.55) = 9 → sorted[9] = 100
    assert abs(stats.p95_latency_ms - 100.0) < 0.01
    # p99: idx = round(0.99 * 9) = round(8.91) = 9 → sorted[9] = 100
    assert abs(stats.p99_latency_ms - 100.0) < 0.01


# ---------------------------------------------------------------------------
# Serialization
# ---------------------------------------------------------------------------

def test_aggregate_repr_contains_totals():
    record_request("GET", "/ping", 200, 15.0)
    stats = rust_py_monitor.aggregate()
    r = repr(stats)
    assert "total=1" in r
    assert "errors=0" in r


def test_aggregate_to_dict_has_all_keys():
    stats = rust_py_monitor.aggregate()
    d = stats.to_dict()
    expected_keys = {
        "total_requests", "total_errors", "error_rate",
        "avg_latency_ms", "min_latency_ms", "max_latency_ms",
        "p50_latency_ms", "p95_latency_ms", "p99_latency_ms",
    }
    assert set(d.keys()) == expected_keys


def test_aggregate_to_dict_values_match():
    record_request("GET", "/ok", 200, 42.0)
    stats = rust_py_monitor.aggregate()
    d = stats.to_dict()

    assert d["total_requests"] == stats.total_requests
    assert d["avg_latency_ms"] == stats.avg_latency_ms
    assert d["p95_latency_ms"] == stats.p95_latency_ms


# ---------------------------------------------------------------------------
# Integration: aggregate() reflects what the middleware recorded
# ---------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_aggregate_integrates_with_fastapi():
    """aggregate() must reflect requests captured by the middleware."""
    from fastapi import FastAPI
    from httpx import ASGITransport, AsyncClient
    from rust_py_monitor.fastapi import MonitorMiddleware

    app = FastAPI()
    app.add_middleware(MonitorMiddleware)

    @app.get("/ok")
    async def ok():
        return {}

    @app.get("/fail")
    async def fail():
        from fastapi.responses import JSONResponse
        return JSONResponse({}, status_code=500)

    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        await client.get("/ok")
        await client.get("/ok")
        await client.get("/fail")

    stats = rust_py_monitor.aggregate()
    assert stats.total_requests == 3
    assert stats.total_errors == 1
    assert abs(stats.error_rate - 33.333) < 0.01
    assert stats.avg_latency_ms > 0
