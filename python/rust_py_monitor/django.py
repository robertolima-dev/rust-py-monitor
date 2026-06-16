"""
Django middleware for rust_py_monitor.

Usage in settings.py:

    MIDDLEWARE = [
        "rust_py_monitor.django.MonitorMiddleware",
        # ... other middlewares ...
    ]
"""
import asyncio
import time
from typing import Callable

from rust_py_monitor._core import record_request


class MonitorMiddleware:
    """
    Intercepts every Django request and records method, path,
    status_code, and duration in the Rust store.

    Supports both synchronous (WSGI) and asynchronous (ASGI) applications:
    - WSGI: Django calls __call__ directly.
    - ASGI: Django detects _is_coroutine and calls __acall__.
    """

    # Flags that Django uses to determine whether this middleware
    # accepts synchronous and/or asynchronous calls.
    async_capable = True
    sync_capable = True

    def __init__(self, get_response: Callable) -> None:
        self.get_response = get_response
        # If the response handler is a coroutine (ASGI mode), Django needs
        # to know that this middleware is also asynchronous.
        # Setting `_is_coroutine` is the official Django signal for this.
        if asyncio.iscoroutinefunction(self.get_response):
            self._is_coroutine = asyncio.coroutines._is_coroutine  # type: ignore[attr-defined]

    def __call__(self, request):  # type: ignore[no-untyped-def]
        """Sync handler — used in WSGI applications."""
        start = time.perf_counter()
        response = self.get_response(request)
        duration_ms = (time.perf_counter() - start) * 1000.0

        record_request(
            method=request.method,
            path=request.path,
            status_code=response.status_code,
            duration_ms=duration_ms,
        )
        return response

    async def __acall__(self, request):  # type: ignore[no-untyped-def]
        """Async handler — used in Django ASGI applications."""
        start = time.perf_counter()
        response = await self.get_response(request)
        duration_ms = (time.perf_counter() - start) * 1000.0

        record_request(
            method=request.method,
            path=request.path,
            status_code=response.status_code,
            duration_ms=duration_ms,
        )
        return response
