//! Host helpers for built-in companion modules.

use crate::config::Config;
use openshield_sdk::ModuleHost;

/// Build a [`ModuleHost`] from the loaded OpenShield config.
pub fn host_from_config(config: &Config) -> ModuleHost {
    host_from_parts(config, None)
}

/// Like [`host_from_config`], with an explicit config directory for
/// embedded hosts that have no XDG paths.
pub fn host_from_parts(config: &Config, config_dir: Option<std::path::PathBuf>) -> ModuleHost {
    ModuleHost::new(
        config.memory_db_path.clone(),
        config.modules.clone(),
        config_dir,
    )
}

/// Module ids that have a compiled crate in this build.
pub fn compiled_modules() -> Vec<&'static str> {
    vec![
        #[cfg(feature = "replay")]
        "replay",
    ]
}

/// Human-readable module status line for `openshield doctor`.
pub fn doctor_message(config: &Config) -> String {
    let compiled = compiled_modules();
    let mut parts = Vec::new();
    for id in openshield_sdk::MODULE_IDS {
        let compiled_here = compiled.contains(id);
        let enabled = match *id {
            "replay" => config.modules.replay.is_enabled(),
            "eval" => config.modules.eval.is_enabled(),
            "armory" => config.modules.armory.is_enabled(),
            "guard" => config.modules.guard.is_enabled(),
            _ => false,
        };
        if compiled_here {
            parts.push(format!(
                "{}: compiled, {}",
                id,
                if enabled { "enabled" } else { "disabled" }
            ));
        } else {
            parts.push(format!("{}: not compiled (reserved)", id));
        }
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_list_includes_replay_on_default_features() {
        let compiled = compiled_modules();
        assert!(
            compiled.contains(&"replay"),
            "default features should compile replay: {compiled:?}"
        );
        assert!(!compiled.contains(&"eval"), "eval crate has not landed yet");
    }

    #[test]
    fn doctor_message_mentions_all_ids() {
        let msg = doctor_message(&Config::default());
        for id in openshield_sdk::MODULE_IDS {
            assert!(msg.contains(id), "{msg} should mention {id}");
        }
        assert!(msg.contains("replay: compiled, enabled"));
    }
}
