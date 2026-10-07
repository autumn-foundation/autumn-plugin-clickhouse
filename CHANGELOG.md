# Changelog

## Unreleased

- Require `autumn-web` 0.8 (`>=0.8, <0.9`).
- Remove an unknown clippy lint name from the `FromRequestParts` impl.

## 0.1.0 — 2026-09-29

First release.

- `ClickHouse` handle: typed `fetch_all`/`fetch_one` with `#[derive(clickhouse::Row)]`,
  one-batch `insert`, batching `inserter` with config limits, `execute` for DDL, `ping`.
- `ClickHousePlugin` with `::new()`, `.config_section()`, `.config()`, `.configure()`.
  The startup hook pings ClickHouse and puts the handle in the app state.
- `ClickHouse` extractor for Autumn handlers, plus `ClickHouse::from_state` for jobs.
- `migrate` module: ordered `CREATE TABLE IF NOT EXISTS` migrations,
  `Migration::create_table` DDL builder with identifier validation.
- `[clickhouse]` layered config: `url`, `database`, `user`, `password`, `timeout_ms`,
  `health_check`, `inserter_max_rows`, `inserter_max_bytes`, `inserter_period_secs`;
  `AUTUMN_CLICKHOUSE__*` environment overrides. Passwords never appear in `Debug`.
- `HealthOnly` health check (`SELECT 1`) and Prometheus call counters.
- `ClickHouseError` with `ErrorKind`, HTTP status mapping, `is_retryable`, and
  `ClickHouseResultExt::or_http`.
- `examples/analytics.rs`: migrate, insert 1,000 events, query counts.
- Unit tests run against the `clickhouse` mock server; no live server needed.
