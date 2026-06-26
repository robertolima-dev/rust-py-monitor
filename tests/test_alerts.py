"""check_alerts: threshold alerts over the current process snapshot.

The comparison logic is verified deterministically in the Rust unit tests
(`src/alerts.rs`) with explicit values. Here we drive the live `check_alerts`
deterministically by leaning on an invariant: a running process always has
RSS memory > 0, so a `memory_rss_mb=0` threshold always fires and a huge one
never does.
"""

import rust_py_monitor


def test_no_thresholds_returns_empty():
    assert rust_py_monitor.check_alerts() == []


def test_memory_threshold_zero_always_fires():
    fired = rust_py_monitor.check_alerts(memory_rss_mb=0)
    assert len(fired) == 1
    alert = fired[0]
    assert alert["metric"] == "memory_rss_mb"
    assert alert["severity"] == "warning"
    assert alert["threshold"] == 0
    assert alert["value"] > 0  # RSS is always positive for a live process


def test_huge_threshold_never_fires():
    # 10 TB in MB — no test process uses this.
    assert rust_py_monitor.check_alerts(memory_rss_mb=10_000_000) == []


def test_multiple_alerts_at_once():
    fired = rust_py_monitor.check_alerts(memory_rss_mb=0, memory_virtual_mb=0)
    metrics = sorted(a["metric"] for a in fired)
    assert metrics == ["memory_rss_mb", "memory_virtual_mb"]


def test_only_provided_thresholds_are_evaluated():
    # Only memory is watched; cpu is ignored even though it's not passed.
    fired = rust_py_monitor.check_alerts(memory_rss_mb=0)
    assert all(a["metric"] == "memory_rss_mb" for a in fired)
