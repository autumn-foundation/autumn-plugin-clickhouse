//! The public error type.
//!
//! # Contract
//!
//! - A ClickHouse error keeps its kind and its full message.
//! - The error text shows the kind only. [`ClickHouseError::detail`] gives the full message.
//!   A server message can hold user data.
//! - [`ErrorKind`] is the plugin's own copy of the ClickHouse error kinds.
//!   A `clickhouse` update does not change it.
//! - A timeout gives HTTP 504. A network error gives 503. A bad server answer gives 502.
//!   A missing row gives 404. Bad parameters give 400. All other errors give 500.
//! - The `Debug` output is the error text. It does not show the full message.
//! - A timeout and a network error are retryable.

use std::time::Duration;

use autumn_web::AutumnError;
use http::StatusCode;

use crate::config::ConfigError;

/// An error from the plugin.
///
/// The `Debug` output is the error text. It does not show `detail`.
#[derive(Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ClickHouseError {
    /// The configuration is not valid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// ClickHouse refused the call.
    ///
    /// The text does not show `detail`, because it can hold user data.
    #[error("ClickHouse refused the call: {kind}")]
    #[non_exhaustive]
    Database {
        /// The ClickHouse error kind.
        kind: ErrorKind,
        /// The full ClickHouse message.
        detail: String,
    },
    /// The call did not complete in time.
    #[error("the call did not complete in {timeout:?}")]
    #[non_exhaustive]
    Timeout {
        /// The timeout.
        timeout: Duration,
    },
    /// A migration failed.
    ///
    /// The text does not show `detail`, because it can hold a server message.
    #[error("the migration `{name}` failed")]
    #[non_exhaustive]
    Migration {
        /// The migration name.
        name: String,
        /// The full server message.
        detail: String,
    },
    /// The app does not have the plugin.
    #[error("the ClickHouse plugin is not installed: add `ClickHousePlugin` to the app")]
    NotInstalled,
}

/// The kind of a ClickHouse error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A bind value or a query parameter is not valid.
    InvalidParams,
    /// The HTTP layer failed. The server is down or unreachable.
    Network,
    /// The client time limit ended the operation.
    TimedOut,
    /// The server gave a bad answer: an HTTP error or a ClickHouse exception.
    BadResponse,
    /// A query found no row.
    NotFound,
    /// The row type does not match the table schema.
    SchemaMismatch,
    /// The client could not encode or decode the data.
    Decode,
    /// The build does not support the operation.
    Unsupported,
    /// The client refused the call for its own reason.
    Custom,
    /// ClickHouse gave a kind that the plugin does not know.
    Unknown,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidParams => "invalid parameters",
            Self::Network => "network error",
            Self::TimedOut => "timed out",
            Self::BadResponse => "bad response",
            Self::NotFound => "row not found",
            Self::SchemaMismatch => "schema mismatch",
            Self::Decode => "decode error",
            Self::Unsupported => "unsupported",
            Self::Custom => "client error",
            Self::Unknown => "unknown error",
        })
    }
}

impl std::fmt::Debug for ClickHouseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The derived output shows `detail`. It can hold user data.
        f.debug_tuple("ClickHouseError")
            .field(&self.to_string())
            .finish()
    }
}

impl From<clickhouse::error::Error> for ClickHouseError {
    fn from(err: clickhouse::error::Error) -> Self {
        use clickhouse::error::Error as Ch;
        let kind = match &err {
            Ch::InvalidParams(_) => ErrorKind::InvalidParams,
            Ch::Network(_) => ErrorKind::Network,
            Ch::TimedOut => ErrorKind::TimedOut,
            Ch::BadResponse(_) => ErrorKind::BadResponse,
            Ch::RowNotFound => ErrorKind::NotFound,
            Ch::SchemaMismatch(_) => ErrorKind::SchemaMismatch,
            Ch::Compression(_)
            | Ch::Decompression(_)
            | Ch::InvalidUtf8Encoding(_)
            | Ch::InvalidTagEncoding(_)
            | Ch::VariantDiscriminatorIsOutOfBound(_)
            | Ch::NotEnoughData
            | Ch::SequenceMustHaveLength
            | Ch::DeserializeAnyNotSupported
            | Ch::InvalidColumnsHeader(_) => ErrorKind::Decode,
            Ch::Unsupported(_) => ErrorKind::Unsupported,
            Ch::Custom(_) => ErrorKind::Custom,
            _ => ErrorKind::Unknown,
        };
        Self::Database {
            kind,
            detail: err.to_string(),
        }
    }
}

impl ClickHouseError {
    /// The full message of a [`ClickHouseError::Database`] or [`ClickHouseError::Migration`] error.
    ///
    /// The message can hold user data. Do not show it to users.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Database { detail, .. } | Self::Migration { detail, .. } => Some(detail),
            _ => None,
        }
    }

    /// The ClickHouse error kind of a [`ClickHouseError::Database`] error.
    #[must_use]
    pub const fn kind(&self) -> Option<ErrorKind> {
        match self {
            Self::Database { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    /// Returns `true` if a retry of the same call can succeed.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. }
                | Self::Database {
                    kind: ErrorKind::Network | ErrorKind::TimedOut,
                    ..
                }
        )
    }

    /// The HTTP status for this error.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        match self {
            Self::Timeout { .. }
            | Self::Database {
                kind: ErrorKind::TimedOut,
                ..
            } => StatusCode::GATEWAY_TIMEOUT,
            Self::Database {
                kind: ErrorKind::Network,
                ..
            } => StatusCode::SERVICE_UNAVAILABLE,
            Self::Database {
                kind: ErrorKind::BadResponse,
                ..
            } => StatusCode::BAD_GATEWAY,
            Self::Database {
                kind: ErrorKind::NotFound,
                ..
            } => StatusCode::NOT_FOUND,
            Self::Database {
                kind: ErrorKind::InvalidParams,
                ..
            } => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Converts to an [`AutumnError`] with [`status`](Self::status).
    ///
    /// `AutumnError` has no bad-gateway constructor, so a 502 status becomes a 500.
    /// The `?` operator also converts, but always gives status 500.
    /// Autumn shows server error details only in development.
    #[must_use]
    pub fn into_autumn(self) -> AutumnError {
        match self.status() {
            StatusCode::NOT_FOUND => AutumnError::not_found(self),
            StatusCode::BAD_REQUEST => AutumnError::bad_request(self),
            StatusCode::SERVICE_UNAVAILABLE => AutumnError::service_unavailable(self),
            StatusCode::GATEWAY_TIMEOUT => AutumnError::query_timeout(self.to_string()),
            _ => AutumnError::internal_server_error(self),
        }
    }
}

/// Adds [`or_http`](ClickHouseResultExt::or_http) to `Result<T, ClickHouseError>`.
pub trait ClickHouseResultExt<T> {
    /// Converts the error with [`ClickHouseError::into_autumn`].
    ///
    /// # Errors
    ///
    /// Returns the converted error.
    fn or_http(self) -> Result<T, AutumnError>;
}

impl<T> ClickHouseResultExt<T> for Result<T, ClickHouseError> {
    fn or_http(self) -> Result<T, AutumnError> {
        self.map_err(ClickHouseError::into_autumn)
    }
}

#[cfg(test)]
mod tests;
