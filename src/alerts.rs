//! Simple threshold alerts over the current process metrics.
//!
//! `check_alerts(...)` is **stateless**: you pass the thresholds you want to
//! watch and it returns the alerts that fired against a freshly collected
//! snapshot. An alert fires when the metric **exceeds** its threshold
//! (`value > threshold`). Only the thresholds you provide are evaluated.
//!
//! The evaluation logic lives in a pure [`evaluate`] function (unit-tested with
//! explicit values), while the `#[pyfunction]` wires it to a live snapshot.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::snapshot;

/// Thresholds to watch. Each is optional; only the ones set are evaluated.
/// Memory thresholds are in **megabytes** to match `Snapshot.memory_rss_mb`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Thresholds {
    pub cpu_percent: Option<f64>,
    pub memory_rss_mb: Option<f64>,
    pub memory_virtual_mb: Option<f64>,
}

/// A fired alert. `metric` names which threshold tripped.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub metric: &'static str,
    pub value: f64,
    pub threshold: f64,
}

/// Pure evaluation: returns the alerts that fired for the given current values.
/// An alert fires when `value > threshold`. This is deterministic and easy to
/// test without touching the live system.
pub fn evaluate(
    cpu_percent: f64,
    memory_rss_mb: f64,
    memory_virtual_mb: f64,
    thresholds: &Thresholds,
) -> Vec<Alert> {
    let mut fired = Vec::new();
    let mut check = |metric: &'static str, value: f64, threshold: Option<f64>| {
        if let Some(t) = threshold {
            if value > t {
                fired.push(Alert {
                    metric,
                    value,
                    threshold: t,
                });
            }
        }
    };
    check("cpu_percent", cpu_percent, thresholds.cpu_percent);
    check("memory_rss_mb", memory_rss_mb, thresholds.memory_rss_mb);
    check(
        "memory_virtual_mb",
        memory_virtual_mb,
        thresholds.memory_virtual_mb,
    );
    fired
}

/// `check_alerts(cpu_percent=None, memory_rss_mb=None, memory_virtual_mb=None)`.
///
/// Collects a snapshot of the current process and returns the alerts that fired
/// as a list of dicts: `{"metric", "value", "threshold", "severity"}`. Memory
/// thresholds are in megabytes. `severity` is currently always `"warning"`.
#[pyfunction]
#[pyo3(signature = (cpu_percent=None, memory_rss_mb=None, memory_virtual_mb=None))]
pub fn check_alerts<'py>(
    py: Python<'py>,
    cpu_percent: Option<f64>,
    memory_rss_mb: Option<f64>,
    memory_virtual_mb: Option<f64>,
) -> PyResult<Bound<'py, PyList>> {
    let snap = snapshot::collect().map_err(PyRuntimeError::new_err)?;
    let rss_mb = snap.memory_rss as f64 / 1024.0 / 1024.0;
    let virt_mb = snap.memory_virtual as f64 / 1024.0 / 1024.0;

    let thresholds = Thresholds {
        cpu_percent,
        memory_rss_mb,
        memory_virtual_mb,
    };
    let fired = evaluate(snap.cpu_percent, rss_mb, virt_mb, &thresholds);

    let list = PyList::empty(py);
    for alert in fired {
        let d = PyDict::new(py);
        d.set_item("metric", alert.metric)?;
        d.set_item("value", alert.value)?;
        d.set_item("threshold", alert.threshold)?;
        d.set_item("severity", "warning")?;
        list.append(d)?;
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_fires_below_thresholds() {
        let t = Thresholds {
            cpu_percent: Some(80.0),
            memory_rss_mb: Some(500.0),
            memory_virtual_mb: None,
        };
        assert!(evaluate(50.0, 100.0, 400.0, &t).is_empty());
    }

    #[test]
    fn cpu_fires_when_exceeded() {
        let t = Thresholds {
            cpu_percent: Some(80.0),
            ..Default::default()
        };
        let fired = evaluate(91.2, 100.0, 400.0, &t);
        assert_eq!(
            fired,
            vec![Alert {
                metric: "cpu_percent",
                value: 91.2,
                threshold: 80.0
            }]
        );
    }

    #[test]
    fn only_provided_thresholds_are_evaluated() {
        let t = Thresholds {
            memory_rss_mb: Some(50.0),
            ..Default::default()
        };
        let fired = evaluate(99.0, 100.0, 9999.0, &t);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].metric, "memory_rss_mb");
    }

    #[test]
    fn equal_value_does_not_fire() {
        let t = Thresholds {
            cpu_percent: Some(50.0),
            ..Default::default()
        };
        assert!(evaluate(50.0, 0.0, 0.0, &t).is_empty());
    }

    #[test]
    fn multiple_alerts_at_once() {
        let t = Thresholds {
            cpu_percent: Some(10.0),
            memory_rss_mb: Some(1.0),
            memory_virtual_mb: Some(1.0),
        };
        let fired = evaluate(50.0, 100.0, 400.0, &t);
        assert_eq!(fired.len(), 3);
    }

    #[test]
    fn empty_thresholds_fire_nothing() {
        assert!(evaluate(100.0, 100.0, 100.0, &Thresholds::default()).is_empty());
    }
}
