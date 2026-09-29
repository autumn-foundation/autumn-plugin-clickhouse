//! Call counters for the plugin. The plugin module adds them to Autumn.
//!
//! # Contract
//!
//! - `clickhouse_queries_total` counts the queries that ended, with the label `outcome`.
//! - `clickhouse_inserts_total` counts the insert calls that ended, with the label `outcome`.
//! - `clickhouse_insert_rows_total` counts the rows of successful inserts.
//! - `clickhouse_ddl_total` counts the DDL statements that ended, with the label `outcome`.
//! - No metric name starts with `autumn_`. Each counter name ends with `_total`.

use std::sync::atomic::{AtomicU64, Ordering};

use autumn_web::actuator::{MetricFamily, MetricKind, MetricSample};

use crate::error::ClickHouseError;

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Succeeded,
    Failed,
    TimedOut,
}

impl Outcome {
    /// Maps a call result to an outcome.
    pub(crate) const fn of<T>(result: &Result<T, ClickHouseError>) -> Self {
        match result {
            Ok(_) => Self::Succeeded,
            Err(
                ClickHouseError::Timeout { .. }
                | ClickHouseError::Database {
                    kind: crate::error::ErrorKind::TimedOut,
                    ..
                },
            ) => Self::TimedOut,
            Err(_) => Self::Failed,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
        }
    }
}

/// Counters for all calls of one client.
#[derive(Debug, Default)]
pub struct Metrics {
    queries_succeeded: AtomicU64,
    queries_failed: AtomicU64,
    queries_timed_out: AtomicU64,
    inserts_succeeded: AtomicU64,
    inserts_failed: AtomicU64,
    inserts_timed_out: AtomicU64,
    insert_rows: AtomicU64,
    ddl_succeeded: AtomicU64,
    ddl_failed: AtomicU64,
    ddl_timed_out: AtomicU64,
}

impl Metrics {
    /// Records the end of a query.
    pub(crate) fn query_ended(&self, outcome: Outcome) {
        let counter = match outcome {
            Outcome::Succeeded => &self.queries_succeeded,
            Outcome::Failed => &self.queries_failed,
            Outcome::TimedOut => &self.queries_timed_out,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Records the end of an insert. A successful insert adds its rows.
    pub(crate) fn insert_ended(&self, outcome: Outcome, rows: u64) {
        let counter = match outcome {
            Outcome::Succeeded => &self.inserts_succeeded,
            Outcome::Failed => &self.inserts_failed,
            Outcome::TimedOut => &self.inserts_timed_out,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        if outcome == Outcome::Succeeded {
            self.insert_rows.fetch_add(rows, Ordering::Relaxed);
        }
    }

    /// Records the end of a DDL statement.
    pub(crate) fn ddl_ended(&self, outcome: Outcome) {
        let counter = match outcome {
            Outcome::Succeeded => &self.ddl_succeeded,
            Outcome::Failed => &self.ddl_failed,
            Outcome::TimedOut => &self.ddl_timed_out,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// The counter families.
    #[must_use]
    pub fn families(&self) -> Vec<MetricFamily> {
        let load = |counter: &AtomicU64| value(counter.load(Ordering::Relaxed));
        vec![
            family(
                "clickhouse_queries_total",
                "ClickHouse queries that ended, by outcome.",
                MetricKind::Counter,
                [
                    (
                        "outcome",
                        Outcome::Succeeded.label(),
                        load(&self.queries_succeeded),
                    ),
                    (
                        "outcome",
                        Outcome::Failed.label(),
                        load(&self.queries_failed),
                    ),
                    (
                        "outcome",
                        Outcome::TimedOut.label(),
                        load(&self.queries_timed_out),
                    ),
                ],
            ),
            family(
                "clickhouse_inserts_total",
                "ClickHouse insert calls that ended, by outcome.",
                MetricKind::Counter,
                [
                    (
                        "outcome",
                        Outcome::Succeeded.label(),
                        load(&self.inserts_succeeded),
                    ),
                    (
                        "outcome",
                        Outcome::Failed.label(),
                        load(&self.inserts_failed),
                    ),
                    (
                        "outcome",
                        Outcome::TimedOut.label(),
                        load(&self.inserts_timed_out),
                    ),
                ],
            ),
            single(
                "clickhouse_insert_rows_total",
                "Rows of successful ClickHouse inserts.",
                MetricKind::Counter,
                load(&self.insert_rows),
            ),
            family(
                "clickhouse_ddl_total",
                "ClickHouse DDL statements that ended, by outcome.",
                MetricKind::Counter,
                [
                    (
                        "outcome",
                        Outcome::Succeeded.label(),
                        load(&self.ddl_succeeded),
                    ),
                    ("outcome", Outcome::Failed.label(), load(&self.ddl_failed)),
                    (
                        "outcome",
                        Outcome::TimedOut.label(),
                        load(&self.ddl_timed_out),
                    ),
                ],
            ),
        ]
    }
}

fn single(name: &str, help: &str, kind: MetricKind, value: f64) -> MetricFamily {
    MetricFamily {
        name: name.to_owned(),
        help: help.to_owned(),
        kind,
        samples: vec![MetricSample {
            labels: Vec::new(),
            value,
        }],
    }
}

fn family<'a>(
    name: &str,
    help: &str,
    kind: MetricKind,
    samples: impl IntoIterator<Item = (&'a str, &'a str, f64)>,
) -> MetricFamily {
    MetricFamily {
        name: name.to_owned(),
        help: help.to_owned(),
        kind,
        samples: samples
            .into_iter()
            .map(|(key, label, value)| MetricSample {
                labels: vec![(key.to_owned(), label.to_owned())],
                value,
            })
            .collect(),
    }
}

#[allow(clippy::cast_precision_loss, reason = "Prometheus values are f64")]
const fn value(raw: u64) -> f64 {
    raw as f64
}

#[cfg(test)]
mod tests;
