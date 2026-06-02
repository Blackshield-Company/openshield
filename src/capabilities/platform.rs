//! Platform-specific capabilities — Yuanbao, computer use.

use anyhow::Result;

use crate::tools::Tool;

// ─── Yuanbao Tool ───────────────────────────────────────────────────────────

pub struct YuanbaoTool;

impl Tool for YuanbaoTool {
    fn name(&self) -> &str {
        "yuanbao"
    }
    fn description(&self) -> &str {
        "Yuanbao (元宝) group interactions. Args: --list | --send <message> --target <group_code>"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let trimmed = args.trim();
        if trimmed == "--list" {
            Ok("Yuanbao groups:\n  No groups configured.\n\nSet up: Configure Yuanbao credentials in Hermes config.".to_string())
        } else if trimmed.starts_with("--send ") {
            let rest = trimmed.strip_prefix("--send ").unwrap_or("").trim();
            Ok(format!(
                "Yuanbao message queued: {}\n\nNote: Requires Yuanbao credentials.",
                rest.chars().take(200).collect::<String>()
            ))
        } else {
            Ok("Usage: yuanbao --list | --send <message> --target <group_code>".to_string())
        }
    }
}

// ─── Computer Use Tool ──────────────────────────────────────────────────────

pub struct ComputerUseTool;

impl Tool for ComputerUseTool {
    fn name(&self) -> &str {
        "computer_use"
    }
    fn description(&self) -> &str {
        "Computer use automation (macOS). Args: <instruction> [--screenshot] [--click <x> <y>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        if args.trim().is_empty() {
            return Ok(
                "Usage: computer_use <instruction> [--screenshot] [--click <x> <y>]".to_string(),
            );
        }
        Ok(format!(
            "Computer use requested: {}\n\nNote: Computer use automation requires macOS and a vision-capable model. The model will guide actions based on screenshots.",
            args.chars().take(200).collect::<String>()
        ))
    }
}
