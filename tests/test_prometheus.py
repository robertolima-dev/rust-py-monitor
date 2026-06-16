"""
Prometheus exporter tests.
"""
import pytest

import rust_py_monitor
from rust_py_monitor._core import clear_requests, metrics_text, record_request
from rust_py_monitor.prometheus import PROMETHEUS_CONTENT_TYPE


@pytest.fixture(autouse=True)
def reset():
    clear_requests()
    yield
    clear_requests()


# ---------------------------------------------------------------------------
# metrics_text() — format and structure
# ---------------------------------------------------------------------------

def test_metrics_text_returns_non_empty_string():
    text = metrics_text()
    assert isinstance(text, str)
    assert len(text) > 0


def test_metrics_text_contains_help_lines():
    text = metrics_text()
    assert "# HELP" in text


def test_metrics_text_contains_type_lines():
    text = metrics_text()
    assert "# TYPE" in text


def test_each_metric_family_has_help_and_type():
    text = metrics_text()
    lines = text.splitlines()
    help_count = sum(1 for l in lines if l.startswith("# HELP"))
    type_count = sum(1 for l in lines if l.startswith("# TYPE"))
    assert help_count == type_count
    assert help_count > 0


def test_metrics_text_contains_request_metric_names():
    text = metrics_text()
    for name in [
        "rpy_requests_total",
        "rpy_errors_total",
        "rpy_error_rate_percent",
        "rpy_latency_avg_ms",
        "rpy_latency_min_ms",
        "rpy_latency_max_ms",
        "rpy_latency_p50_ms",
        "rpy_latency_p95_ms",
        "rpy_latency_p99_ms",
    ]:
        assert name in text, f"missing metric: {name}"


def test_metrics_text_contains_process_metric_names():
    text = metrics_text()
    for name in [
        "rpy_process_cpu_percent",
        "rpy_process_memory_rss_bytes",
        "rpy_process_memory_virtual_bytes",
        "rpy_process_threads",
    ]:
        assert name in text, f"missing metric: {name}"


# ---------------------------------------------------------------------------
# metrics_text() — values reflect recorded data
# ---------------------------------------------------------------------------

def test_metrics_text_empty_store_shows_zero_counts():
    text = metrics_text()
    assert "rpy_requests_total 0" in text
    assert "rpy_errors_total 0" in text


def test_metrics_text_reflects_request_count():
    for _ in range(4):
        record_request("GET", "/x", 200, 10.0)
    assert "rpy_requests_total 4" in metrics_text()


def test_metrics_text_reflects_error_count():
    record_request("GET", "/ok", 200, 5.0)
    record_request("GET", "/fail", 500, 5.0)
    record_request("GET", "/nf", 404, 3.0)
    text = metrics_text()
    assert "rpy_requests_total 3" in text
    assert "rpy_errors_total 2" in text


def test_metrics_text_process_rss_is_positive():
    text = metrics_text()
    lines = text.splitlines()
    rss_line = next((l for l in lines if l.startswith("rpy_process_memory_rss_bytes")), None)
    assert rss_line is not None
    value = float(rss_line.split()[-1])
    assert value > 0, f"RSS must be positive, got: {value}"


# ---------------------------------------------------------------------------
# Content-Type constant
# ---------------------------------------------------------------------------

def test_prometheus_content_type_format():
    assert PROMETHEUS_CONTENT_TYPE == "text/plain; version=0.0.4; charset=utf-8"


def test_prometheus_content_type_exported_from_core():
    assert rust_py_monitor.PROMETHEUS_CONTENT_TYPE == PROMETHEUS_CONTENT_TYPE


# ---------------------------------------------------------------------------
# FastAPI integration
# ---------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_fastapi_metrics_endpoint_returns_200():
    from fastapi import FastAPI
    from httpx import ASGITransport, AsyncClient
    from rust_py_monitor.prometheus import make_fastapi_router

    app = FastAPI()
    app.include_router(make_fastapi_router())

    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        resp = await client.get("/metrics")

    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_fastapi_metrics_endpoint_content_type():
    from fastapi import FastAPI
    from httpx import ASGITransport, AsyncClient
    from rust_py_monitor.prometheus import make_fastapi_router

    app = FastAPI()
    app.include_router(make_fastapi_router())

    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        resp = await client.get("/metrics")

    assert "text/plain" in resp.headers["content-type"]
    assert "0.0.4" in resp.headers["content-type"]


@pytest.mark.asyncio
async def test_fastapi_metrics_endpoint_reflects_data():
    from fastapi import FastAPI
    from httpx import ASGITransport, AsyncClient
    from rust_py_monitor.prometheus import make_fastapi_router

    record_request("GET", "/api/test", 200, 15.0)
    record_request("GET", "/api/test", 200, 25.0)
    record_request("POST", "/api/orders", 500, 8.0)

    app = FastAPI()
    app.include_router(make_fastapi_router())

    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        resp = await client.get("/metrics")

    assert "rpy_requests_total 3" in resp.text
    assert "rpy_errors_total 1" in resp.text
    assert "rpy_latency_p95_ms" in resp.text


@pytest.mark.asyncio
async def test_fastapi_custom_metrics_path():
    from fastapi import FastAPI
    from httpx import ASGITransport, AsyncClient
    from rust_py_monitor.prometheus import make_fastapi_router

    app = FastAPI()
    app.include_router(make_fastapi_router(path="/prom"))

    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        ok = await client.get("/prom")
        not_found = await client.get("/metrics")

    assert ok.status_code == 200
    assert not_found.status_code == 404


# ---------------------------------------------------------------------------
# Django integration
# ---------------------------------------------------------------------------

def test_django_metrics_view_returns_200():
    from django.test import RequestFactory
    from rust_py_monitor.prometheus import django_metrics_view

    request = RequestFactory().get("/metrics")
    response = django_metrics_view(request)
    assert response.status_code == 200


def test_django_metrics_view_content_type():
    from django.test import RequestFactory
    from rust_py_monitor.prometheus import django_metrics_view

    request = RequestFactory().get("/metrics")
    response = django_metrics_view(request)
    assert "text/plain" in response.get("Content-Type", "")
    assert "0.0.4" in response.get("Content-Type", "")


def test_django_metrics_view_reflects_data():
    record_request("DELETE", "/api/item/1", 204, 7.5)
    record_request("GET", "/api/broken", 503, 200.0)

    from django.test import RequestFactory
    from rust_py_monitor.prometheus import django_metrics_view

    request = RequestFactory().get("/metrics")
    response = django_metrics_view(request)
    body = response.content.decode()

    assert "rpy_requests_total" in body
    assert "rpy_errors_total" in body
    assert "rpy_process_memory_rss_bytes" in body
