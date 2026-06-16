"""
Integration tests: Django + MonitorMiddleware.

Django is configured once in conftest.py (pytest_configure hook).
`RequestFactory` creates real request objects without needing an HTTP server.
"""
import pytest
from django.http import HttpResponse, JsonResponse
from django.test import RequestFactory

from rust_py_monitor._core import clear_requests, get_requests
from rust_py_monitor.django import MonitorMiddleware

factory = RequestFactory()


# ---------------------------------------------------------------------------
# Test view helpers
# ---------------------------------------------------------------------------

def view_ok(request):
    return HttpResponse("ok", status=200)


def view_created(request):
    return JsonResponse({"id": 1}, status=201)


def view_not_found(request):
    return JsonResponse({"detail": "not found"}, status=404)


def view_server_error(request):
    return HttpResponse("error", status=500)


# ---------------------------------------------------------------------------
# Fixture: clear the store before and after each test
# ---------------------------------------------------------------------------

@pytest.fixture(autouse=True)
def reset_metrics():
    clear_requests()
    yield
    clear_requests()


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_middleware_records_get_200():
    request = factory.get("/ping")
    middleware = MonitorMiddleware(view_ok)
    response = middleware(request)

    assert response.status_code == 200
    metrics = get_requests()
    assert len(metrics) == 1
    m = metrics[0]
    assert m.method == "GET"
    assert m.path == "/ping"
    assert m.status_code == 200


def test_middleware_records_post_201():
    request = factory.post("/users", data={}, content_type="application/json")
    middleware = MonitorMiddleware(view_created)
    response = middleware(request)

    assert response.status_code == 201
    m = get_requests()[0]
    assert m.method == "POST"
    assert m.path == "/users"
    assert m.status_code == 201


def test_middleware_records_404_error():
    request = factory.get("/not-found")
    middleware = MonitorMiddleware(view_not_found)
    response = middleware(request)

    assert response.status_code == 404
    m = get_requests()[0]
    assert m.status_code == 404


def test_middleware_records_500_error():
    request = factory.get("/crash")
    middleware = MonitorMiddleware(view_server_error)
    middleware(request)

    m = get_requests()[0]
    assert m.status_code == 500


def test_middleware_records_positive_duration():
    request = factory.get("/ping")
    middleware = MonitorMiddleware(view_ok)
    middleware(request)

    m = get_requests()[0]
    assert m.duration_ms > 0.0


def test_middleware_records_path_with_param():
    request = factory.get("/items/99")
    middleware = MonitorMiddleware(view_ok)
    middleware(request)

    m = get_requests()[0]
    assert m.path == "/items/99"


def test_middleware_accumulates_multiple_requests():
    mw_ok = MonitorMiddleware(view_ok)
    mw_created = MonitorMiddleware(view_created)

    mw_ok(factory.get("/ping"))
    mw_ok(factory.get("/ping"))
    mw_created(factory.post("/users", data={}, content_type="application/json"))

    metrics = get_requests()
    assert len(metrics) == 3
    assert metrics[0].path == "/ping"
    assert metrics[2].path == "/users"


def test_middleware_returns_response_unchanged():
    """The middleware must not alter the response content."""
    request = factory.get("/ping")
    middleware = MonitorMiddleware(view_ok)
    response = middleware(request)

    assert response.content == b"ok"
    assert response.status_code == 200


def test_middleware_request_metric_repr():
    request = factory.get("/ping")
    MonitorMiddleware(view_ok)(request)

    r = repr(get_requests()[0])
    assert "GET" in r
    assert "/ping" in r
    assert "200" in r


def test_middleware_request_metric_to_dict():
    request = factory.delete("/item/5")
    MonitorMiddleware(view_ok)(request)

    d = get_requests()[0].to_dict()
    assert d["method"] == "DELETE"
    assert d["path"] == "/item/5"
    assert d["status_code"] == 200
    assert "duration_ms" in d
    assert "timestamp" in d


def test_middleware_chain_simulates_django_stack():
    """
    Simulates multiple stacked middlewares (as Django does in practice):
    MonitorMiddleware wrapping another middleware wrapping the view.
    """
    def inner_middleware(get_response):
        def handler(request):
            response = get_response(request)
            response["X-Inner"] = "true"
            return response
        return handler

    inner = inner_middleware(view_ok)
    monitor = MonitorMiddleware(inner)

    response = monitor(factory.get("/chain"))

    assert response.status_code == 200
    assert response.get("X-Inner") == "true"
    assert get_requests()[0].path == "/chain"
