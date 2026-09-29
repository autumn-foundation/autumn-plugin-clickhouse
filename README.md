# autumn-plugin-clickhouse

A ClickHouse OLAP client for the [Autumn](https://autumn-web.app) web framework.

This is the OLAP/analytics counterpart to the DuckDB plugin: where DuckDB embeds an
analytical database in the app process, this plugin talks to a remote ClickHouse server
over HTTP. It gives Autumn handlers typed `SELECT` queries, batch inserts through the
`clickhouse` inserter API, and ordered `CREATE TABLE IF NOT EXISTS` migrations.

## Quickstart

Add the plugin to the app, then take the `ClickHouse` extractor in a handler:

```rust
use autumn_plugin_clickhouse::{ClickHouse, ClickHousePlugin, ClickHouseResultExt as _};
use autumn_web::prelude::*;

#[derive(clickhouse::Row, serde::Deserialize)]
struct EventCount {
    name: String,
    count: u64,
}

#[get("/events/counts")]
async fn event_counts(db: ClickHouse) -> AutumnResult<Json<Vec<EventCount>>> {
    let rows = db
        .fetch_all::<EventCount>("SELECT name, count() AS count FROM events GROUP BY name")
        .await
        .or_http()?;
    Ok(Json(rows))
}

autumn_web::app()
    .plugin(ClickHousePlugin::new())
    .routes(routes![event_counts])
    .run()
    .await;
```

Configure it in `autumn.toml`:

```toml
[clickhouse]
url = "http://localhost:8123"
database = "analytics"
user = "default"
password = "secret"   # or AUTUMN_CLICKHOUSE__PASSWORD
timeout_ms = 5000
health_check = true
```

Secrets also come from the environment: `AUTUMN_CLICKHOUSE__URL`,
`AUTUMN_CLICKHOUSE__DATABASE`, `AUTUMN_CLICKHOUSE__USER`, `AUTUMN_CLICKHOUSE__PASSWORD`.
Environment variables override every file layer.

## What it gives

- **Typed queries**: `db.fetch_all::<Row>("SELECT ...")` and `db.fetch_one::<Row>(...)`
  with `#[derive(clickhouse::Row)]` structs.
- **Batch inserts**: `db.insert("events", &rows)` for one batch, or `db.inserter("events")`
  for a long-lived batcher with row/byte/period limits from the config.
- **Migrations**: `migrate(&db, &[Migration::create_table("events", "ts DateTime, name String")?])`
  runs ordered DDL and stops at the first failure.
- **Health check**: `SELECT 1`, registered as `HealthOnly` (a down ClickHouse never blocks deploys).
- **Metrics**: `clickhouse_queries_total`, `clickhouse_inserts_total`,
  `clickhouse_insert_rows_total`, `clickhouse_ddl_total`.
- **Config**: layered `[clickhouse]` section (defaults → `autumn.toml` →
  `[profile.<name>.clickhouse]` → `autumn-<name>.toml` → `AUTUMN_CLICKHOUSE__*`),
  plus `ClickHousePlugin::configure(|c| ...)` and `ClickHousePlugin::config(...)`.

## Example

`examples/analytics.rs` migrates an events table, inserts 1,000 rows, and prints counts.
It needs a reachable ClickHouse:

```sh
AUTUMN_CLICKHOUSE__URL=http://localhost:8123 cargo run --example analytics
```

## Testing

Unit tests use the `clickhouse` crate's mock server (`test-util` feature), so no live
server is needed:

```sh
cargo test --locked --all-targets --all-features
```

## Building

The crate declares `rust-version = "1.88"`, but the `clickhouse` 0.14 dependency needs
rustc 1.89 or newer to compile — use a current stable toolchain.

## Known issues

- The `Debug` output of `ClickHouseConfig` redacts the password, and error text never
  shows server messages or row data. If you log `ClickHouseError::detail()`, you log
  user data — keep it out of user-facing output.
- The startup ping stops the boot when ClickHouse is unreachable. For apps that must
  boot without ClickHouse, do not add the plugin, or gate it behind your own flag.

## License

Apache-2.0.
