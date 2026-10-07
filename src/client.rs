//! The [`ClickHouse`] handle: typed queries, batch inserts, and DDL.
//!
//! # Contract
//!
//! - The handle is cheap to clone. Clones share the HTTP pool and the metrics.
//! - Each call has the `timeout_ms` timeout. A timed-out call gives [`ClickHouseError::Timeout`].
//! - Rows must be owned: `#[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]`
//!   on a struct without lifetimes.
//! - The plugin re-exports the `clickhouse` crate for the `Row` derive and for escape hatches.
//!   [`ClickHouse::client`] gives the raw client.
//! - Metrics count each ended call. Logs and error text never show row data or passwords.

use std::sync::Arc;

use crate::config::ClickHouseConfig;
use crate::error::ClickHouseError;
use crate::metrics::{Metrics, Outcome};

/// The ClickHouse handle for Autumn handlers.
///
/// Build it with [`ClickHouse::new`] and put it in the app state with [`ClickHousePlugin`](crate::ClickHousePlugin).
/// Handlers take it as an extractor. Jobs use [`ClickHouse::from_state`](Self::from_state).
#[derive(Clone)]
pub struct ClickHouse {
    client: clickhouse::Client,
    config: ClickHouseConfig,
    metrics: Arc<Metrics>,
}

impl std::fmt::Debug for ClickHouse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `clickhouse::Client` is not `Debug`. The config redacts the password.
        f.debug_struct("ClickHouse")
            .field("client", &"<clickhouse::Client>")
            .field("config", &self.config)
            .field("metrics", &self.metrics)
            .finish()
    }
}

impl ClickHouse {
    /// Makes a handle from `config`.
    ///
    /// Building the handle opens no connection. The first call connects.
    #[must_use]
    pub fn new(config: &ClickHouseConfig) -> Self {
        let client = clickhouse::Client::default()
            .with_url(config.url.clone())
            .with_user(config.user.clone())
            .with_password(config.password.clone())
            .with_database(config.database.clone());
        Self::with_client(client, config.clone())
    }

    /// Makes a handle from a ready client. Tests use it with the mock server.
    pub(crate) fn with_client(client: clickhouse::Client, config: ClickHouseConfig) -> Self {
        Self {
            client,
            config,
            metrics: Arc::new(Metrics::default()),
        }
    }

    /// The configuration of the handle.
    #[must_use]
    pub const fn config(&self) -> &ClickHouseConfig {
        &self.config
    }

    /// The call counters of the handle.
    #[must_use]
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// The raw `clickhouse` client, for calls that the handle does not cover.
    #[must_use]
    pub const fn client(&self) -> &clickhouse::Client {
        &self.client
    }

    /// Runs `SELECT 1`. It checks the connection and the credentials.
    ///
    /// # Errors
    ///
    /// Returns [`ClickHouseError`] if the server is unreachable or the call times out.
    pub async fn ping(&self) -> Result<(), ClickHouseError> {
        let result = self.call(self.client.query("SELECT 1").execute()).await;
        self.metrics.query_ended(Outcome::of(&result));
        result
    }

    /// Runs a `SELECT` and returns all rows as `T`.
    ///
    /// # Errors
    ///
    /// Returns [`ClickHouseError`] if the query fails or the call times out.
    pub async fn fetch_all<T>(&self, sql: &str) -> Result<Vec<T>, ClickHouseError>
    where
        T: clickhouse::RowOwned + clickhouse::RowRead,
    {
        let result = self.call(self.client.query(sql).fetch_all::<T>()).await;
        self.metrics.query_ended(Outcome::of(&result));
        result
    }

    /// Runs a `SELECT` and returns the first row as `T`.
    ///
    /// # Errors
    ///
    /// Returns [`ClickHouseError::Database`] with [`ErrorKind::NotFound`](crate::ErrorKind::NotFound) if the query
    /// returns no row, or another [`ClickHouseError`] if the query fails or times out.
    pub async fn fetch_one<T>(&self, sql: &str) -> Result<T, ClickHouseError>
    where
        T: clickhouse::RowOwned + clickhouse::RowRead,
    {
        let result = self.call(self.client.query(sql).fetch_one::<T>()).await;
        self.metrics.query_ended(Outcome::of(&result));
        result
    }

    /// Runs a statement and discards the result. Use it for DDL.
    ///
    /// # Errors
    ///
    /// Returns [`ClickHouseError`] if the statement fails or the call times out.
    pub async fn execute(&self, sql: &str) -> Result<(), ClickHouseError> {
        let result = self.call(self.client.query(sql).execute()).await;
        self.metrics.ddl_ended(Outcome::of(&result));
        result
    }

    /// Inserts `rows` into `table` in one batch.
    ///
    /// # Errors
    ///
    /// Returns [`ClickHouseError`] if the insert fails or the call times out.
    pub async fn insert<T>(&self, table: &str, rows: &[T]) -> Result<(), ClickHouseError>
    where
        T: clickhouse::RowOwned + serde::Serialize + Sync,
    {
        let result = self.insert_inner(table, rows).await;
        let outcome = Outcome::of(&result);
        let count = if outcome == Outcome::Succeeded {
            row_count(rows.len())
        } else {
            0
        };
        self.metrics.insert_ended(outcome, count);
        result
    }

    /// Makes an [`Inserter`] for `table`. It batches rows across many calls.
    ///
    /// The limits come from the configuration: `inserter_max_rows`, `inserter_max_bytes`
    /// and `inserter_period_secs`.
    #[must_use]
    pub fn inserter<T>(&self, table: &str) -> Inserter<T>
    where
        T: clickhouse::RowOwned + serde::Serialize + Sync,
    {
        let inner = self
            .client
            .inserter::<T>(table)
            .with_max_rows(self.config.inserter_max_rows)
            .with_max_bytes(self.config.inserter_max_bytes)
            .with_period(Some(self.config.inserter_period()));
        Inserter {
            inner,
            timeout: self.config.timeout(),
            metrics: Arc::clone(&self.metrics),
        }
    }

    /// Runs `future` with the configured timeout.
    async fn call<T>(
        &self,
        future: impl std::future::Future<Output = clickhouse::error::Result<T>>,
    ) -> Result<T, ClickHouseError> {
        let timeout = self.config.timeout();
        tokio::time::timeout(timeout, future).await.map_or_else(
            |_| Err(ClickHouseError::Timeout { timeout }),
            |result| result.map_err(ClickHouseError::from),
        )
    }

    /// The insert body. [`insert`](Self::insert) records the metrics.
    async fn insert_inner<T>(&self, table: &str, rows: &[T]) -> Result<(), ClickHouseError>
    where
        T: clickhouse::RowOwned + serde::Serialize + Sync,
    {
        let mut insert = self.call(self.client.insert::<T>(table)).await?;
        for row in rows {
            self.call(insert.write(row)).await?;
        }
        self.call(insert.end()).await?;
        Ok(())
    }
}

/// An insert batcher. It holds rows and commits them when a limit hits.
///
/// Make it with [`ClickHouse::inserter`]. Call [`Inserter::commit`] to flush on your
/// schedule, and [`Inserter::end`] to flush the last rows. The limits come from the
/// plugin configuration.
pub struct Inserter<T> {
    inner: clickhouse::inserter::Inserter<T>,
    timeout: std::time::Duration,
    metrics: Arc<Metrics>,
}

impl<T> Inserter<T>
where
    T: clickhouse::RowOwned + serde::Serialize + Sync,
{
    /// Buffers `row`. It can commit a full batch first.
    ///
    /// # Errors
    ///
    /// Returns [`ClickHouseError`] if the write fails or the call times out.
    pub async fn write(&mut self, row: &T) -> Result<(), ClickHouseError> {
        let timeout = self.timeout;
        let result = tokio::time::timeout(timeout, self.inner.write(row))
            .await
            .map_or_else(
                |_| Err(ClickHouseError::Timeout { timeout }),
                |inner| inner.map_err(ClickHouseError::from),
            );
        if result.is_err() {
            self.metrics.insert_ended(Outcome::of(&result), 0);
        }
        result
    }

    /// Commits the buffered rows. It returns the batch statistics.
    ///
    /// # Errors
    ///
    /// Returns [`ClickHouseError`] if the commit fails or the call times out.
    pub async fn commit(&mut self) -> Result<InsertStats, ClickHouseError> {
        let timeout = self.timeout;
        let result = tokio::time::timeout(timeout, self.inner.force_commit())
            .await
            .map_or_else(
                |_| Err(ClickHouseError::Timeout { timeout }),
                |inner| inner.map_err(ClickHouseError::from),
            );
        let outcome = Outcome::of(&result);
        let stats = result.map(InsertStats::from)?;
        self.metrics.insert_ended(outcome, stats.rows);
        Ok(stats)
    }

    /// Commits the buffered rows and ends the insert.
    ///
    /// # Errors
    ///
    /// Returns [`ClickHouseError`] if the end fails or the call times out.
    pub async fn end(self) -> Result<InsertStats, ClickHouseError> {
        let Self {
            inner,
            timeout,
            metrics,
        } = self;
        let result = tokio::time::timeout(timeout, inner.end())
            .await
            .map_or_else(
                |_| Err(ClickHouseError::Timeout { timeout }),
                |inner| inner.map_err(ClickHouseError::from),
            );
        let outcome = Outcome::of(&result);
        let stats = result.map(InsertStats::from)?;
        metrics.insert_ended(outcome, stats.rows);
        Ok(stats)
    }
}

/// The statistics of an inserter commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertStats {
    /// The inserted rows.
    pub rows: u64,
    /// The uncompressed inserted bytes.
    pub bytes: u64,
    /// The nonempty transactions.
    pub transactions: u64,
}

impl From<clickhouse::inserter::Quantities> for InsertStats {
    fn from(quantities: clickhouse::inserter::Quantities) -> Self {
        Self {
            rows: quantities.rows,
            bytes: quantities.bytes,
            transactions: quantities.transactions,
        }
    }
}

/// Converts a row count without a truncating cast.
fn row_count(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
