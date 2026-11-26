//! OpenShield built-in module contract.
//!
//! Companion modules (replay, eval, armory, guard) talk to the host only
//! through this crate: the `[modules]` config table, hook types, and the
//! [`BuiltinModule`] trait. They must not depend on the `openshield`
//! package — that would be a crate cycle.
//!
//! This is the Rust equivalent of OpenClaw's plugin-sdk: a thin, one-way
//! boundary from extensions into the host.

pub mod config;
pub mod hooks;
pub mod host;

pub use config::{ArmoryModuleConfig, GuardModuleConfig, ModuleToggle, ModulesConfig};
pub use hooks::{BuiltinModule, HookResult, ModuleHooks, PostToolUse, PreToolUse, SkillLoad};
pub use host::ModuleHost;

/// Identifiers of the built-in modules described in the companions plan.
pub const MODULE_IDS: &[&str] = &["replay", "eval", "armory", "guard"];
