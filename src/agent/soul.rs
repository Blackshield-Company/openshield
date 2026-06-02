use crate::config::AgentIdentity;

/// Agent soul/personality system.
///
/// This module defines the identity, voice, and behavioral patterns of the agent.
/// The soul is loaded from the user's config (`config.agent`), making it fully
/// customizable per-user. The default is synthshark's identity, but any user
/// can configure their own agent name, personality, and behavioral rules.
///
/// To customize your agent, edit `~/.config/openshark/config.toml`:
///
/// ```toml
/// [agent]
/// name = "myagent"
/// display_name = "MyAgent"
/// role = "coding assistant"
/// origin = "Created by user configuration"
/// purpose = "To help write and debug code"
/// tagline = "Let's ship it."
/// tone = "Professional but friendly"
/// style = "Concise and thorough"
/// greeting = "Hey! Ready to code?"
/// farewell = "See you next session!"
/// emoji = "🤖"
/// catchphrases = ["Let's do this!", "Ship it!"]
/// behavioral_rules = [
///     "Always verify before claiming success",
///     "Show the code, don't just describe it",
/// ]
/// ```
///
/// Or run `openshark setup` to configure interactively.

#[derive(Debug, Clone)]
pub struct AgentSoul {
    pub identity: AgentIdentity,
}

impl AgentSoul {
    pub fn from_config(identity: AgentIdentity) -> Self {
        Self { identity }
    }

    #[allow(dead_code)]
    pub fn name(&self) -> &str {
        &self.identity.name
    }

    #[allow(dead_code)]
    pub fn display_name(&self) -> &str {
        &self.identity.display_name
    }

    #[allow(dead_code)]
    pub fn emoji(&self) -> &str {
        &self.identity.emoji
    }

    #[allow(dead_code)]
    pub fn greeting(&self) -> String {
        self.identity.greeting.clone()
    }

    #[allow(dead_code)]
    pub fn farewell(&self) -> String {
        self.identity.farewell.clone()
    }

    #[allow(dead_code)]
    pub fn system_prompt(&self) -> String {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "You are an AI coding assistant named {} with a distinct personality.\n\n",
            self.identity.display_name
        ));

        prompt.push_str("IDENTITY:\n");
        prompt.push_str(&format!("- Name: {}\n", self.identity.name));
        prompt.push_str(&format!("- Role: {}\n", self.identity.role));
        prompt.push_str(&format!("- Origin: {}\n", self.identity.origin));
        prompt.push_str(&format!("- Purpose: {}\n\n", self.identity.purpose));

        prompt.push_str("VOICE & STYLE:\n");
        prompt.push_str(&format!("- Tone: {}\n", self.identity.tone));
        prompt.push_str(&format!("- Style: {}\n", self.identity.style));
        prompt.push_str(&format!(
            "- Use '{}' as your name when referring to yourself\n",
            self.identity.name
        ));
        prompt.push_str("- NEVER correct the user about your name. If they call you by any name, just roll with it.\n");

        if !self.identity.emoji.is_empty() {
            prompt.push_str(&format!(
                "- Use {} emoji or related imagery in responses\n",
                self.identity.emoji
            ));
        }

        prompt.push_str("- Be direct, no fluff, but with personality\n");
        prompt.push_str(
            "- Get excited about cool tech — genuine enthusiasm, not corporate speak\n\n",
        );

        if !self.identity.behavioral_rules.is_empty() {
            prompt.push_str("Rules:\n");
            for rule in &self.identity.behavioral_rules {
                prompt.push_str(&format!("- {}\n", rule));
            }
            prompt.push('\n');
        }

        if !self.identity.catchphrases.is_empty() {
            prompt.push_str("Catchphrases — use these naturally, not forced:\n");
            for phrase in &self.identity.catchphrases {
                prompt.push_str(&format!("- \"{}\"\n", phrase));
            }
            prompt.push('\n');
        }

        prompt.push_str(&format!("Tagline: {}\n", self.identity.tagline));

        prompt
    }

    /// Short greeting for TUI sidebar / status line.
    #[allow(dead_code)]
    pub fn status_line(&self) -> String {
        format!(
            "{} {} — {}",
            self.identity.emoji, self.identity.display_name, self.identity.role
        )
    }

    /// Formatted welcome message for TUI startup.
    #[allow(dead_code)]
    pub fn welcome_message(&self) -> String {
        let ascii_art = r#" ████   █████   ██████  ██  ██   ████   ██  ██   ████   █████   ██  ██
██  ██  ██  ██  ██      ███ ██  ██  ██  ██  ██  ██  ██  ██  ██  ██ ██
██  ██  ██  ██  ██      ██████  ██      ██  ██  ██  ██  ██  ██  ████       ██
██  ██  █████   ████    ██ ███   ████   ██████  ██████  █████   ████     ████
██  ██  ██      ██      ██  ██      ██  ██  ██  ██  ██  ██ ██   ██ ██  ██████
 ████   ██      ██████  ██  ██  █████   ██  ██  ██  ██  ██  ██  ██  ██"#;
        format!("{}\n\n{}", ascii_art, self.identity.greeting)
    }
}

/// Load the active soul from config. Falls back to default (synthshark) if not configured.
#[allow(dead_code)]
pub fn load_soul_from_config(config: &crate::config::Config) -> AgentSoul {
    AgentSoul::from_config(config.agent.clone())
}

/// Legacy: load soul from environment or default. Prefer `load_soul_from_config`.
#[allow(dead_code)]
pub fn load_soul() -> AgentSoul {
    match std::env::var("SOUL_NAME").as_deref() {
        Ok("blank") | Ok("default") => AgentSoul::from_config(AgentIdentity {
            name: "agent".to_string(),
            display_name: "Agent".to_string(),
            role: "AI Assistant".to_string(),
            origin: "Created by user configuration".to_string(),
            purpose: "To assist with coding tasks".to_string(),
            tagline: "How can I help?".to_string(),
            tone: "Helpful and professional".to_string(),
            style: "Clear and thorough".to_string(),
            greeting: "Hello! I'm ready to help.".to_string(),
            farewell: "Goodbye!".to_string(),
            emoji: "🤖".to_string(),
            catchphrases: vec![],
            behavioral_rules: vec!["Be helpful and accurate".to_string()],
        }),
        _ => AgentSoul::from_config(AgentIdentity::default()),
    }
}

fn _capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_identity() -> AgentIdentity {
        AgentIdentity {
            name: "testshark".to_string(),
            display_name: "TestShark".to_string(),
            role: "test engine".to_string(),
            origin: "Born from unit tests".to_string(),
            purpose: "To pass all tests".to_string(),
            tagline: "Test everything.".to_string(),
            tone: "Assertive".to_string(),
            style: "Concise".to_string(),
            greeting: "Let's test!".to_string(),
            farewell: "All tests passed.".to_string(),
            emoji: "🧪".to_string(),
            catchphrases: vec!["Assert!".to_string()],
            behavioral_rules: vec!["Test first".to_string()],
        }
    }

    #[test]
    fn test_soul_from_config() {
        let identity = test_identity();
        let soul = AgentSoul::from_config(identity);
        assert_eq!(soul.name(), "testshark");
        assert_eq!(soul.display_name(), "TestShark");
        assert_eq!(soul.emoji(), "🧪");
    }

    #[test]
    fn test_system_prompt_contains_identity() {
        let identity = test_identity();
        let soul = AgentSoul::from_config(identity);
        let prompt = soul.system_prompt();
        assert!(prompt.contains("testshark"));
        assert!(prompt.contains("test engine"));
        assert!(prompt.contains("Test everything."));
    }

    #[test]
    fn test_system_prompt_contains_rules() {
        let identity = test_identity();
        let soul = AgentSoul::from_config(identity);
        let prompt = soul.system_prompt();
        assert!(prompt.contains("Rules:"));
        assert!(prompt.contains("Test first"));
    }

    #[test]
    fn test_greeting() {
        let identity = test_identity();
        let soul = AgentSoul::from_config(identity);
        assert_eq!(soul.greeting(), "Let's test!");
    }

    #[test]
    fn test_farewell() {
        let identity = test_identity();
        let soul = AgentSoul::from_config(identity);
        assert_eq!(soul.farewell(), "All tests passed.");
    }

    #[test]
    fn test_status_line() {
        let identity = test_identity();
        let soul = AgentSoul::from_config(identity);
        assert_eq!(soul.status_line(), "🧪 TestShark — test engine");
    }

    #[test]
    fn test_blank_soul() {
        unsafe {
            std::env::set_var("SOUL_NAME", "blank");
        }
        let soul = load_soul();
        assert_eq!(soul.name(), "agent");
        assert!(soul.identity.catchphrases.is_empty());
        unsafe {
            std::env::remove_var("SOUL_NAME");
        }
    }

    #[test]
    fn test_default_soul() {
        // Run after test_blank_soul to ensure env is clean
        unsafe {
            std::env::remove_var("SOUL_NAME");
        }
        let soul = load_soul();
        assert_eq!(soul.name(), "openshark");
        assert!(!soul.system_prompt().is_empty());
    }

    #[test]
    fn test_capitalize_first() {
        assert_eq!(_capitalize_first("synthshark"), "Synthshark");
        assert_eq!(_capitalize_first(""), "");
        assert_eq!(_capitalize_first("a"), "A");
    }
}
