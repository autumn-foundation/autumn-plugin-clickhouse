//! Analytics example: migrate an events table, insert events, query counts.
//!
//! Needs a reachable ClickHouse server. Point the plugin at it with
//! `AUTUMN_CLICKHOUSE__URL` (or a `[clickhouse]` section in `autumn.toml`):
//!
//! ```sh
//! AUTUMN_CLICKHOUSE__URL=http://localhost:8123 cargo run --example analytics
//! ```
//!
//! The unit tests use the `clickhouse` mock server instead, so they need no live server.

use autumn_plugin_clickhouse::clickhouse::Row;
use autumn_plugin_clickhouse::{ClickHouse, ClickHouseConfig, Migration, migrate};

#[derive(Debug, Clone, PartialEq, Row, serde::Serialize, serde::Deserialize)]
struct Event {
    ts: u32,
    name: String,
    value: u64,
}

#[derive(Debug, Clone, PartialEq, Row, serde::Deserialize)]
struct EventCount {
    name: String,
    count: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = ClickHouseConfig::resolve("clickhouse")?;
    "analytics".clone_into(&mut config.database);
    let db = ClickHouse::new(&config);

    db.ping().await?;
    println!("connected to {}", config.url);

    db.execute("CREATE DATABASE IF NOT EXISTS analytics")
        .await?;
    migrate(
        &db,
        &[Migration::create_table(
            "analytics.events",
            "ts DateTime, name String, value UInt64",
        )?],
    )
    .await?;
    println!("migrations are done");

    let mut inserter = db.inserter::<Event>("analytics.events");
    for i in 0..1_000u64 {
        inserter
            .write(&Event {
                ts: 1_700_000_000,
                name: if i % 2 == 0 {
                    "click".to_owned()
                } else {
                    "view".to_owned()
                },
                value: i,
            })
            .await?;
    }
    let stats = inserter.end().await?;
    println!("inserted {} rows ({} bytes)", stats.rows, stats.bytes);

    let counts = db
        .fetch_all::<EventCount>(
            "SELECT name, count() AS count FROM analytics.events GROUP BY name ORDER BY name",
        )
        .await?;
    for row in &counts {
        println!("{}: {}", row.name, row.count);
    }
    Ok(())
}
