//! Ordered DDL migrations: `CREATE TABLE IF NOT EXISTS` statements.
//!
//! # Contract
//!
//! - [`migrate`] runs the migrations in order and stops at the first failure.
//! - A failed migration gives [`ClickHouseError::Migration`] with the migration name.
//!   The error text never shows the server message.
//! - [`Migration::create_table`] builds a `MergeTree` table. The table name must be an
//!   identifier: letters, digits, `_` and `.`, starting with a letter or `_`.

use crate::client::ClickHouse;
use crate::config::ConfigError;
use crate::error::ClickHouseError;

/// One DDL statement with a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    /// The migration name. Errors name it.
    pub name: String,
    /// The DDL statement, for example a `CREATE TABLE IF NOT EXISTS`.
    pub ddl: String,
}

impl Migration {
    /// Makes a migration from a name and a DDL statement.
    pub fn new(name: impl Into<String>, ddl: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ddl: ddl.into(),
        }
    }

    /// Makes a `CREATE TABLE IF NOT EXISTS` migration for a `MergeTree` table.
    ///
    /// `columns` holds the column definitions, for example
    /// `"ts DateTime, name String, count UInt64"`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if `name` is not a valid table identifier.
    pub fn create_table(name: &str, columns: &str) -> Result<Self, ConfigError> {
        if !is_table_name(name) {
            return Err(ConfigError(format!(
                "migration `{name}`: the table name is not an identifier"
            )));
        }
        Ok(Self {
            name: format!("create table {name}"),
            ddl: format!(
                "CREATE TABLE IF NOT EXISTS {name} ({columns}) ENGINE = MergeTree() ORDER BY tuple()"
            ),
        })
    }
}

/// Returns `true` if `name` is a safe table identifier.
fn is_table_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// Runs the migrations in order. It stops at the first failure.
///
/// # Errors
///
/// Returns [`ClickHouseError::Migration`] with the failed migration name.
pub async fn migrate(db: &ClickHouse, migrations: &[Migration]) -> Result<(), ClickHouseError> {
    for migration in migrations {
        db.execute(&migration.ddl).await.map_err(|err| {
            let detail = err.detail().map_or_else(|| err.to_string(), str::to_owned);
            ClickHouseError::Migration {
                name: migration.name.clone(),
                detail,
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
