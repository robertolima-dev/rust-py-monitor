"""
Tests for the bounded request store and version metadata.
"""
from importlib.metadata import version

import pytest

import rust_py_monitor
from rust_py_monitor import get_max_requests, set_max_requests
from rust_py_monitor._core import clear_requests, get_requests, record_request


@pytest.fixture(autouse=True)
def reset():
    clear_requests()
    set_max_requests(10_000)  # restore default
    yield
    clear_requests()
    set_max_requests(10_000)


# ---------------------------------------------------------------------------
# Bounded ring buffer
# ---------------------------------------------------------------------------

def test_default_capacity():
    assert get_max_requests() == 10_000


def test_store_is_bounded():
    set_max_requests(5)
    for i in range(20):
        record_request("GET", "/x", 200, float(i))

    metrics = get_requests()
    # Never grows past the cap.
    assert len(metrics) == 5
    # Keeps the most recent entries, oldest first.
    assert metrics[0].duration_ms == 15.0
    assert metrics[-1].duration_ms == 19.0


def test_shrinking_capacity_evicts_immediately():
    for i in range(10):
        record_request("GET", "/x", 200, float(i))
    set_max_requests(3)

    metrics = get_requests()
    assert len(metrics) == 3
    assert metrics[0].duration_ms == 7.0
    assert metrics[-1].duration_ms == 9.0


def test_capacity_clamped_to_at_least_one():
    set_max_requests(0)
    assert get_max_requests() == 1
    record_request("GET", "/a", 200, 1.0)
    record_request("GET", "/b", 200, 2.0)
    metrics = get_requests()
    assert len(metrics) == 1
    assert metrics[0].path == "/b"


# ---------------------------------------------------------------------------
# Version is single-sourced from package metadata
# ---------------------------------------------------------------------------

def test_version_matches_package_metadata():
    assert rust_py_monitor.__version__ == version("rust-py-monitor")


def test_hello_reports_same_version():
    # hello() derives its version from CARGO_PKG_VERSION at compile time.
    assert rust_py_monitor.__version__ in rust_py_monitor.hello()
