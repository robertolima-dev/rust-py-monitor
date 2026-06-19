"""
Django middleware for rust_py_monitor.

Usage in settings.py:

    MIDDLEWARE = [
        "rust_py_monitor.django.MonitorMiddleware",
        # ... other middlewares ...
    ]
"""
import time
from typing import Callable

from asgiref.sync import iscoroutinefunction

from rust_py_monitor._core import record_request

try:
    # Public API for marking an object as a coroutine function — the supported
    # replacement for the private `asyncio.coroutines._is_coroutine` marker.
    # Available in asgiref >= 3.6 (Django >= 4.1).
    from asgiref.sync import markcoroutinefunction
except ImportError:  # pragma: no cover - asgiref < 3.6 (Django 4.0)
    markcoroutinefunction = None


class MonitorMiddleware:
    """
    Intercepts every Django request and records method, path,
    status_code, and duration in the Rust store.

    Supports both synchronous (WSGI) and asynchronous (ASGI) applications:
    - WSGI: Django calls __call__ directly.
    - ASGI: Django detects the coroutine marker and calls __acall__.
    """

    # Flags that Django uses to determine whether this middleware
    # accepts synchronous and/or asynchronous calls.
    async_capable = True
    sync_capable = True

    def __init__(self, get_response: Callable) -> None:
        self.get_response = get_response
        # If the response handler is a coroutine (ASGI mode), Django needs
        # to know that this middleware is also asynchronous.
        if iscoroutinefunction(self.get_response):
            if markcoroutinefunction is not None:
                markcoroutinefunction(self)
            else:  # pragma: no cover - legacy asgiref fallback
                import asyncio

                self._is_coroutine = asyncio.coroutines._is_coroutine  # type: ignore[attr-defined]

    def __call__(self, request):  # type: ignore[no-untyped-def]
        """Sync handler — used in WSGI applications."""
        start = time.perf_counter()
        # Default to 500 so requests that raise are still recorded as errors.
        status_code = 500
        try:
            response = self.get_response(request)
            status_code = response.status_code
            return response
        finally:
            duration_ms = (time.perf_counter() - start) * 1000.0
            record_request(
                method=request.method,
                path=request.path,
                status_code=status_code,
                duration_ms=duration_ms,
            )

    async def __acall__(self, request):  # type: ignore[no-untyped-def]
        """Async handler — used in Django ASGI applications."""
        start = time.perf_counter()
        status_code = 500
        try:
            response = await self.get_response(request)
            status_code = response.status_code
            return response
        finally:
            duration_ms = (time.perf_counter() - start) * 1000.0
            record_request(
                method=request.method,
                path=request.path,
                status_code=status_code,
                duration_ms=duration_ms,
            )
