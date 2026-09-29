# Planning — autumn-plugin-clickhouse

## Goal

A ClickHouse OLAP client plugin for Autumn (lane 3: external crate). It is the
OLAP/analytics counterpart to the existing DuckDB plugin: DuckDB embeds the
analytical database, this plugin talks to a remote ClickHouse server over HTTP.

## Scope (v0.1)

- `ClickHouse` handle: typed `SELECT` (`fetch_all`, `fetch_one`), one-batch `insert`,
  batching `inserter` with row/byte/period limits, `execute` for DDL, `ping` (`SELECT 1`).
- `ClickHousePlugin`: `::new`, `.config_section`, `.config`, `.configure`; startup hook
  pings the server and installs the handle; `HealthOnly` health check; metrics source.
- `ClickHouse` extractor for handlers (`FromRequestParts`), `from_state` for jobs.
- `migrate` module: ordered `CREATE TABLE IF NOT EXISTS` DDL, `Migration::create_table`
  builder with identifier validation.
- `[clickhouse]` layered config with `AUTUMN_CLICKHOUSE__*` env overrides for secrets.
- `ClickHouseError` + `ErrorKind` + HTTP status mapping + `or_http`.
- `examples/analytics.rs` (needs a live server; documented).
- Unit tests against the `clickhouse` mock server — no live server in CI.

## Non-goals (v0.1)

- Session store / cache backends on ClickHouse (row-oriented workloads do not fit).
- Native TCP protocol (the `clickhouse` crate is HTTP-only for now).
- Query builder DSL (raw SQL with `?fields`/`?` placeholders is enough).
- Automatic schema sync from `Row` types (validation stays server-side).

## Decisions

- Build on the official `clickhouse` crate 0.14 (`inserter` feature; `tls` feature for
  HTTPS via rustls). No compression features: the plugin asks for none.
- Health check is `HealthOnly`: an analytics sink must not block deploys.
- The startup ping fails the boot on unreachable ClickHouse (fail fast, loud error).
- Timeouts wrap every call with `tokio::time::timeout`; the config timeout rules.
- Error text shows kinds only; `detail()` carries server messages for operators.
