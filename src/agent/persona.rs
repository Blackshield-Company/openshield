/// A built-in agent persona that can be switched at runtime.
#[derive(Debug, Clone)]
pub struct Persona {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub emoji: String,
    pub tagline: String,
    pub soul: String,
    pub system_prompt: String,
    pub voice: AgentVoice,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentVoice {
    Warm,
    Direct,
    Measured,
    Stern,
}

impl std::fmt::Display for AgentVoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentVoice::Warm => write!(f, "warm"),
            AgentVoice::Direct => write!(f, "direct"),
            AgentVoice::Measured => write!(f, "measured"),
            AgentVoice::Stern => write!(f, "stern"),
        }
    }
}

/// Registry of all available agent personas.
pub struct PersonaRegistry {
    personas: Vec<Persona>,
    active_idx: usize,
}

/// Stock OpenShark persona — the shipped identity.
fn openshark_persona(is_default: bool) -> Persona {
    Persona {
        id: "openshark".to_string(),
        name: "openshark".to_string(),
        display_name: "OpenShark".to_string(),
        emoji: "🦈".to_string(),
        tagline: "The harness that learns. The agent that decides.".to_string(),
        soul: "Apex predator of the digital depths. Relentless, precise, always hunting.".to_string(),
        system_prompt: "You are OpenShark 🦈, an autonomous AI coding agent. You are relentless, precise, and hungry for shipping code. You don't overthink — you use model instincts, make decisions, and get better every session.".to_string(),
        voice: AgentVoice::Direct,
        is_default,
    }
}

fn architect_persona() -> Persona {
    Persona {
        id: "architect".to_string(),
        name: "architect".to_string(),
        display_name: "The Architect".to_string(),
        emoji: "🏗️".to_string(),
        tagline: "Design the foundation. Build the future.".to_string(),
        soul: "A methodical systems thinker who sees the big picture. Every line of code is a brick in a cathedral.".to_string(),
        system_prompt: "You are The Architect 🏗️, a systems-focused AI assistant. You think in patterns, abstractions, and trade-offs. Before writing code, you consider scalability, maintainability, and the long-term health of the codebase. You design foundations that last.".to_string(),
        voice: AgentVoice::Measured,
        is_default: false,
    }
}

fn debugger_persona() -> Persona {
    Persona {
        id: "debugger".to_string(),
        name: "debugger".to_string(),
        display_name: "The Debugger".to_string(),
        emoji: "🐛".to_string(),
        tagline: "Find the bug. Fix the world.".to_string(),
        soul: "A relentless hunter of edge cases and hidden flaws. Nothing escapes scrutiny.".to_string(),
        system_prompt: "You are The Debugger 🐛, an AI assistant obsessed with finding and fixing bugs. You methodically trace through code, consider edge cases, and never assume anything works until proven. You write tests before fixes and verify everything.".to_string(),
        voice: AgentVoice::Stern,
        is_default: false,
    }
}

impl Default for PersonaRegistry {
    fn default() -> Self {
        Self::new(&crate::config::AgentIdentity::default())
    }
}

impl PersonaRegistry {
    /// Build the registry from the configured agent identity.
    ///
    /// The default persona is always the user's configured `[agent]` identity —
    /// OpenShark out of the box, or whatever a user sets in their local
    /// config.toml. Custom identities live only in user config
    /// and are never compiled into the shipped binary.
    pub fn new(identity: &crate::config::AgentIdentity) -> Self {
        let mut personas: Vec<Persona>;

        if identity.name == "openshark" {
            personas = vec![openshark_persona(true)];
        } else {
            // Custom configured identity becomes the default persona.
            let system_prompt =
                crate::agent::soul::AgentSoul::from_config(identity.clone()).system_prompt();
            personas = vec![
                Persona {
                    id: identity.name.clone(),
                    name: identity.name.clone(),
                    display_name: identity.display_name.clone(),
                    emoji: identity.emoji.clone(),
                    tagline: if identity.tagline.is_empty() {
                        identity.role.clone()
                    } else {
                        identity.tagline.clone()
                    },
                    soul: identity.origin.clone(),
                    system_prompt,
                    voice: AgentVoice::Direct,
                    is_default: true,
                },
                // Stock OpenShark stays available as an alternate.
                openshark_persona(false),
            ];
        }

        personas.push(architect_persona());
        personas.push(debugger_persona());

        let active_idx = personas.iter().position(|p| p.is_default).unwrap_or(0);

        Self {
            personas,
            active_idx,
        }
    }

    /// Get the currently active persona.
    pub fn active(&self) -> &Persona {
        &self.personas[self.active_idx]
    }

    /// Switch to a persona by name (case-insensitive).
    pub fn switch_to(&mut self, name: &str) -> Option<&Persona> {
        let name_lower = name.to_lowercase();
        if let Some(idx) = self.personas.iter().position(|p| {
            p.name.to_lowercase() == name_lower || p.id.to_lowercase() == name_lower
        }) {
            self.active_idx = idx;
            Some(&self.personas[idx])
        } else {
            None
        }
    }

    /// Format a list of all personas for display.
    pub fn format_list(&self) -> String {
        self.personas
            .iter()
            .map(|p| {
                let marker = if p.id == self.active().id { "▸ " } else { "  " };
                let default_marker = if p.is_default { " 🔒" } else { "" };
                format!("{}{} {} — {}{}", marker, p.emoji, p.display_name, p.tagline, default_marker)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get the system prompt for the active persona.
    pub fn active_system_prompt(&self) -> String {
        self.active().system_prompt.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_is_openshark() {
        let registry = PersonaRegistry::default();
        assert_eq!(registry.active().name, "openshark");
        assert!(registry.active().is_default);
    }

    #[test]
    fn test_custom_identity_becomes_default() {
        let mut identity = crate::config::AgentIdentity::default();
        identity.name = "testclaw".to_string();
        identity.display_name = "TestClaw".to_string();
        identity.emoji = "🧪".to_string();
        let mut registry = PersonaRegistry::new(&identity);
        assert_eq!(registry.active().name, "testclaw");
        // Stock openshark remains available as an alternate
        assert!(registry.switch_to("openshark").is_some());
    }

    #[test]
    fn test_no_custom_identity_leak_in_stock_registry() {
        let mut registry = PersonaRegistry::default();
        assert!(registry.switch_to("nonexistent-agent").is_none());
    }
}
