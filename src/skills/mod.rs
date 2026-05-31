use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// A skill is a reusable piece of procedural knowledge — YAML frontmatter + markdown body.
///
/// Skills are loaded from `~/.config/openshark/skills/` and auto-injected into
/// the system prompt when their trigger keywords match the user's query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Skill name (kebab-case, unique).
    pub name: String,
    /// Short description of what this skill does.
    pub description: String,
    /// Keywords that trigger this skill to be loaded.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// Tags for categorization.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The skill content (markdown instructions for the model).
    #[serde(skip)]
    pub content: String,
    /// Source file path.
    #[serde(skip)]
    pub path: PathBuf,
}

/// Registry of all loaded skills, indexed by trigger keyword.
pub struct SkillRegistry {
    /// All loaded skills.
    skills: Vec<Skill>,
    /// Trigger keyword -> skill index mapping.
    trigger_index: HashMap<String, Vec<usize>>,
    /// Skills directory path.
    skills_dir: PathBuf,
}

impl SkillRegistry {
    /// Create a new skill registry and load all skills from disk.
    pub fn new(skills_dir: PathBuf) -> Result<Self> {
        let mut registry = Self {
            skills: Vec::new(),
            trigger_index: HashMap::new(),
            skills_dir,
        };
        registry.load_all()?;
        Ok(registry)
    }

    /// Load all skills from the skills directory.
    fn load_all(&mut self) -> Result<()> {
        if !self.skills_dir.exists() {
            std::fs::create_dir_all(&self.skills_dir)
                .with_context(|| format!("Failed to create skills dir: {:?}", self.skills_dir))?;
            // Create built-in skills on first run
            self.create_builtin_skills()?;
        }

        let entries = std::fs::read_dir(&self.skills_dir)
            .with_context(|| format!("Failed to read skills dir: {:?}", self.skills_dir))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                match self.load_skill(&path) {
                    Ok(skill) => {
                        let idx = self.skills.len();
                        for trigger in &skill.triggers {
                            let trigger_lower = trigger.to_lowercase();
                            self.trigger_index
                                .entry(trigger_lower)
                                .or_default()
                                .push(idx);
                        }
                        self.skills.push(skill);
                    }
                    Err(e) => {
                        warn!("Failed to load skill {:?}: {}", path, e);
                    }
                }
            }
        }

        info!(
            "Loaded {} skills with {} triggers",
            self.skills.len(),
            self.trigger_index.len()
        );
        Ok(())
    }

    /// Load a single skill from a markdown file with YAML frontmatter.
    fn load_skill(&self, path: &Path) -> Result<Skill> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read skill file: {:?}", path))?;

        // Parse YAML frontmatter between --- markers
        let (frontmatter, body) = parse_frontmatter(&content)
            .with_context(|| format!("Failed to parse frontmatter in {:?}", path))?;

        let mut skill: Skill = serde_yaml::from_str(&frontmatter)
            .with_context(|| format!("Failed to parse YAML frontmatter in {:?}", path))?;

        skill.content = body.trim().to_string();
        skill.path = path.to_path_buf();

        Ok(skill)
    }

    /// Find skills triggered by the given query text.
    pub fn find_triggered(&self, query: &str) -> Vec<&Skill> {
        let query_lower = query.to_lowercase();
        let mut triggered = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (trigger, indices) in &self.trigger_index {
            if query_lower.contains(trigger) {
                for &idx in indices {
                    if seen.insert(idx) {
                        triggered.push(&self.skills[idx]);
                    }
                }
            }
        }

        triggered
    }

    /// Get all loaded skills.
    pub fn all_skills(&self) -> &[Skill] {
        &self.skills
    }

    /// Get skill by name.
    #[allow(dead_code)]
    pub fn get_by_name(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// Create built-in skills on first run.
    fn create_builtin_skills(&self) -> Result<()> {
        let builtins = vec![
            ("rust.md", include_str!("builtin/rust.md")),
            ("docker.md", include_str!("builtin/docker.md")),
            ("git.md", include_str!("builtin/git.md")),
            ("testing.md", include_str!("builtin/testing.md")),
            ("debugging.md", include_str!("builtin/debugging.md")),
        ];

        for (filename, content) in &builtins {
            let path = self.skills_dir.join(filename);
            if !path.exists() {
                std::fs::write(&path, content)
                    .with_context(|| format!("Failed to write builtin skill: {:?}", path))?;
            }
        }

        info!("Created {} built-in skills", builtins.len());
        Ok(())
    }

    /// Reload all skills from disk.
    #[allow(dead_code)]
    pub fn reload(&mut self) -> Result<()> {
        self.skills.clear();
        self.trigger_index.clear();
        self.load_all()
    }
}

/// Parse YAML frontmatter from markdown content.
/// Returns (frontmatter_yaml_string, body_content).
fn parse_frontmatter(content: &str) -> Result<(String, String)> {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        // No frontmatter — treat entire content as body
        return Ok((String::new(), content.to_string()));
    }

    // Find the closing ---
    let after_open = &trimmed[3..];
    if let Some(end_pos) = after_open.find("\n---") {
        let frontmatter = after_open[..end_pos].trim();
        let body = &after_open[end_pos + 4..];
        Ok((frontmatter.to_string(), body.to_string()))
    } else {
        anyhow::bail!("YAML frontmatter not properly closed with '---'")
    }
}

/// Format triggered skills into a system prompt augmentation.
pub fn format_skills_prompt(skills: &[&Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut prompt = String::from("\n\n[RELEVANT SKILLS LOADED]\n\n");
    for skill in skills {
        prompt.push_str(&format!("## {}\n", skill.name));
        prompt.push_str(&format!("{}\n\n", skill.content));
    }
    prompt.push_str("[END SKILLS]\n");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_valid() {
        let content = "---\nname: test-skill\ndescription: A test\ntriggers:\n  - test\n---\n\n# Test Content\nThis is the body.";
        let (frontmatter, body) = parse_frontmatter(content).unwrap();
        assert!(frontmatter.contains("name: test-skill"));
        assert!(body.contains("# Test Content"));
    }

    #[test]
    fn test_parse_frontmatter_no_frontmatter() {
        let content = "# Just markdown\nNo frontmatter here.";
        let (frontmatter, body) = parse_frontmatter(content).unwrap();
        assert!(frontmatter.is_empty());
        assert!(body.contains("Just markdown"));
    }

    #[test]
    fn test_skill_trigger_matching() {
        let skill = Skill {
            name: "rust-error-handling".to_string(),
            description: "Rust error handling patterns".to_string(),
            triggers: vec![
                "rust".to_string(),
                "error".to_string(),
                "anyhow".to_string(),
            ],
            tags: vec!["rust".to_string()],
            content: "Use anyhow for error propagation.".to_string(),
            path: PathBuf::from("/tmp/test.md"),
        };

        let registry = SkillRegistry {
            skills: vec![skill.clone()],
            trigger_index: {
                let mut map = HashMap::new();
                map.insert("rust".to_string(), vec![0]);
                map.insert("error".to_string(), vec![0]);
                map.insert("anyhow".to_string(), vec![0]);
                map
            },
            skills_dir: PathBuf::from("/tmp"),
        };

        let triggered = registry.find_triggered("How do I handle errors in rust?");
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].name, "rust-error-handling");
    }
}
