import os
import time

import rust_py_monitor
from rust_py_monitor import Snapshot


# --- type and structure ---

def test_snapshot_returns_snapshot_object():
    m = rust_py_monitor.snapshot()
    assert isinstance(m, Snapshot)


def test_snapshot_has_expected_attributes():
    m = rust_py_monitor.snapshot()
    _ = m.pid
    _ = m.cpu_percent
    _ = m.memory_rss
    _ = m.memory_rss_mb
    _ = m.memory_virtual
    _ = m.threads
    _ = m.timestamp


def test_snapshot_is_immutable():
    m = rust_py_monitor.snapshot()
    try:
        m.pid = 0  # type: ignore[misc]
        assert False, "should have raised AttributeError"
    except AttributeError:
        pass


# --- correct values ---

def test_snapshot_pid_is_correct():
    m = rust_py_monitor.snapshot()
    assert m.pid == os.getpid()


def test_snapshot_rss_memory_is_positive():
    m = rust_py_monitor.snapshot()
    assert m.memory_rss > 0
    assert m.memory_rss_mb > 0.0


def test_snapshot_rss_mb_is_consistent():
    m = rust_py_monitor.snapshot()
    expected = m.memory_rss / 1024 / 1024
    assert abs(m.memory_rss_mb - expected) < 0.001


def test_snapshot_virtual_memory_is_positive():
    m = rust_py_monitor.snapshot()
    assert m.memory_virtual > 0


def test_snapshot_timestamp_is_recent():
    before = int(time.time()) - 2
    m = rust_py_monitor.snapshot()
    after = int(time.time()) + 2
    assert before <= m.timestamp <= after


def test_snapshot_cpu_is_nonnegative():
    m = rust_py_monitor.snapshot()
    assert m.cpu_percent >= 0.0


# --- repr and serialization ---

def test_snapshot_repr_contains_pid():
    m = rust_py_monitor.snapshot()
    assert str(m.pid) in repr(m)


def test_snapshot_repr_contains_rss():
    m = rust_py_monitor.snapshot()
    assert "rss=" in repr(m)


def test_snapshot_to_dict_returns_dict():
    m = rust_py_monitor.snapshot()
    d = m.to_dict()
    assert isinstance(d, dict)


def test_snapshot_to_dict_has_all_keys():
    m = rust_py_monitor.snapshot()
    d = m.to_dict()
    assert set(d.keys()) == {"pid", "cpu_percent", "memory_rss", "memory_virtual", "threads", "timestamp"}


def test_snapshot_to_dict_values_match():
    m = rust_py_monitor.snapshot()
    d = m.to_dict()
    assert d["pid"] == m.pid
    assert d["memory_rss"] == m.memory_rss
    assert d["timestamp"] == m.timestamp


# --- consistency across calls ---

def test_snapshot_pid_stable_across_calls():
    m1 = rust_py_monitor.snapshot()
    time.sleep(0.05)
    m2 = rust_py_monitor.snapshot()
    assert m1.pid == m2.pid


def test_snapshot_timestamp_does_not_go_back():
    m1 = rust_py_monitor.snapshot()
    time.sleep(0.05)
    m2 = rust_py_monitor.snapshot()
    assert m2.timestamp >= m1.timestamp
