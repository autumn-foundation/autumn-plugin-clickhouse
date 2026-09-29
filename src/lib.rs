//! Autumn plugin for ClickHouse.
//!
//! Add [`ClickHousePlugin`] to the app. Then use the [`ClickHouse`] extractor in a handler.
//! ClickHouse is the OLAP/analytics counterpart to the `DuckDB` plugin: it queries a remote
//! ClickHouse server over HTTP instead of an embedded database.
//!
//! ```rust,no_run
//! use autumn_plugin_clickhouse::{ClickHouse, ClickHousePlugin, ClickHouseResultExt as _};
//! use autumn_web::prelude::*;
//!
//! #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
//! struct EventCount {
//!     name: String,
//!     count: u64,
//! }
//!
//! #[get("/events/counts")]
//! async fn event_counts(db: ClickHouse) -> AutumnResult<Json<Vec<EventCount>>> {
//!     let rows = db
//!         .fetch_all::<EventCount>("SELECT name, count() AS count FROM events GROUP BY name")
//!         .await
//!         .or_http()?;
//!     Ok(Json(rows))
//! }
//!
//! # async fn run() {
//! autumn_web::app()
//!     .plugin(ClickHousePlugin::new().configure(|c| {
//!         c.url = "http://localhost:8123".into();
//!         c.database = "analytics".into();
//!     }))
//!     .routes(routes![event_counts])
//!     .run()
//!     .await;
//! # }
//! ```
//!
//! The plugin reads `[clickhouse]` in `autumn.toml`. See [`config`] for the keys.
//!
//! # What the plugin gives
//!
//! - [`ClickHouse`]: typed `SELECT` queries with `#[derive(clickhouse::Row)]` structs,
//!   one-batch [`ClickHouse::insert`] and the batching [`Inserter`],
//!   plus [`ClickHouse::execute`] for DDL.
//! - [`migrate`]: runs ordered `CREATE TABLE IF NOT EXISTS` migrations at startup.
//! - A `HealthOnly` readiness check that runs `SELECT 1`, and Prometheus call counters.
//!
//! # Limits
//!
//! - Each call has the `timeout_ms` timeout. A timed-out call gives [`ClickHouseError::Timeout`].
//! - Rows must be owned structs: `#[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]`
//!   without lifetimes.
//! - Logs, error text and `Debug` output do not have row data, server messages or passwords.
//! - The plugin needs a reachable ClickHouse server. The startup ping stops the boot if the
//!   server is down. Tests use the `clickhouse` mock server, not a live server.

mod client;
pub mod config;
mod error;
mod health;
mod metrics;
mod migrate;
mod plugin;

/// The `clickhouse` crate that the plugin uses. Use it for the `Row` derive.
pub use clickhouse;
pub use client::{ClickHouse, InsertStats, Inserter};
pub use config::{ClickHouseConfig, ConfigError, DEFAULT_SECTION};
pub use error::{ClickHouseError, ClickHouseResultExt, ErrorKind};
pub use metrics::Metrics;
pub use migrate::{Migration, migrate};
pub use plugin::{ClickHousePlugin, PLUGIN_NAME};
