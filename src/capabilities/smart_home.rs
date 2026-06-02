//! Smart home capabilities — Home Assistant, Spotify.

use anyhow::Result;

use crate::tools::Tool;

// ─── Home Assistant Tool ────────────────────────────────────────────────────

pub struct HomeAssistantTool;

impl Tool for HomeAssistantTool {
    fn name(&self) -> &str {
        "homeassistant"
    }
    fn description(&self) -> &str {
        "Control smart home devices. Args: --list | --toggle <device_id> | --set <device_id> <state>"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let trimmed = args.trim();
        if trimmed == "--list" || trimmed.is_empty() {
            Ok("Home Assistant devices:\n  No devices configured.\n\nSet up: export HASS_URL=http://homeassistant.local:8123 && export HASS_TOKEN=your_token".to_string())
        } else if trimmed.starts_with("--toggle ") {
            let device = trimmed.strip_prefix("--toggle ").unwrap_or("").trim();
            Ok(format!(
                "Toggling device: {}\n\nNote: Requires HASS_URL and HASS_TOKEN environment variables.",
                device
            ))
        } else if trimmed.starts_with("--set ") {
            let rest = trimmed.strip_prefix("--set ").unwrap_or("").trim();
            Ok(format!(
                "Setting device state: {}\n\nNote: Requires HASS_URL and HASS_TOKEN environment variables.",
                rest
            ))
        } else {
            Ok(
                "Usage: homeassistant --list | --toggle <device_id> | --set <device_id> <state>"
                    .to_string(),
            )
        }
    }
}

// ─── Spotify Tool ───────────────────────────────────────────────────────────

pub struct SpotifyTool;

impl Tool for SpotifyTool {
    fn name(&self) -> &str {
        "spotify"
    }
    fn description(&self) -> &str {
        "Spotify playback control. Args: --play <query> | --pause | --resume | --queue <track> | --now-playing"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let trimmed = args.trim();
        if trimmed.starts_with("--play ") {
            let query = trimmed.strip_prefix("--play ").unwrap_or("").trim();
            Ok(format!(
                "Spotify play request: {}\n\nNote: Requires Spotify credentials. Set SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET.",
                query
            ))
        } else if trimmed == "--pause" {
            Ok("Spotify pause requested.\n\nNote: Requires Spotify credentials.".to_string())
        } else if trimmed == "--resume" {
            Ok("Spotify resume requested.\n\nNote: Requires Spotify credentials.".to_string())
        } else if trimmed.starts_with("--queue ") {
            let track = trimmed.strip_prefix("--queue ").unwrap_or("").trim();
            Ok(format!(
                "Spotify queue request: {}\n\nNote: Requires Spotify credentials.",
                track
            ))
        } else if trimmed == "--now-playing" {
            Ok("Spotify now-playing requested.\n\nNote: Requires Spotify credentials.".to_string())
        } else {
            Ok("Usage: spotify --play <query> | --pause | --resume | --queue <track> | --now-playing".to_string())
        }
    }
}
