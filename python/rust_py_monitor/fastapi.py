"""
FastAPI/Starlette middleware for rust_py_monitor.

Usage:
    from fastapi import FastAPI
    from rust_py_monitor.fastapi import MonitorMiddleware

    app = FastAPI()
    app.add_middleware(MonitorMiddleware)
"""
import time

from starlette.middleware.base import BaseHTTPMiddleware
from starlette.requests import Request
from starlette.responses import Response
from starlette.types import ASGIApp

from rust_py_monitor._core import record_request


class MonitorMiddleware(BaseHTTPMiddleware):
    """
    Intercepts every HTTP request and records:
    - HTTP method (GET, POST, ...)
    - URL path (/users, /items/42, ...)
    - response status code
    - duration in milliseconds

    Data is pushed to the Rust store via record_request() and can be
    retrieved with rust_py_monitor.get_requests() or exported via
    Prometheus (Step 9).
    """

    def __init__(self, app: ASGIApp, **kwargs: object) -> None:
        super().__init__(app, **kwargs)

    async def dispatch(self, request: Request, call_next) -> Response:  # type: ignore[override]
        start = time.perf_counter()

        # Default to 500: if the handler raises before producing a response,
        # the request is still recorded as a server error. Without this, the
        # unhandled-exception 500s — the ones that matter most — would never
        # appear in the metrics, since the recording code below would be skipped.
        status_code = 500
        try:
            response = await call_next(request)
            status_code = response.status_code
            return response
        finally:
            # perf_counter returns seconds with high resolution; convert to ms.
            duration_ms = (time.perf_counter() - start) * 1000.0
            record_request(
                method=request.method,
                path=request.url.path,
                status_code=status_code,
                duration_ms=duration_ms,
            )
