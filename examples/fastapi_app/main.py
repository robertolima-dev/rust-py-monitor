"""
FastAPI example app for rust-py-monitor.

Run:
    pip install "rust-py-monitor[fastapi]" uvicorn
    uvicorn examples.fastapi_app.main:app --reload

Then:
    curl http://localhost:8000/
    curl http://localhost:8000/users
    curl http://localhost:8000/metrics
"""
import rust_py_monitor
from fastapi import FastAPI
from fastapi.responses import JSONResponse
from rust_py_monitor.fastapi import MonitorMiddleware
from rust_py_monitor.prometheus import make_fastapi_router

app = FastAPI(title="rust-py-monitor FastAPI example")

# 1. Add monitoring middleware — must be the first middleware added
app.add_middleware(MonitorMiddleware)

# 2. Mount the Prometheus /metrics endpoint
app.include_router(make_fastapi_router())


# ---------------------------------------------------------------------------
# Example routes
# ---------------------------------------------------------------------------

@app.get("/")
async def root():
    return {"status": "ok", "version": rust_py_monitor.__version__}


@app.get("/users")
async def list_users():
    return [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]


@app.get("/users/{user_id}")
async def get_user(user_id: int):
    if user_id > 100:
        return JSONResponse({"detail": "user not found"}, status_code=404)
    return {"id": user_id, "name": f"User {user_id}"}


@app.get("/process")
async def process_info():
    """Returns a live process snapshot (CPU, memory, PID)."""
    snap = rust_py_monitor.snapshot()
    return snap.to_dict()


@app.get("/stats")
async def request_stats():
    """Returns aggregated request statistics."""
    stats = rust_py_monitor.aggregate()
    return stats.to_dict()


@app.get("/requests")
async def recent_requests(limit: int = 20):
    """Returns the last N recorded requests."""
    all_requests = rust_py_monitor.get_requests()
    recent = all_requests[-limit:]
    return [r.to_dict() for r in recent]


@app.delete("/requests")
async def reset_requests():
    """Clears the request store (useful for testing)."""
    rust_py_monitor.clear_requests()
    return {"cleared": True}
