//! Hook types a built-in module can register with the host.
//!
//! The host calls these from the tool-dispatch path and from skill / hook
//! / MCP loaders. Guard and Armory land on later PRs; the types are here
//! so those modules can share the same contract.

use serde::{Deserialize, Serialize};

/// Decision returned by a pre-tool or load hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum HookResult {
    /// The module has no opinion; later gates still run.
    Skip,
    /// Allow this call or load to proceed (later gates may still deny).
    Allow,
    /// Refuse the call or load.
    Deny { reason: String },
    /// Hold for a human (reuses the host's 120s approval gate).
    Ask { reason: String, risk: String },
}

impl HookResult {
    /// True when the hook wants the action stopped.
    pub fn is_deny(&self) -> bool {
        matches!(self, HookResult::Deny { .. })
    }

    /// True when the hook wants a human decision.
    pub fn is_ask(&self) -> bool {
        matches!(self, HookResult::Ask { .. })
    }
}

/// Context for a pre-tool hook, fired before `SecurityEngine::check_tool_call`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreToolUse {
    pub tool_name: String,
    pub args: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Context for a post-tool hook, fired after a tool has been executed
/// (or denied). Used by replay to append durable events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostToolUse {
    pub tool_name: String,
    pub args: String,
    pub result: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Context for a skill, plugin-hook, or MCP load check (Armory).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLoad {
    /// `skill`, `hook`, or `mcp`.
    pub kind: String,
    pub name: String,
    pub path: String,
}

/// Optional hook callbacks a module may install. Missing callbacks are
/// treated as [`HookResult::Skip`].
#[derive(Default)]
pub struct ModuleHooks {
    /// Called before a tool is executed.
    pub pre_tool_use: Option<fn(&PreToolUse) -> HookResult>,
    /// Called after a tool is executed or denied.
    pub post_tool_use: Option<fn(&PostToolUse)>,
    /// Called before a skill, hook script, or MCP server is loaded.
    pub skill_load: Option<fn(&SkillLoad) -> HookResult>,
}

/// Contract every built-in module implements.
pub trait BuiltinModule: Send + Sync {
    /// Stable id, matching `[modules.<id>]` and `openshield <id>`.
    fn id(&self) -> &'static str;

    /// Hook callbacks. The default is no hooks.
    fn hooks(&self) -> ModuleHooks {
        ModuleHooks::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_result_serde_roundtrip() {
        let deny = HookResult::Deny {
            reason: "policy".into(),
        };
        let json = serde_json::to_string(&deny).unwrap();
        assert!(json.contains("deny"));
        let back: HookResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deny, back);
    }

    #[test]
    fn skip_is_neither_deny_nor_ask() {
        assert!(!HookResult::Skip.is_deny());
        assert!(!HookResult::Skip.is_ask());
    }
}
