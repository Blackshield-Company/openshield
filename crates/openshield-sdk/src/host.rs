//! Host-facing context passed into module CLI commands and HTTP routes.

use std::path::PathBuf;

use crate::config::ModulesConfig;

/// What a module needs from the running OpenShield process.
///
/// Kept small on purpose: a database path, the `[modules]` table, and an
/// optional config directory (embedded/Android hosts have no XDG dirs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleHost {
    pub memory_db_path: PathBuf,
    pub modules: ModulesConfig,
    pub config_dir: Option<PathBuf>,
}

impl ModuleHost {
    /// Build a host context from the pieces the OpenShield config already
    /// has.
    pub fn new(
        memory_db_path: PathBuf,
        modules: ModulesConfig,
        config_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            memory_db_path,
            modules,
            config_dir,
        }
    }

    /// Runtime switch for the replay module.
    pub fn replay_enabled(&self) -> bool {
        self.modules.replay.is_enabled()
    }

    /// Runtime switch for the eval module.
    pub fn eval_enabled(&self) -> bool {
        self.modules.eval.is_enabled()
    }

    /// Runtime switch for the armory module.
    pub fn armory_enabled(&self) -> bool {
        self.modules.armory.is_enabled()
    }

    /// Runtime switch for the guard module.
    pub fn guard_enabled(&self) -> bool {
        self.modules.guard.is_enabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_host_reports_default_enables() {
        let host = ModuleHost::new(
            PathBuf::from("/tmp/memory.db"),
            ModulesConfig::default(),
            None,
        );
        assert!(host.replay_enabled());
        assert!(host.eval_enabled());
        assert!(host.armory_enabled());
        assert!(host.guard_enabled());
    }
}
