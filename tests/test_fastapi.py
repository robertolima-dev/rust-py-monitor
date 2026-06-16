"""
Integration tests: FastAPI + MonitorMiddleware.

httpx.AsyncClient with ASGITransport fires real requests inside the process
without needing a running HTTP server, ensuring the middleware is activated
exactly as in production.
"""
import pytest
from fastapi import FastAPI
from fastapi.responses import JSONResponse
from httpx import ASGITransport, AsyncClient

import rust_py_monitor
from rust_py_monitor._core import clear_requests, get_requests
from rust_py_monitor.fastapi import MonitorMiddleware


# ---------------------------------------------------------------------------
# Test app
# ---------------------------------------------------------------------------

def build_app() -> FastAPI:
    app = FastAPI()
    app.add_middleware(MonitorMiddleware)

    @app.get("/ping")
    async def ping():
        return {"status": "ok"}

    @app.get("/items/{item_id}")
    async def get_item(item_id: int):
        return {"item_id": item_id}

    @app.post("/users")
    async def create_user():
        return JSONResponse({"id": 1}, status_code=201)

    @app.get("/error")
    async def trigger_error():
        return JSONResponse({"detail": "not found"}, status_code=404)

    return app


@pytest.fixture
def app() -> FastAPI:
    return build_app()


@pytest.fixture(autouse=True)
def reset_metrics():
    clear_requests()
    yield
    clear_requests()


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_middleware_records_simple_request(app: FastAPI):
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        resp = await client.get("/ping")

    assert resp.status_code == 200
    metrics = get_requests()
    assert len(metrics) == 1
    m = metrics[0]
    assert m.method == "GET"
    assert m.path == "/ping"
    assert m.status_code == 200


@pytest.mark.asyncio
async def test_middleware_records_positive_duration(app: FastAPI):
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        await client.get("/ping")

    m = get_requests()[0]
    assert m.duration_ms > 0.0


@pytest.mark.asyncio
async def test_middleware_records_post_201(app: FastAPI):
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        resp = await client.post("/users")

    assert resp.status_code == 201
    m = get_requests()[0]
    assert m.method == "POST"
    assert m.path == "/users"
    assert m.status_code == 201


@pytest.mark.asyncio
async def test_middleware_records_404_error(app: FastAPI):
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        resp = await client.get("/error")

    assert resp.status_code == 404
    m = get_requests()[0]
    assert m.status_code == 404


@pytest.mark.asyncio
async def test_middleware_records_path_with_param(app: FastAPI):
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        await client.get("/items/42")

    m = get_requests()[0]
    assert m.path == "/items/42"


@pytest.mark.asyncio
async def test_middleware_accumulates_multiple_requests(app: FastAPI):
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        await client.get("/ping")
        await client.get("/ping")
        await client.post("/users")

    metrics = get_requests()
    assert len(metrics) == 3
    assert metrics[0].path == "/ping"
    assert metrics[2].path == "/users"


@pytest.mark.asyncio
async def test_request_metric_repr(app: FastAPI):
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        await client.get("/ping")

    m = get_requests()[0]
    r = repr(m)
    assert "GET" in r
    assert "/ping" in r
    assert "200" in r
    assert "ms" in r


@pytest.mark.asyncio
async def test_request_metric_to_dict(app: FastAPI):
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        await client.get("/ping")

    d = get_requests()[0].to_dict()
    assert set(d.keys()) == {"method", "path", "status_code", "duration_ms", "timestamp"}
    assert d["method"] == "GET"
    assert d["status_code"] == 200


@pytest.mark.asyncio
async def test_snapshot_works_inside_request(app: FastAPI):
    """The Rust snapshot must work normally inside a request context."""

    @app.get("/with-snapshot")
    async def with_snapshot():
        m = rust_py_monitor.snapshot()
        return {"pid": m.pid, "rss_mb": m.memory_rss_mb}

    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as client:
        resp = await client.get("/with-snapshot")

    assert resp.status_code == 200
    data = resp.json()
    assert data["pid"] > 0
    assert data["rss_mb"] > 0
