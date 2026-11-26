//! Built-in replay module.
//!
//! This crate currently exposes a status CLI (and, with `web-api`, a
//! status route). Listing, opening, and redacted export land in a
//! follow-up PR once approvals are persisted in the memory database.

use clap::Subcommand;
use openshield_sdk::{BuiltinModule, ModuleHooks, ModuleHost};
use serde::Serialize;

/// Clap subcommands for `openshield replay`.
#[derive(Debug, Clone, Subcommand)]
pub enum ReplayCli {
    /// Show whether the replay module is compiled and enabled.
    Status,
}

/// JSON/text report for `openshield replay status` and `GET /api/v1/replay/status`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReplayStatus {
    pub id: &'static str,
    pub compiled: bool,
    pub enabled: bool,
}

impl ReplayStatus {
    /// Status for this compiled crate, given the runtime switch.
    pub fn current(enabled: bool) -> Self {
        Self {
            id: "replay",
            compiled: true,
            enabled,
        }
    }
}

/// The replay built-in module.
pub struct ReplayModule;

impl BuiltinModule for ReplayModule {
    fn id(&self) -> &'static str {
        "replay"
    }

    fn hooks(&self) -> ModuleHooks {
        ModuleHooks::default()
    }
}

/// Run a `openshield replay` subcommand.
///
/// `status` always succeeds. Other commands (added later) should return
/// [`ExitCode::USAGE`] (2) when the module is disabled.
pub fn run_cli(cmd: ReplayCli, host: &ModuleHost) -> anyhow::Result<ReplayStatus> {
    match cmd {
        ReplayCli::Status => Ok(ReplayStatus::current(host.replay_enabled())),
    }
}

/// Print a status report as plain text.
pub fn format_status(status: &ReplayStatus) -> String {
    format!(
        "module: {}\ncompiled: {}\nenabled: {}",
        status.id, status.compiled, status.enabled
    )
}

/// Exit code used when a module is compiled but disabled at runtime.
pub const DISABLED_EXIT: u8 = 2;

#[cfg(feature = "web-api")]
mod routes {
    use axum::Json;
    use axum::Router;
    use axum::extract::State;
    use axum::routing::get;
    use openshield_sdk::ModuleHost;

    use super::ReplayStatus;

    /// Routes mounted at `/api/v1/replay`.
    ///
    /// Generic over the host app state so this sub-router can be nested
    /// into `Router<AppState>` via [`axum::extract::FromRef`].
    pub fn router<S>() -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
        ModuleHost: axum::extract::FromRef<S>,
    {
        Router::new().route("/status", get(status))
    }

    async fn status(State(host): State<ModuleHost>) -> Json<ReplayStatus> {
        Json(ReplayStatus::current(host.replay_enabled()))
    }
}

#[cfg(feature = "web-api")]
pub use routes::router;

#[cfg(test)]
mod tests {
    use super::*;
    use openshield_sdk::{ModuleToggle, ModulesConfig};
    use std::path::PathBuf;

    fn host(enabled: bool) -> ModuleHost {
        ModuleHost::new(
            PathBuf::from("/tmp/memory.db"),
            ModulesConfig {
                replay: ModuleToggle { enabled },
                ..Default::default()
            },
            None,
        )
    }

    #[test]
    fn status_reports_runtime_switch() {
        let on = run_cli(ReplayCli::Status, &host(true)).unwrap();
        assert!(on.compiled);
        assert!(on.enabled);
        let off = run_cli(ReplayCli::Status, &host(false)).unwrap();
        assert!(!off.enabled);
        assert!(format_status(&off).contains("enabled: false"));
    }

    #[test]
    fn module_id_is_replay() {
        assert_eq!(ReplayModule.id(), "replay");
    }
}
