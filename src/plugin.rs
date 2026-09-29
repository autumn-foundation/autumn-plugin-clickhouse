//! [`ClickHousePlugin`]: installs a [`ClickHouse`](crate::ClickHouse) handle in an Autumn app.
//!
//! # Contract
//!
//! - `build` reads the configuration. A bad configuration stops the boot in the startup hook.
//! - The startup hook builds the handle, pings ClickHouse with `SELECT 1`, and puts the handle
//!   in the app state. An unreachable ClickHouse stops the boot.
//! - With `health_check = true`, `build` adds a `HealthOnly` check that runs `SELECT 1`.
//! - `build` adds the call counters as a metrics source.
//! - Handlers take the [`ClickHouse`](crate::ClickHouse) extractor. It fails with a 500 error
//!   if the plugin is not installed.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use autumn_web::actuator::{MetricFamily, MetricsSource};
use autumn_web::app::AppBuilder;
use autumn_web::plugin::Plugin;
use autumn_web::{AppState, AutumnError};
use axum::extract::FromRef;

use crate::client::ClickHouse;
use crate::config::{ClickHouseConfig, ConfigError, DEFAULT_SECTION};
use crate::error::ClickHouseError;
use crate::health::ClickHouseCheck;

/// The plugin name in Autumn diagnostics.
pub const PLUGIN_NAME: &str = "autumn-plugin-clickhouse";

/// State that the plugin hooks share.
#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) handle: OnceLock<ClickHouse>,
}

/// The metrics source: the call counters of the handle.
struct Source(Arc<Shared>);

impl MetricsSource for Source {
    fn collect(&self) -> Vec<MetricFamily> {
        self.0
            .handle
            .get()
            .map_or_else(Vec::new, |db| db.metrics().families())
    }
}

enum ConfigSource {
    Section(String),
    Explicit(Box<ClickHouseConfig>),
}

type Change = Box<dyn FnOnce(&mut ClickHouseConfig) + Send>;

/// Installs a [`ClickHouse`](crate::ClickHouse) handle in an Autumn app.
///
/// ```rust,no_run
/// use autumn_plugin_clickhouse::ClickHousePlugin;
///
/// # async fn run() {
/// autumn_web::app().plugin(ClickHousePlugin::new()).run().await;
/// # }
/// ```
#[must_use]
pub struct ClickHousePlugin {
    source: ConfigSource,
    changes: Vec<Change>,
}

impl Default for ClickHousePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ClickHousePlugin {
    /// Makes a plugin that reads `[clickhouse]`.
    pub fn new() -> Self {
        Self {
            source: ConfigSource::Section(DEFAULT_SECTION.to_owned()),
            changes: Vec::new(),
        }
    }

    /// Reads `[section]` instead of `[clickhouse]`.
    ///
    /// An app can have one ClickHouse plugin only. Autumn ignores a second plugin with the same name.
    pub fn config_section(mut self, section: impl Into<String>) -> Self {
        self.source = ConfigSource::Section(section.into());
        self
    }

    /// Uses `config` and reads no files or variables.
    pub fn config(mut self, config: ClickHouseConfig) -> Self {
        self.source = ConfigSource::Explicit(Box::new(config));
        self
    }

    /// Changes the configuration after the plugin reads it.
    pub fn configure(
        mut self,
        change: impl FnOnce(&mut ClickHouseConfig) + Send + 'static,
    ) -> Self {
        self.changes.push(Box::new(change));
        self
    }

    fn resolve(
        source: &ConfigSource,
        changes: Vec<Change>,
    ) -> Result<ClickHouseConfig, ConfigError> {
        let mut config = match source {
            ConfigSource::Section(section) => ClickHouseConfig::resolve(section)?,
            ConfigSource::Explicit(config) => (**config).clone(),
        };
        for change in changes {
            change(&mut config);
        }
        match source {
            ConfigSource::Section(section) => config.validate_section(section)?,
            ConfigSource::Explicit(_) => config.validate()?,
        }
        Ok(config)
    }
}

impl Plugin for ClickHousePlugin {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed(PLUGIN_NAME)
    }

    fn build(self, app: AppBuilder) -> AppBuilder {
        let Self { source, changes } = self;
        let mut app = app;
        if let ConfigSource::Section(section) = &source {
            app = app.config_section(section.clone());
        }
        let resolved = Self::resolve(&source, changes);
        let shared = Arc::new(Shared::default());
        app = app.metrics_source("clickhouse", Arc::new(Source(Arc::clone(&shared))));
        if resolved.as_ref().is_ok_and(|config| config.health_check) {
            app = app.health_indicator(
                "clickhouse",
                Arc::new(ClickHouseCheck::new(Arc::clone(&shared))),
            );
        }
        let on_start = Arc::clone(&shared);
        let resolved = Arc::new(resolved);
        app.on_startup(move |state| {
            let shared = Arc::clone(&on_start);
            let resolved = Arc::clone(&resolved);
            async move {
                let config = resolved.as_ref().clone().map_err(|err| boot_error(&err))?;
                let db = ClickHouse::new(&config);
                db.ping().await.map_err(|err| boot_error(&err))?;
                state.insert_extension(db.clone());
                let _ = shared.handle.set(db);
                tracing::info!("the ClickHouse plugin is ready");
                Ok(())
            }
        })
    }
}

/// A boot error. The text has no server message, because it can hold user data.
fn boot_error(err: &dyn std::fmt::Display) -> AutumnError {
    AutumnError::internal_server_error_msg(format!("{PLUGIN_NAME}: {err}"))
}

impl std::fmt::Debug for ClickHousePlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let source = match &self.source {
            ConfigSource::Section(section) => section.as_str(),
            ConfigSource::Explicit(_) => "(explicit)",
        };
        f.debug_struct("ClickHousePlugin")
            .field("config", &source)
            .field("changes", &self.changes.len())
            .finish()
    }
}

impl ClickHouse {
    /// Gets the handle from the app state, for example in a job or a task.
    #[must_use]
    pub fn from_state(state: &AppState) -> Option<Self> {
        state.extension::<Self>().map(|db| (*db).clone())
    }
}

impl<S> axum::extract::FromRequestParts<S> for ClickHouse
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AutumnError;

    // The `FromRequestParts` trait requires an async signature, but extraction
    // is synchronous (state lookup only); there is nothing to await.
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(
        _parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        Self::from_state(&app_state).ok_or_else(|| ClickHouseError::NotInstalled.into_autumn())
    }
}

#[cfg(test)]
mod tests;
