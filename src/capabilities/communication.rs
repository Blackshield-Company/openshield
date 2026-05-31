//! Communication capabilities — cross-platform messaging.

use anyhow::Result;

use crate::tools::Tool;

// ─── Messaging Tool ─────────────────────────────────────────────────────────

pub struct MessagingTool;

impl Tool for MessagingTool {
    fn name(&self) -> &str {
        "messaging"
    }
    fn description(&self) -> &str {
        "Send messages to connected platforms. Args: --send <message> --target <platform:channel> [--media <path>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let trimmed = args.trim();
        
        // Parse --send and --target
        let mut message = None;
        let mut target = None;
        let mut media = None;

        let parts: Vec<&str> = trimmed.split("--").collect();
        for part in parts.iter().skip(1) {
            let p = part.trim();
            if p.starts_with("send ") {
                message = Some(p.strip_prefix("send ").unwrap_or("").trim());
            } else if p.starts_with("target ") {
                target = Some(p.strip_prefix("target ").unwrap_or("").trim());
            } else if p.starts_with("media ") {
                media = Some(p.strip_prefix("media ").unwrap_or("").trim());
            }
        }

        let message = message.unwrap_or("");
        let target = target.unwrap_or("");

        if message.is_empty() || target.is_empty() {
            return Ok("Usage: messaging --send <message> --target <platform:channel> [--media <path>]".to_string());
        }

        let mut result = format!(
            "Message queued for delivery:\n  Target: {}\n  Content: {}",
            target,
            message.chars().take(200).collect::<String>()
        );

        if let Some(media_path) = media {
            result.push_str(&format!("\n  Media: {}", media_path));
        }

        result.push_str("\n\nNote: Actual delivery requires gateway configuration. Supported platforms: discord, telegram, slack, matrix.");
        Ok(result)
    }
}
