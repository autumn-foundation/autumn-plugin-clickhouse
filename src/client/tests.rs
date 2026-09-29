use clickhouse::Row;
use clickhouse::test::{self, Mock};

use super::*;
use crate::config::ClickHouseConfig;
use crate::error::ErrorKind;

#[derive(Debug, Clone, PartialEq, Row, serde::Serialize, serde::Deserialize)]
struct Event {
    name: String,
    count: u64,
}

fn event(name: &str, count: u64) -> Event {
    Event {
        name: name.to_owned(),
        count,
    }
}

/// A handle that talks to the mock server.
fn handle() -> (ClickHouse, Mock) {
    let mock = Mock::new();
    let client = clickhouse::Client::default().with_mock(&mock);
    let db = ClickHouse::with_client(client, ClickHouseConfig::default());
    (db, mock)
}

#[tokio::test]
async fn ping_runs_select_1() {
    let (db, mock) = handle();
    let recording = mock.add(test::handlers::record_ddl());
    db.ping().await.unwrap();
    assert!(recording.query().await.contains("SELECT 1"));
}

#[tokio::test]
async fn fetch_all_returns_typed_rows() {
    let (db, mock) = handle();
    let rows = vec![event("click", 3), event("view", 7)];
    mock.add(test::handlers::provide(rows.clone()));
    let got = db
        .fetch_all::<Event>("SELECT ?fields FROM events")
        .await
        .unwrap();
    assert_eq!(got, rows);
}

#[tokio::test]
async fn fetch_one_returns_the_first_row() {
    let (db, mock) = handle();
    mock.add(test::handlers::provide(vec![event("click", 3)]));
    let got = db
        .fetch_one::<Event>("SELECT ?fields FROM events LIMIT 1")
        .await
        .unwrap();
    assert_eq!(got, event("click", 3));
}

#[tokio::test]
async fn fetch_one_with_no_rows_gives_not_found() {
    let (db, mock) = handle();
    mock.add(test::handlers::provide(Vec::<Event>::new()));
    let err = db
        .fetch_one::<Event>("SELECT ?fields FROM events LIMIT 1")
        .await
        .unwrap_err();
    assert_eq!(err.kind(), Some(ErrorKind::NotFound));
}

#[tokio::test]
async fn insert_writes_the_rows() {
    let (db, mock) = handle();
    let recording = mock.add(test::handlers::record());
    let rows = vec![event("click", 3), event("view", 7)];
    db.insert("events", &rows).await.unwrap();
    let got: Vec<Event> = recording.collect().await;
    assert_eq!(got, rows);
}

#[tokio::test]
async fn server_failure_maps_to_bad_response() {
    let (db, mock) = handle();
    mock.add(test::handlers::failure(test::status::FORBIDDEN));
    let err = db
        .fetch_all::<Event>("SELECT ?fields FROM events")
        .await
        .unwrap_err();
    assert_eq!(err.kind(), Some(ErrorKind::BadResponse));
    assert!(!err.is_retryable());
}

#[tokio::test]
async fn ddl_failure_maps_to_bad_response() {
    let (db, mock) = handle();
    mock.add(test::handlers::exception(209));
    let err = db
        .execute("CREATE TABLE events (name String)")
        .await
        .unwrap_err();
    assert_eq!(err.kind(), Some(ErrorKind::BadResponse));
}

#[tokio::test]
async fn inserter_batches_and_commits() {
    let (db, mock) = handle();
    let _recording = mock.add(test::handlers::record::<Event>());
    let mut inserter = db.inserter::<Event>("events");
    inserter.write(&event("click", 1)).await.unwrap();
    inserter.write(&event("click", 2)).await.unwrap();
    let stats = inserter.commit().await.unwrap();
    assert_eq!(stats.rows, 2);
    assert!(stats.bytes > 0);
    let end = inserter.end().await.unwrap();
    assert_eq!(end.rows, 0);
}
