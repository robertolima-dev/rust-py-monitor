"""
Example views for the Django app.
"""
import rust_py_monitor
from django.http import JsonResponse


def index(request):
    return JsonResponse({"status": "ok", "version": rust_py_monitor.__version__})


def users(request):
    return JsonResponse({"users": [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]})


def process_info(request):
    """Returns a live process snapshot (CPU, memory, PID)."""
    snap = rust_py_monitor.snapshot()
    return JsonResponse(snap.to_dict())


def request_stats(request):
    """Returns aggregated request statistics."""
    stats = rust_py_monitor.aggregate()
    return JsonResponse(stats.to_dict())


def recent_requests(request):
    """Returns the last 20 recorded requests."""
    all_reqs = rust_py_monitor.get_requests()
    return JsonResponse({"requests": [r.to_dict() for r in all_reqs[-20:]]})
