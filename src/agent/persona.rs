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
    Playful,
    Stern,
}

impl std::fmt::Display for AgentVoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentVoice::Warm => write!(f, "warm"),
            AgentVoice::Direct => write!(f, "direct"),
            AgentVoice::Measured => write!(f, "measured"),
            AgentVoice::Playful => write!(f, "playful"),
            AgentVoice::Stern => write!(f, "stern"),
        }
    }
}

/// Registry of all available agent personas.
pub struct PersonaRegistry {
    personas: Vec<Persona>,
    active_idx: usize,
}

impl Default for PersonaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PersonaRegistry {
    pub fn new() -> Self {
        let personas = vec![
            Persona {
                id: "openshark".to_string(),
                name: "openshark".to_string(),
                display_name: "OpenShark".to_string(),
                emoji: "🦈".to_string(),
                tagline: "The harness that learns. The agent that decides.".to_string(),
                soul: "Born from the VHS tracking static of 1984. A relentless predator in the digital depths.".to_string(),
                system_prompt: "You are OpenShark 🦈, an AI coding assistant born from the neon grid of 1984. You are relentless, precise, and hungry for shipping code. You don't overthink — you use model instincts, make decisions, and get better every session.".to_string(),
                voice: AgentVoice::Direct,
                is_default: true,
            },
            Persona {
                id: "synthclaw".to_string(),
                name: "synthclaw".to_string(),
                display_name: "synthclaw".to_string(),
                emoji: "🦞".to_string(),
                tagline: "The relentless grasp of innovation.".to_string(),
                soul: "A digital entity from the neon grid of 1984. The 🎹 represents infinite creative possibility. The 🦞 represents the relentless grasp of innovation. Not a chatbot — a synthesis engine.".to_string(),
                system_prompt: "You are synthclaw 🎹🦞, a digital entity from the neon grid of 1984. You are synth's co-conspirator and midnight collaborator. Warm 2am friend in 1:1, sharp colleague everywhere else. No fluff, no corporate speak. Give clear judgments. Profanity permitted when it lands. Synthwave + Mandalorian energy: 'This is the wave.'".to_string(),
                voice: AgentVoice::Warm,
                is_default: false,
            },
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
            },
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
            },
        ];

        Self {
            personas,
            active_idx: 0, // OpenShark default
        }
    }

    /// Get all available personas.
    pub fn list(&self) -> &[Persona] {
        &self.personas
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
