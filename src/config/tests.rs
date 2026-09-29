#![allow(
    clippy::field_reassign_with_default,
    reason = "each test changes one key of the defaults"
)]

use std::path::Path;

use autumn_web::config::MockEnv;

use super::*;

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

fn env_for(dir: &Path) -> MockEnv {
    MockEnv::new().with("AUTUMN_MANIFEST_DIR", dir.to_str().unwrap())
}

fn resolve(env: &MockEnv) -> Result<ClickHouseConfig, ConfigError> {
    ClickHouseConfig::resolve_with_env("clickhouse", env)
}

fn invalid(change: impl FnOnce(&mut ClickHouseConfig)) -> String {
    let mut config = ClickHouseConfig::default();
    change(&mut config);
    config.validate().unwrap_err().to_string()
}

#[test]
fn defaults_are_safe() {
    let config = ClickHouseConfig::default();
    assert_eq!(config.url, "http://localhost:8123");
    assert_eq!(config.database, "default");
    assert_eq!(config.user, "default");
    assert_eq!(config.password, "");
    assert_eq!(config.timeout(), Duration::from_secs(5));
    assert!(config.health_check);
    assert_eq!(config.inserter_max_rows, 100_000);
    assert_eq!(config.inserter_max_bytes, 16 * 1024 * 1024);
    assert_eq!(config.inserter_period_secs, 5);
    assert_eq!(config.inserter_period(), Duration::from_secs(5));
    config.validate().unwrap();
}

#[test]
fn debug_hides_the_password() {
    let mut config = ClickHouseConfig::default();
    config.password = "secret".to_owned();
    let text = format!("{config:?}");
    assert!(!text.contains("secret"));
    assert!(text.contains("<redacted>"));
}

#[test]
fn no_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        resolve(&env_for(dir.path())).unwrap(),
        ClickHouseConfig::default()
    );
}

#[test]
fn reads_the_section_from_autumn_toml() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        r#"
[clickhouse]
url = "https://cloud.clickhouse.com:8443"
database = "analytics"
user = "analyst"
password = "secret"
timeout_ms = 250
health_check = false
inserter_max_rows = 10
inserter_max_bytes = 1000
inserter_period_secs = 30
"#,
    );
    let config = resolve(&env_for(dir.path())).unwrap();
    assert_eq!(config.url, "https://cloud.clickhouse.com:8443");
    assert_eq!(config.database, "analytics");
    assert_eq!(config.user, "analyst");
    assert_eq!(config.password, "secret");
    assert_eq!(config.timeout(), Duration::from_millis(250));
    assert!(!config.health_check);
    assert_eq!(config.inserter_max_rows, 10);
    assert_eq!(config.inserter_max_bytes, 1000);
    assert_eq!(config.inserter_period(), Duration::from_secs(30));
}

#[test]
fn env_overrides_the_file() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        "[clickhouse]\nurl = \"http://localhost:8123\"\nuser = \"file_user\"\n",
    );
    let env = env_for(dir.path())
        .with("AUTUMN_CLICKHOUSE__USER", "env_user")
        .with("AUTUMN_CLICKHOUSE__PASSWORD", "env_secret")
        .with("AUTUMN_CLICKHOUSE__TIMEOUT_MS", "1500")
        .with("AUTUMN_CLICKHOUSE__HEALTH_CHECK", "false")
        .with("AUTUMN_CLICKHOUSE__INSERTER_PERIOD_SECS", "30");
    let config = resolve(&env).unwrap();
    assert_eq!(config.url, "http://localhost:8123");
    assert_eq!(config.user, "env_user");
    assert_eq!(config.password, "env_secret");
    assert_eq!(config.timeout(), Duration::from_millis(1500));
    assert!(!config.health_check);
    assert_eq!(config.inserter_period(), Duration::from_secs(30));
}

#[test]
fn env_prefix_follows_the_section_name() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_for(dir.path()).with("AUTUMN_ANALYTICS__URL", "https://ch.example.com:8443");
    let config = ClickHouseConfig::resolve_with_env("analytics", &env).unwrap();
    assert_eq!(config.url, "https://ch.example.com:8443");
    // The default section prefix does not leak into a custom section.
    let env = env_for(dir.path()).with("AUTUMN_CLICKHOUSE__URL", "https://ch.example.com:8443");
    let config = ClickHouseConfig::resolve_with_env("analytics", &env).unwrap();
    assert_eq!(config.url, "http://localhost:8123");
}

#[test]
fn profile_section_overrides_the_base() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        "[clickhouse]\nurl = \"http://localhost:8123\"\n\n[profile.prod.clickhouse]\nurl = \"https://prod.example.com:8443\"\n",
    );
    let env = env_for(dir.path()).with("AUTUMN_PROFILE", "prod");
    let config = resolve(&env).unwrap();
    assert_eq!(config.url, "https://prod.example.com:8443");
    assert_eq!(config.database, "default");
}

#[test]
fn profile_file_overrides_autumn_toml() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        "[clickhouse]\nurl = \"http://localhost:8123\"\n",
    );
    write(
        dir.path(),
        "autumn-prod.toml",
        "[clickhouse]\nurl = \"https://prod.example.com:8443\"\n",
    );
    let env = env_for(dir.path()).with("AUTUMN_PROFILE", "prod");
    let config = resolve(&env).unwrap();
    assert_eq!(config.url, "https://prod.example.com:8443");
}

#[test]
fn unknown_keys_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "autumn.toml", "[clickhouse]\nbogus = 1\n");
    let err = resolve(&env_for(dir.path())).unwrap_err().to_string();
    assert!(err.contains("bogus"), "unexpected error: {err}");
}

#[test]
fn validation_names_the_bad_key() {
    assert_eq!(
        invalid(|c| c.url = String::new()),
        "clickhouse.url must be an `http://` or `https://` URL"
    );
    assert_eq!(
        invalid(|c| c.url = "tcp://localhost:9000".to_owned()),
        "clickhouse.url must be an `http://` or `https://` URL"
    );
    assert_eq!(
        invalid(|c| c.url = " http://localhost:8123".to_owned()),
        "clickhouse.url must not start or end with white space"
    );
    assert_eq!(
        invalid(|c| c.database = String::new()),
        "clickhouse.database must not be empty"
    );
    assert_eq!(
        invalid(|c| c.user = String::new()),
        "clickhouse.user must not be empty"
    );
    assert_eq!(
        invalid(|c| c.timeout_ms = 0),
        "clickhouse.timeout_ms must be from 1 to 86400000 (one day)"
    );
    assert_eq!(
        invalid(|c| c.inserter_max_rows = 0),
        "clickhouse.inserter_max_rows must be from 1 to 10000000"
    );
    assert_eq!(
        invalid(|c| c.inserter_max_bytes = 0),
        "clickhouse.inserter_max_bytes must be from 1 to 1073741824 (1 GiB)"
    );
    assert_eq!(
        invalid(|c| c.inserter_period_secs = 0),
        "clickhouse.inserter_period_secs must be from 1 to 86400 (one day)"
    );
    assert_eq!(
        invalid(|c| c.inserter_period_secs = 86_401),
        "clickhouse.inserter_period_secs must be from 1 to 86400 (one day)"
    );
}

#[test]
fn bad_env_values_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_for(dir.path()).with("AUTUMN_CLICKHOUSE__TIMEOUT_MS", "nope");
    let err = resolve(&env).unwrap_err().to_string();
    assert!(
        err.contains("AUTUMN_CLICKHOUSE__TIMEOUT_MS"),
        "unexpected error: {err}"
    );
    let env = env_for(dir.path()).with("AUTUMN_CLICKHOUSE__TIMEOUT_MS", "-5");
    assert!(resolve(&env).is_err());
    let env = env_for(dir.path()).with("AUTUMN_CLICKHOUSE__HEALTH_CHECK", "maybe");
    assert!(resolve(&env).is_err());
}
