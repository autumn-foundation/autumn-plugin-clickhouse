use super::*;

fn database(kind: ErrorKind) -> ClickHouseError {
    ClickHouseError::Database {
        kind,
        detail: "secret detail".to_owned(),
    }
}

#[test]
fn status_codes() {
    use http::StatusCode;
    assert_eq!(
        ClickHouseError::Timeout {
            timeout: Duration::from_secs(1)
        }
        .status(),
        StatusCode::GATEWAY_TIMEOUT
    );
    assert_eq!(
        database(ErrorKind::TimedOut).status(),
        StatusCode::GATEWAY_TIMEOUT
    );
    assert_eq!(
        database(ErrorKind::Network).status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        database(ErrorKind::BadResponse).status(),
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(
        database(ErrorKind::NotFound).status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        database(ErrorKind::InvalidParams).status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        database(ErrorKind::SchemaMismatch).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        database(ErrorKind::Decode).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        database(ErrorKind::Unsupported).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        database(ErrorKind::Custom).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        database(ErrorKind::Unknown).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        ClickHouseError::Migration {
            name: "m".to_owned(),
            detail: "d".to_owned(),
        }
        .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        ClickHouseError::NotInstalled.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        ClickHouseError::from(ConfigError("bad".to_owned())).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[test]
fn retryable_kinds() {
    assert!(
        ClickHouseError::Timeout {
            timeout: Duration::from_secs(1)
        }
        .is_retryable()
    );
    assert!(database(ErrorKind::TimedOut).is_retryable());
    assert!(database(ErrorKind::Network).is_retryable());
    assert!(!database(ErrorKind::BadResponse).is_retryable());
    assert!(!database(ErrorKind::NotFound).is_retryable());
    assert!(!database(ErrorKind::InvalidParams).is_retryable());
    assert!(!ClickHouseError::NotInstalled.is_retryable());
}

#[test]
fn text_and_debug_hide_the_detail() {
    let err = database(ErrorKind::BadResponse);
    assert_eq!(err.to_string(), "ClickHouse refused the call: bad response");
    assert_eq!(
        format!("{err:?}"),
        "ClickHouseError(\"ClickHouse refused the call: bad response\")"
    );
    assert_eq!(err.detail(), Some("secret detail"));
    assert_eq!(err.kind(), Some(ErrorKind::BadResponse));

    let migration = ClickHouseError::Migration {
        name: "create table events".to_owned(),
        detail: "secret detail".to_owned(),
    };
    assert_eq!(
        migration.to_string(),
        "the migration `create table events` failed"
    );
    assert!(!format!("{migration:?}").contains("secret detail"));
    assert_eq!(migration.detail(), Some("secret detail"));
    assert_eq!(migration.kind(), None);

    assert_eq!(ClickHouseError::NotInstalled.detail(), None);
    assert_eq!(
        ClickHouseError::Timeout {
            timeout: Duration::from_secs(1)
        }
        .detail(),
        None
    );
}

#[test]
fn maps_clickhouse_errors() {
    use clickhouse::error::Error as Ch;
    let err = ClickHouseError::from(Ch::TimedOut);
    assert_eq!(err.kind(), Some(ErrorKind::TimedOut));
    assert!(err.is_retryable());

    let err = ClickHouseError::from(Ch::RowNotFound);
    assert_eq!(err.kind(), Some(ErrorKind::NotFound));
    assert_eq!(err.status(), StatusCode::NOT_FOUND);

    let err = ClickHouseError::from(Ch::BadResponse("Code: 209".to_owned()));
    assert_eq!(err.kind(), Some(ErrorKind::BadResponse));
    assert!(err.detail().is_some());

    let err = ClickHouseError::from(Ch::SchemaMismatch("no such column".to_owned()));
    assert_eq!(err.kind(), Some(ErrorKind::SchemaMismatch));

    let err = ClickHouseError::from(Ch::NotEnoughData);
    assert_eq!(err.kind(), Some(ErrorKind::Decode));
}

#[test]
fn or_http_converts() {
    use http::StatusCode;
    let result: Result<(), ClickHouseError> = Err(database(ErrorKind::Network));
    let err = result.or_http().unwrap_err();
    assert_eq!(err.status(), StatusCode::SERVICE_UNAVAILABLE);
    let result: Result<(), ClickHouseError> = Err(database(ErrorKind::NotFound));
    let err = result.or_http().unwrap_err();
    assert_eq!(err.status(), StatusCode::NOT_FOUND);
}
