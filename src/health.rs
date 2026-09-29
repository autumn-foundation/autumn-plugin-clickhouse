//! The health check: `SELECT 1`.
//!
//! # Contract
//!
//! - The check is down before the plugin starts. The plugin has no shutdown hook:
//!   the client is an HTTP pool and needs no close.
//! - The check runs `SELECT 1` and waits at most 1 second.
//! - The check is `HealthOnly`: a down ClickHouse does not block deploys.
//!   An analytics sink is not readiness-critical.
//! - The details name the database. They never show the URL or the password.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use autumn_web::actuator::{HealthCheckOutput, HealthIndicator, IndicatorGroup};

use crate::plugin::Shared;

/// The longest wait of the check, in milliseconds.
pub(crate) const CHECK_WAIT_MS: u64 = 1_000;

/// The longest wait of the check.
pub(crate) const CHECK_WAIT: Duration = Duration::from_millis(CHECK_WAIT_MS);

/// Checks the ClickHouse connection with `SELECT 1`.
pub(crate) struct ClickHouseCheck {
    shared: Arc<Shared>,
}

impl ClickHouseCheck {
    pub(crate) const fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

/// The future type of a health check.
type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

impl HealthIndicator for ClickHouseCheck {
    fn check(&self) -> BoxFuture<'_, HealthCheckOutput> {
        Box::pin(async move {
            let Some(db) = self.shared.handle.get() else {
                return HealthCheckOutput::down()
                    .with_details(details(&[("state", "not started")]));
            };
            let database = db.config().database.clone();
            let ping = async {
                match tokio::time::timeout(CHECK_WAIT, db.ping()).await {
                    Ok(Ok(())) => None,
                    Ok(Err(err)) => {
                        tracing::warn!(error = %err, "the ClickHouse health check failed");
                        Some("unreachable")
                    }
                    Err(_) => Some("timed out"),
                }
            };
            let mut fields = vec![("database", database.as_str())];
            match ping.await {
                None => HealthCheckOutput::up().with_details(details(&fields)),
                Some(state) => {
                    fields.push(("state", state));
                    HealthCheckOutput::down().with_details(details(&fields))
                }
            }
        })
    }

    fn group(&self) -> IndicatorGroup {
        IndicatorGroup::HealthOnly
    }

    fn timeout_ms(&self) -> u64 {
        CHECK_WAIT_MS
    }
}

/// Builds the details map.
fn details(fields: &[(&str, &str)]) -> HashMap<String, serde_json::Value> {
    fields
        .iter()
        .map(|(key, value)| {
            (
                (*key).to_owned(),
                serde_json::Value::String((*value).to_owned()),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests;
