//! The `[clickhouse]` section of `autumn.toml`.
//!
//! # Contract
//!
//! Each layer overrides the layers before it:
//!
//! 1. The defaults.
//! 2. `[clickhouse]` in `autumn.toml`.
//! 3. `[profile.<name>.clickhouse]` in `autumn.toml`.
//! 4. `[clickhouse]` in `autumn-<name>.toml`.
//! 5. `AUTUMN_CLICKHOUSE__<KEY>` variables. `AUTUMN_CLICKHOUSE__TIMEOUT_MS` sets `timeout_ms`.
//!
//! The result must pass [`ClickHouseConfig::validate`]. Unknown keys are errors.
//!
//! ```toml
//! [clickhouse]
//! url = "http://localhost:8123"
//! database = "analytics"
//! user = "default"
//! password = "secret"
//! timeout_ms = 5000
//! health_check = true
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use autumn_web::config::Env;
use serde::{Deserialize, Serialize};

/// The default section name.
pub const DEFAULT_SECTION: &str = "clickhouse";

/// A configuration that is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub(crate) String);

/// The plugin settings.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[non_exhaustive]
pub struct ClickHouseConfig {
    /// The ClickHouse HTTP(S) endpoint, for example `http://localhost:8123`.
    pub url: String,
    /// The database that queries run against.
    pub database: String,
    /// The ClickHouse user.
    pub user: String,
    /// The ClickHouse password. The `Debug` output never shows it.
    pub password: String,
    /// The call timeout in milliseconds. Every query, insert, and DDL call gets this long.
    pub timeout_ms: u64,
    /// If `true`, the plugin adds a `SELECT 1` health check.
    pub health_check: bool,
    /// The most rows that one inserter batch holds before it flushes.
    pub inserter_max_rows: u64,
    /// The most bytes that one inserter batch holds before it flushes.
    pub inserter_max_bytes: u64,
    /// The longest time in seconds that an inserter batch waits before it flushes.
    pub inserter_period_secs: u64,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8123".to_owned(),
            database: "default".to_owned(),
            user: "default".to_owned(),
            password: String::new(),
            timeout_ms: 5_000,
            health_check: true,
            inserter_max_rows: 100_000,
            inserter_max_bytes: 16 * 1024 * 1024,
            inserter_period_secs: 5,
        }
    }
}

impl std::fmt::Debug for ClickHouseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickHouseConfig")
            .field("url", &self.url)
            .field("database", &self.database)
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .field("timeout_ms", &self.timeout_ms)
            .field("health_check", &self.health_check)
            .field("inserter_max_rows", &self.inserter_max_rows)
            .field("inserter_max_bytes", &self.inserter_max_bytes)
            .field("inserter_period_secs", &self.inserter_period_secs)
            .finish()
    }
}

/// The type of a configuration leaf, for environment values.
#[derive(Clone, Copy)]
enum Kind {
    Text,
    Unsigned,
    Bool,
}

/// Each leaf key and its type.
const LEAVES: &[(&str, Kind)] = &[
    ("url", Kind::Text),
    ("database", Kind::Text),
    ("user", Kind::Text),
    ("password", Kind::Text),
    ("timeout_ms", Kind::Unsigned),
    ("health_check", Kind::Bool),
    ("inserter_max_rows", Kind::Unsigned),
    ("inserter_max_bytes", Kind::Unsigned),
    ("inserter_period_secs", Kind::Unsigned),
];

/// The largest call timeout: one day.
const MAX_TIMEOUT_MS: u64 = 86_400_000;
/// The largest inserter batch.
const MAX_BATCH_ROWS: u64 = 10_000_000;
/// The largest inserter batch in bytes: 1 GiB.
const MAX_BATCH_BYTES: u64 = 1 << 30;
/// The longest inserter period: one day.
const MAX_PERIOD_SECS: u64 = 86_400;

impl ClickHouseConfig {
    /// Reads `[section]` from the app files and the environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if a file is not valid TOML or a value is not valid.
    pub fn resolve(section: &str) -> Result<Self, ConfigError> {
        autumn_web::dotenv::os_env_with_dotenv().map_or_else(
            |_| Self::resolve_with_env(section, &autumn_web::config::OsEnv),
            |env| Self::resolve_with_env(section, &env),
        )
    }

    /// Reads `[section]` with `env` as the environment.
    ///
    /// # Errors
    ///
    /// See [`resolve`](Self::resolve).
    pub fn resolve_with_env(section: &str, env: &dyn Env) -> Result<Self, ConfigError> {
        let (selected, profile) = active_profile(env);
        let mut merged = toml::Table::new();
        if let Some(base) = read_toml(&config_file("autumn.toml", env))? {
            merge_section(&mut merged, base.get(section), section)?;
            for name in inline_profile_names(&profile) {
                let inline = base
                    .get("profile")
                    .and_then(|p| p.get(name))
                    .and_then(|p| p.get(section));
                merge_section(&mut merged, inline, section)?;
            }
        }
        for name in autumn_web::config::profile_override_file_lookup_names(&profile, &selected) {
            if let Some(file) = read_toml(&config_file(&format!("autumn-{name}.toml"), env))? {
                merge_section(&mut merged, file.get(section), section)?;
                break;
            }
        }
        apply_env(&mut merged, section, env)?;
        let config: Self = toml::Value::Table(merged)
            .try_into()
            .map_err(|err| ConfigError(format!("[{section}]: {err}")))?;
        config.validate_section(section)?;
        Ok(config)
    }

    /// Checks each value.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] that names the first key that is not valid.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_section(DEFAULT_SECTION)
    }

    /// Checks each value. The errors name keys in `section`.
    pub(crate) fn validate_section(&self, section: &str) -> Result<(), ConfigError> {
        let fail = |key: &str, rule: &str| Err(ConfigError(format!("{section}.{key} {rule}")));
        if self.url.trim().is_empty() {
            return fail("url", "must be an `http://` or `https://` URL");
        }
        if self.url.trim() != self.url {
            return fail("url", "must not start or end with white space");
        }
        if !self.url_is_http() {
            return fail("url", "must be an `http://` or `https://` URL");
        }
        if self.database.trim().is_empty() {
            return fail("database", "must not be empty");
        }
        if self.database.trim() != self.database {
            return fail("database", "must not start or end with white space");
        }
        if self.user.trim().is_empty() {
            return fail("user", "must not be empty");
        }
        if !(1..=MAX_TIMEOUT_MS).contains(&self.timeout_ms) {
            return fail("timeout_ms", "must be from 1 to 86400000 (one day)");
        }
        if !(1..=MAX_BATCH_ROWS).contains(&self.inserter_max_rows) {
            return fail("inserter_max_rows", "must be from 1 to 10000000");
        }
        if !(1..=MAX_BATCH_BYTES).contains(&self.inserter_max_bytes) {
            return fail("inserter_max_bytes", "must be from 1 to 1073741824 (1 GiB)");
        }
        if !(1..=MAX_PERIOD_SECS).contains(&self.inserter_period_secs) {
            return fail("inserter_period_secs", "must be from 1 to 86400 (one day)");
        }
        Ok(())
    }

    /// Returns `true` for an `http://` or `https://` URL.
    #[must_use]
    pub fn url_is_http(&self) -> bool {
        let lower = self.url.trim().to_ascii_lowercase();
        lower.starts_with("http://") || lower.starts_with("https://")
    }

    /// The call timeout.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    /// The longest time an inserter batch waits before it flushes.
    #[must_use]
    pub const fn inserter_period(&self) -> Duration {
        Duration::from_secs(self.inserter_period_secs)
    }
}

/// Gives the selected profile text and the normalized profile, as Autumn does.
fn active_profile(env: &dyn Env) -> (String, String) {
    let selected = ["AUTUMN_ENV", "AUTUMN_PROFILE"]
        .iter()
        .filter_map(|key| env.var(key).ok())
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let release = env.var("AUTUMN_IS_DEBUG").is_ok_and(|v| v == "0");
            if release { "prod" } else { "dev" }.to_owned()
        });
    let profile =
        autumn_web::config::normalize_profile_name(&selected).unwrap_or_else(|| "dev".to_owned());
    (selected, profile)
}

/// The inline profile names to read, in order. The canonical name is last.
fn inline_profile_names(profile: &str) -> Vec<&str> {
    match profile {
        "prod" => vec!["production", "prod"],
        "dev" => vec!["development", "dev"],
        other => vec![other],
    }
}

/// Finds a config file in `AUTUMN_MANIFEST_DIR`, or else in the working directory.
fn config_file(name: &str, env: &dyn Env) -> PathBuf {
    env.var("AUTUMN_MANIFEST_DIR")
        .ok()
        .map(|dir| Path::new(&dir).join(name))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn read_toml(path: &Path) -> Result<Option<toml::Table>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map(Some)
            .map_err(|err| ConfigError(format!("{}: {err}", path.display()))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(ConfigError(format!("{}: {err}", path.display()))),
    }
}

fn merge_section(
    into: &mut toml::Table,
    layer: Option<&toml::Value>,
    section: &str,
) -> Result<(), ConfigError> {
    match layer {
        None => Ok(()),
        Some(toml::Value::Table(table)) => {
            deep_merge(into, table);
            Ok(())
        }
        Some(_) => Err(ConfigError(format!("[{section}] must be a table"))),
    }
}

fn deep_merge(into: &mut toml::Table, layer: &toml::Table) {
    for (key, value) in layer {
        match (into.get_mut(key), value) {
            (Some(toml::Value::Table(old)), toml::Value::Table(new)) => deep_merge(old, new),
            _ => {
                into.insert(key.clone(), value.clone());
            }
        }
    }
}

fn apply_env(into: &mut toml::Table, section: &str, env: &dyn Env) -> Result<(), ConfigError> {
    let name: String = section
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    for (key, kind) in LEAVES {
        let name = format!("AUTUMN_{name}__{}", key.to_ascii_uppercase());
        let Ok(raw) = env.var(&name) else {
            continue;
        };
        let bad = || ConfigError(format!("{name}: can not read {raw:?}"));
        let value = match kind {
            Kind::Text => toml::Value::String(raw.clone()),
            Kind::Unsigned => {
                let value: i64 = raw.trim().parse().map_err(|_| bad())?;
                if value < 0 {
                    return Err(bad());
                }
                toml::Value::Integer(value)
            }
            Kind::Bool => match raw.trim() {
                "true" | "1" => toml::Value::Boolean(true),
                "false" | "0" => toml::Value::Boolean(false),
                _ => return Err(bad()),
            },
        };
        into.insert((*key).to_owned(), value);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
