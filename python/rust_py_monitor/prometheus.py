"""
Prometheus metrics exporter for rust_py_monitor.

Provides three integration points:

1. metrics_text() — raw Prometheus text, framework-agnostic:
       from rust_py_monitor.prometheus import metrics_text
       print(metrics_text())

2. make_fastapi_router() — mounts GET /metrics on a FastAPI app:
       from rust_py_monitor.prometheus import make_fastapi_router
       app.include_router(make_fastapi_router())

3. django_metrics_view — Django view for urls.py:
       from rust_py_monitor.prometheus import django_metrics_view
       urlpatterns = [path("metrics/", django_metrics_view)]
"""

from rust_py_monitor._core import PROMETHEUS_CONTENT_TYPE, metrics_text

__all__ = [
    "PROMETHEUS_CONTENT_TYPE",
    "metrics_text",
    "make_fastapi_router",
    "django_metrics_view",
]


def make_fastapi_router(path: str = "/metrics"):
    """
    Returns a FastAPI APIRouter that serves Prometheus metrics at `path`.

    Usage:
        from fastapi import FastAPI
        from rust_py_monitor.prometheus import make_fastapi_router

        app = FastAPI()
        app.include_router(make_fastapi_router())          # GET /metrics
        app.include_router(make_fastapi_router("/prom"))   # custom path
    """
    from fastapi import APIRouter
    from fastapi.responses import PlainTextResponse

    router = APIRouter()

    @router.get(path, include_in_schema=False)
    async def metrics_endpoint() -> PlainTextResponse:
        return PlainTextResponse(
            content=metrics_text(),
            media_type=PROMETHEUS_CONTENT_TYPE,
        )

    return router


def django_metrics_view(request):  # type: ignore[no-untyped-def]
    """
    Django view that serves Prometheus metrics.

    Usage in urls.py:
        from rust_py_monitor.prometheus import django_metrics_view
        urlpatterns = [
            path("metrics/", django_metrics_view),
        ]
    """
    from django.http import HttpResponse

    return HttpResponse(
        content=metrics_text(),
        content_type=PROMETHEUS_CONTENT_TYPE,
    )
