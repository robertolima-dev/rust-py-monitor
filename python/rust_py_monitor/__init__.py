from importlib.metadata import PackageNotFoundError, version

from rust_py_monitor._core import (  # noqa: F401
    PROMETHEUS_CONTENT_TYPE,
    AggregatedMetrics,
    RequestMetric,
    Snapshot,
    aggregate,
    clear_requests,
    get_max_requests,
    get_multiproc_dir,
    get_requests,
    hello,
    metrics_text,
    multiproc_enabled,
    set_max_requests,
    set_multiproc_dir,
    snapshot,
)

# Derive the version from the installed package metadata (single source of
# truth: pyproject.toml) instead of hardcoding it here, which previously drifted
# out of sync with the actual release.
try:
    __version__ = version("rust-py-monitor")
except PackageNotFoundError:  # pragma: no cover - source checkout w/o install
    __version__ = "0.0.0"

__all__ = [
    "hello",
    "snapshot",
    "Snapshot",
    "RequestMetric",
    "AggregatedMetrics",
    "get_requests",
    "clear_requests",
    "set_max_requests",
    "get_max_requests",
    "set_multiproc_dir",
    "get_multiproc_dir",
    "multiproc_enabled",
    "aggregate",
    "metrics_text",
    "PROMETHEUS_CONTENT_TYPE",
]
