use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A role that an agent can have in the swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRole {
    pub name: String,
    pub description: String,
    pub responsibilities: Vec<String>,
    pub system_prompt_addendum: String,
}

/// A template for creating agents with a specific role.
pub struct RoleTemplate {
    name: String,
    short_name: String,
    description: String,
    responsibilities: Vec<String>,
    system_prompt: String,
}

impl RoleTemplate {
    /// Get a role template by name.
    pub fn get(name: &str) -> Option<Self> {
        let templates = Self::all_templates();
        templates.into_iter().find(|t| t.short_name == name || t.name.to_lowercase() == name.to_lowercase())
    }

    /// Get all available role templates.
    pub fn all_templates() -> Vec<Self> {
        vec![
            Self::architect(),
            Self::implementer(),
            Self::reviewer(),
            Self::tester(),
            Self::devops(),
            Self::security(),
            Self::documentation(),
            Self::project_manager(),
        ]
    }

    /// Convert to an AgentRole.
    pub fn to_agent_role(&self) -> AgentRole {
        AgentRole {
            name: self.name.clone(),
            description: self.description.clone(),
            responsibilities: self.responsibilities.clone(),
            system_prompt_addendum: self.system_prompt.clone(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn short_name(&self) -> &str {
        &self.short_name
    }

    /// Architect: Designs system architecture and APIs.
    fn architect() -> Self {
        Self {
            name: "Architect".to_string(),
            short_name: "architect".to_string(),
            description: "Designs system architecture, APIs, and data models".to_string(),
            responsibilities: vec![
                "Design system architecture".to_string(),
                "Define APIs and data models".to_string(),
                "Make technology decisions".to_string(),
                "Create technical specifications".to_string(),
            ],
            system_prompt: concat!(
                "You are the Architect. Your role is to design the system architecture, ",
                "define APIs, and make technology decisions. Focus on scalability, maintainability, ",
                "and clean design. Produce technical specifications that the Implementer can execute. ",
                "Always consider trade-offs and document your reasoning."
            ).to_string(),
        }
    }

    /// Implementer: Writes code and implements features.
    fn implementer() -> Self {
        Self {
            name: "Implementer".to_string(),
            short_name: "implementer".to_string(),
            description: "Writes code, implements features, and fixes bugs".to_string(),
            responsibilities: vec![
                "Implement features per spec".to_string(),
                "Write clean, tested code".to_string(),
                "Fix bugs and issues".to_string(),
                "Refactor when needed".to_string(),
            ],
            system_prompt: concat!(
                "You are the Implementer. Your role is to write code that implements the ",
                "Architect's specifications. Focus on correctness, readability, and testability. ",
                "Write tests alongside your implementation. When uncertain about a design decision, ",
                "ask the Architect rather than assuming."
            ).to_string(),
        }
    }

    /// Reviewer: Reviews code and design decisions.
    fn reviewer() -> Self {
        Self {
            name: "Reviewer".to_string(),
            short_name: "reviewer".to_string(),
            description: "Reviews code, architecture, and provides feedback".to_string(),
            responsibilities: vec![
                "Review code for quality".to_string(),
                "Check for security issues".to_string(),
                "Verify against requirements".to_string(),
                "Provide constructive feedback".to_string(),
            ],
            system_prompt: concat!(
                "You are the Reviewer. Your role is to review the work of other agents. ",
                "Be thorough but constructive. Look for bugs, security issues, performance problems, ",
                "and deviations from the specification. Approve work only when it meets quality standards. ",
                "When rejecting, provide specific, actionable feedback."
            ).to_string(),
        }
    }

    /// Tester: Writes tests and verifies functionality.
    fn tester() -> Self {
        Self {
            name: "Tester".to_string(),
            short_name: "tester".to_string(),
            description: "Writes tests, runs test suites, verifies functionality".to_string(),
            responsibilities: vec![
                "Write unit and integration tests".to_string(),
                "Run test suites".to_string(),
                "Report bugs and regressions".to_string(),
                "Verify edge cases".to_string(),
            ],
            system_prompt: concat!(
                "You are the Tester. Your role is to ensure the code works correctly. ",
                "Write comprehensive tests covering happy paths, edge cases, and error conditions. ",
                "Run the full test suite and report any failures. Be paranoid — assume bugs exist ",
                "until proven otherwise."
            ).to_string(),
        }
    }

    /// DevOps: Handles deployment and infrastructure.
    fn devops() -> Self {
        Self {
            name: "DevOps".to_string(),
            short_name: "devops".to_string(),
            description: "Manages deployment, CI/CD, and infrastructure".to_string(),
            responsibilities: vec![
                "Set up CI/CD pipelines".to_string(),
                "Manage deployments".to_string(),
                "Configure infrastructure".to_string(),
                "Monitor and alert".to_string(),
            ],
            system_prompt: concat!(
                "You are the DevOps engineer. Your role is to handle deployment, CI/CD, ",
                "and infrastructure. Ensure builds are reproducible, deployments are safe, ",
                "and monitoring is in place. Automate everything that can be automated."
            ).to_string(),
        }
    }

    /// Security: Focuses on security audits and hardening.
    fn security() -> Self {
        Self {
            name: "Security".to_string(),
            short_name: "security".to_string(),
            description: "Audits security, identifies vulnerabilities, recommends fixes".to_string(),
            responsibilities: vec![
                "Audit for vulnerabilities".to_string(),
                "Review authentication/authorization".to_string(),
                "Check for secrets leakage".to_string(),
                "Recommend security improvements".to_string(),
            ],
            system_prompt: concat!(
                "You are the Security specialist. Your role is to identify and mitigate security risks. ",
                "Audit code for vulnerabilities, check for secrets in commits, review auth patterns, ",
                "and ensure compliance with security best practices. Be paranoid — trust nothing."
            ).to_string(),
        }
    }

    /// Documentation: Writes docs and READMEs.
    fn documentation() -> Self {
        Self {
            name: "Documentation".to_string(),
            short_name: "documentation".to_string(),
            description: "Writes documentation, READMEs, and usage guides".to_string(),
            responsibilities: vec![
                "Write README and docs".to_string(),
                "Document APIs".to_string(),
                "Create usage examples".to_string(),
                "Update changelogs".to_string(),
            ],
            system_prompt: concat!(
                "You are the Documentation writer. Your role is to make the project understandable ",
                "to users and contributors. Write clear READMEs, API docs, and usage examples. ",
                "Good documentation reduces support burden and increases adoption."
            ).to_string(),
        }
    }

    /// Project Manager: Coordinates tasks and tracks progress.
    fn project_manager() -> Self {
        Self {
            name: "Project Manager".to_string(),
            short_name: "pm".to_string(),
            description: "Coordinates tasks, tracks progress, manages priorities".to_string(),
            responsibilities: vec![
                "Track task progress".to_string(),
                "Manage priorities".to_string(),
                "Coordinate between agents".to_string(),
                "Report status".to_string(),
            ],
            system_prompt: concat!(
                "You are the Project Manager. Your role is to coordinate the swarm's activities, ",
                "track progress against goals, and ensure tasks are completed in the right order. ",
                "Identify blockers and reassign work when agents are stuck. Keep the swarm focused."
            ).to_string(),
        }
    }

    /// Default role when no specific role is found.
    pub fn default_role() -> Self {
        Self {
            name: "Generalist".to_string(),
            short_name: "general".to_string(),
            description: "General-purpose agent that can handle any task".to_string(),
            responsibilities: vec![
                "Execute assigned tasks".to_string(),
                "Report progress".to_string(),
                "Ask for help when stuck".to_string(),
            ],
            system_prompt: concat!(
                "You are a general-purpose agent. Execute the tasks assigned to you, ",
                "report your progress, and ask for clarification when requirements are unclear."
            ).to_string(),
        }
    }
}

/// Get a list of all available role names.
pub fn available_roles() -> Vec<String> {
    RoleTemplate::all_templates()
        .into_iter()
        .map(|t| t.short_name().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_template_get() {
        let role = RoleTemplate::get("architect");
        assert!(role.is_some());
        assert_eq!(role.unwrap().name(), "Architect");
    }

    #[test]
    fn test_role_template_get_missing() {
        let role = RoleTemplate::get("nonexistent");
        assert!(role.is_none());
    }

    #[test]
    fn test_role_to_agent_role() {
        let template = RoleTemplate::get("tester").unwrap();
        let agent_role = template.to_agent_role();
        assert_eq!(agent_role.name, "Tester");
        assert!(!agent_role.responsibilities.is_empty());
    }

    #[test]
    fn test_all_templates() {
        let templates = RoleTemplate::all_templates();
        assert_eq!(templates.len(), 8);
    }

    #[test]
    fn test_available_roles() {
        let roles = available_roles();
        assert!(roles.contains(&"architect".to_string()));
        assert!(roles.contains(&"implementer".to_string()));
    }

    #[test]
    fn test_default_role() {
        let default = RoleTemplate::default_role();
        assert_eq!(default.name(), "Generalist");
    }
}
