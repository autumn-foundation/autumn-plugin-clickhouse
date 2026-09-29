use clickhouse::test::{self, Mock};

use super::*;
use crate::client::ClickHouse;
use crate::config::ClickHouseConfig;
use crate::error::ClickHouseError;

/// A handle that talks to the mock server.
fn handle() -> (ClickHouse, Mock) {
    let mock = Mock::new();
    let client = clickhouse::Client::default().with_mock(&mock);
    let db = ClickHouse::with_client(client, ClickHouseConfig::default());
    (db, mock)
}

#[test]
fn create_table_builds_merge_tree_ddl() {
    let migration = Migration::create_table("events", "ts DateTime, name String").unwrap();
    assert_eq!(migration.name, "create table events");
    assert_eq!(
        migration.ddl,
        "CREATE TABLE IF NOT EXISTS events (ts DateTime, name String) ENGINE = MergeTree() ORDER BY tuple()"
    );
}

#[test]
fn create_table_rejects_bad_names() {
    for name in [
        "",
        "123",
        "has space",
        "semi;colon",
        "dash-ed",
        "drop;table",
    ] {
        let err = Migration::create_table(name, "n UInt64").unwrap_err();
        assert!(
            err.to_string().contains("not an identifier"),
            "unexpected error for {name:?}: {err}"
        );
    }
    assert!(Migration::create_table("ok_name", "n UInt64").is_ok());
    assert!(Migration::create_table("db.events", "n UInt64").is_ok());
    assert!(Migration::create_table("_events", "n UInt64").is_ok());
}

#[test]
fn new_keeps_name_and_ddl() {
    let migration = Migration::new("seed", "INSERT INTO events SELECT 1");
    assert_eq!(migration.name, "seed");
    assert_eq!(migration.ddl, "INSERT INTO events SELECT 1");
}

#[tokio::test]
async fn migrate_runs_ddl_in_order() {
    let (db, mock) = handle();
    let first = mock.add(test::handlers::record_ddl());
    let second = mock.add(test::handlers::record_ddl());
    let migrations = vec![
        Migration::new(
            "first",
            "CREATE TABLE IF NOT EXISTS a (n UInt64) ENGINE = Memory",
        ),
        Migration::new(
            "second",
            "CREATE TABLE IF NOT EXISTS b (n UInt64) ENGINE = Memory",
        ),
    ];
    migrate(&db, &migrations).await.unwrap();
    assert!(first.query().await.contains("CREATE TABLE IF NOT EXISTS a"));
    assert!(
        second
            .query()
            .await
            .contains("CREATE TABLE IF NOT EXISTS b")
    );
}

#[tokio::test]
async fn migrate_stops_at_the_first_failure() {
    let (db, mock) = handle();
    mock.add(test::handlers::exception(60));
    let migrations = vec![
        Migration::new("bad", "CREATE TABLE bad (n BadType) ENGINE = Memory"),
        Migration::new(
            "next",
            "CREATE TABLE IF NOT EXISTS ok (n UInt64) ENGINE = Memory",
        ),
    ];
    let err = migrate(&db, &migrations).await.unwrap_err();
    match err {
        ClickHouseError::Migration { name, .. } => assert_eq!(name, "bad"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn migrate_with_no_migrations_succeeds() {
    let (db, _mock) = handle();
    migrate(&db, &[]).await.unwrap();
}
