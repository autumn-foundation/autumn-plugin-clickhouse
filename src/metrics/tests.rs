use super::*;
use crate::error::ErrorKind;

fn find<'a>(families: &'a [MetricFamily], name: &str) -> &'a MetricFamily {
    families
        .iter()
        .find(|family| family.name == name)
        .unwrap_or_else(|| panic!("missing metric {name}"))
}

fn sample_value(family: &MetricFamily, label: &str) -> f64 {
    family
        .samples
        .iter()
        .find(|sample| {
            sample
                .labels
                .iter()
                .any(|(key, value)| key == "outcome" && value == label)
        })
        .unwrap_or_else(|| panic!("missing outcome {label}"))
        .value
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "counters are whole numbers; f64 equality is exact here"
)]
fn counters_start_at_zero() {
    let metrics = Metrics::default();
    let families = metrics.families();
    assert_eq!(families.len(), 4);
    for family in &families {
        assert!(
            family.name.starts_with("clickhouse_"),
            "bad name: {}",
            family.name
        );
        assert!(family.name.ends_with("_total"), "bad name: {}", family.name);
        for sample in &family.samples {
            assert_eq!(sample.value, 0.0);
        }
    }
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "counters are whole numbers; f64 equality is exact here"
)]
fn outcomes_land_in_the_right_buckets() {
    let metrics = Metrics::default();
    metrics.query_ended(Outcome::Succeeded);
    metrics.query_ended(Outcome::Failed);
    metrics.query_ended(Outcome::TimedOut);
    metrics.insert_ended(Outcome::Succeeded, 42);
    metrics.insert_ended(Outcome::Failed, 7);
    metrics.ddl_ended(Outcome::TimedOut);

    let families = metrics.families();
    let queries = find(&families, "clickhouse_queries_total");
    assert_eq!(sample_value(queries, "succeeded"), 1.0);
    assert_eq!(sample_value(queries, "failed"), 1.0);
    assert_eq!(sample_value(queries, "timed_out"), 1.0);

    let inserts = find(&families, "clickhouse_inserts_total");
    assert_eq!(sample_value(inserts, "succeeded"), 1.0);
    assert_eq!(sample_value(inserts, "failed"), 1.0);
    assert_eq!(sample_value(inserts, "timed_out"), 0.0);

    let rows = find(&families, "clickhouse_insert_rows_total");
    assert_eq!(rows.samples.len(), 1);
    assert_eq!(rows.samples[0].value, 42.0);

    let ddl = find(&families, "clickhouse_ddl_total");
    assert_eq!(sample_value(ddl, "timed_out"), 1.0);
    assert_eq!(sample_value(ddl, "succeeded"), 0.0);
}

#[test]
fn outcome_of_maps_errors() {
    let ok: Result<(), ClickHouseError> = Ok(());
    assert_eq!(Outcome::of(&ok), Outcome::Succeeded);
    let timed_out: Result<(), ClickHouseError> = Err(ClickHouseError::Timeout {
        timeout: std::time::Duration::from_secs(1),
    });
    assert_eq!(Outcome::of(&timed_out), Outcome::TimedOut);
    let failed: Result<(), ClickHouseError> = Err(ClickHouseError::Database {
        kind: ErrorKind::BadResponse,
        detail: String::new(),
    });
    assert_eq!(Outcome::of(&failed), Outcome::Failed);
    let crate_timeout: Result<(), ClickHouseError> = Err(ClickHouseError::Database {
        kind: ErrorKind::TimedOut,
        detail: String::new(),
    });
    assert_eq!(Outcome::of(&crate_timeout), Outcome::TimedOut);
}
