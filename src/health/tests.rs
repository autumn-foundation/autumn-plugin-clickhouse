use autumn_web::actuator::{HealthIndicator, HealthStatus, IndicatorGroup};
use clickhouse::test::{self, Mock};

use super::*;
use crate::client::ClickHouse;
use crate::config::ClickHouseConfig;

fn check() -> ClickHouseCheck {
    ClickHouseCheck::new(Arc::new(Shared::default()))
}

#[tokio::test]
async fn down_before_the_plugin_starts() {
    let output = check().check().await;
    assert_eq!(output.status, HealthStatus::Down);
    assert_eq!(
        output.details.get("state"),
        Some(&serde_json::Value::String("not started".to_owned()))
    );
}

#[tokio::test]
async fn up_when_select_1_succeeds() {
    let mock = Mock::new();
    let client = clickhouse::Client::default().with_mock(&mock);
    mock.add(test::handlers::record_ddl());
    let shared = Arc::new(Shared::default());
    let db = ClickHouse::with_client(client, ClickHouseConfig::default());
    let _ = shared.handle.set(db);
    let output = ClickHouseCheck::new(shared).check().await;
    assert_eq!(output.status, HealthStatus::Up);
    assert_eq!(
        output.details.get("database"),
        Some(&serde_json::Value::String("default".to_owned()))
    );
}

#[tokio::test]
async fn down_when_the_server_refuses() {
    let mock = Mock::new();
    let client = clickhouse::Client::default().with_mock(&mock);
    mock.add(test::handlers::failure(test::status::FORBIDDEN));
    let shared = Arc::new(Shared::default());
    let db = ClickHouse::with_client(client, ClickHouseConfig::default());
    let _ = shared.handle.set(db);
    let output = ClickHouseCheck::new(shared).check().await;
    assert_eq!(output.status, HealthStatus::Down);
    assert_eq!(
        output.details.get("state"),
        Some(&serde_json::Value::String("unreachable".to_owned()))
    );
}

#[test]
fn is_health_only() {
    assert_eq!(check().group(), IndicatorGroup::HealthOnly);
    assert_eq!(check().timeout_ms(), 1_000);
}

#[test]
fn details_never_show_secrets() {
    let fields = [("database", "analytics"), ("url", "http://x")];
    let map = details(&fields);
    assert!(!format!("{map:?}").contains("secret"));
    assert_eq!(map.len(), 2);
}
