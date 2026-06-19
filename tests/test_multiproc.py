"""
Tests for multi-worker (multiproc) metric aggregation.

In multiproc mode the request store is backed by per-process shard files that
the renderer merges at read time. Cross-process merging and dead-worker pruning
are covered by the Rust unit tests (they need real pids); here we verify the
Python-facing behavior: enabling/disabling, and that aggregate()/metrics_text()
read from the shard while enabled.
"""
import pytest

import rust_py_monitor
from rust_py_monitor import (
    get_multiproc_dir,
    multiproc_enabled,
    set_multiproc_dir,
)
from rust_py_monitor._core import clear_requests, record_request


@pytest.fixture(autouse=True)
def isolate_multiproc():
    clear_requests()
    set_multiproc_dir(None)  # ensure we start in single-process mode
    yield
    # Critical: always disable, otherwise later tests' aggregate() would read
    # shards instead of the in-process store.
    set_multiproc_dir(None)
    clear_requests()


def test_disabled_by_default():
    assert multiproc_enabled() is False
    assert get_multiproc_dir() is None


def test_enable_and_disable(tmp_path):
    set_multiproc_dir(str(tmp_path))
    assert multiproc_enabled() is True
    assert get_multiproc_dir() == str(tmp_path)

    set_multiproc_dir(None)
    assert multiproc_enabled() is False
    assert get_multiproc_dir() is None


def test_creates_shard_file(tmp_path):
    set_multiproc_dir(str(tmp_path))
    record_request("GET", "/x", 200, 10.0)
    shards = list(tmp_path.glob("rpy-*.shard"))
    assert len(shards) == 1


def test_aggregate_reads_from_shard(tmp_path):
    set_multiproc_dir(str(tmp_path))
    record_request("GET", "/ok", 200, 10.0)
    record_request("GET", "/ok", 200, 10.0)
    record_request("GET", "/nf", 404, 10.0)

    stats = rust_py_monitor.aggregate()
    assert stats.total_requests == 3
    assert stats.total_errors == 1
    assert abs(stats.error_rate - (1 / 3 * 100)) < 0.01
    assert abs(stats.avg_latency_ms - 10.0) < 0.01
    # All samples land in the (5, 10]ms bucket → percentiles within that band.
    assert 5.0 <= stats.p95_latency_ms <= 10.0


def test_metrics_text_reflects_shard(tmp_path):
    set_multiproc_dir(str(tmp_path))
    for _ in range(5):
        record_request("GET", "/x", 200, 3.0)
    record_request("GET", "/fail", 500, 3.0)

    text = rust_py_monitor.metrics_text()
    assert "rpy_requests_total 6" in text
    assert "rpy_errors_total 1" in text


def test_switching_back_to_local_restores_exact_path(tmp_path):
    # Enable, record into shard.
    set_multiproc_dir(str(tmp_path))
    record_request("GET", "/x", 200, 10.0)
    assert rust_py_monitor.aggregate().total_requests == 1

    # Disable: aggregate() must now use the in-process store. The shard request
    # was also recorded locally (record always feeds the local ring buffer), so
    # we clear to get a clean local count.
    set_multiproc_dir(None)
    clear_requests()
    assert rust_py_monitor.aggregate().total_requests == 0
    record_request("GET", "/y", 200, 5.0)
    assert rust_py_monitor.aggregate().total_requests == 1
