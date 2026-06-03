//! OpenShark Integration Registry
//!
//! Optional bridges to other AI harnesses and tools.
//! All integrations are disabled by default and must be enabled via config.

#![allow(dead_code)]

pub mod hermes;
pub mod claw;
pub mod opencode;
pub mod claude;
pub mod registry;

use serde::{Deserialize, Serialize};

/// Master integrations config — all disabled by default.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrationsConfig {
    #[serde(default)]
    pub hermes: hermes::HermesConfig,
    #[serde(default)]
    pub claw: claw::ClawConfig,
    #[serde(default)]
    pub opencode: opencode::OpencodeConfig,
    #[serde(default)]
    pub claude: claude::ClaudeConfig,
}
