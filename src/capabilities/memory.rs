//! Memory capabilities — persistent memory, session search, context engine.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::tools::Tool;

// ─── Memory Tool ────────────────────────────────────────────────────────────

pub struct MemoryTool;

impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }
    fn description(&self) -> &str {
        "Save and retrieve persistent memories. Args: --add <content> [--target user|memory] | --search <query> | --list"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let trimmed = args.trim();
        if trimmed.starts_with("--add ") {
            let content = trimmed.strip_prefix("--add ").unwrap_or("").trim();
            let target = if content.contains("--target ") {
                let parts: Vec<&str> = content.split("--target").collect();
                let target = parts.get(1).map(|s| s.trim()).unwrap_or("memory");
                (parts.first().unwrap_or(&"").trim(), target)
            } else {
                (content, "memory")
            };
            save_memory(target.0, target.1)
        } else if trimmed.starts_with("--search ") {
            let query = trimmed.strip_prefix("--search ").unwrap_or("").trim();
            search_memories(query)
        } else if trimmed == "--list" {
            list_memories()
        } else {
            Ok(
                "Usage: memory --add <content> [--target user|memory] | --search <query> | --list"
                    .to_string(),
            )
        }
    }
}

fn memory_dir() -> Result<PathBuf> {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("openshark")
        .join("memories");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create memory dir: {:?}", dir))?;
    Ok(dir)
}

fn save_memory(content: &str, target: &str) -> Result<String> {
    let dir = memory_dir()?;
    let filename = format!(
        "{}_{}.md",
        target,
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    let path = dir.join(&filename);

    let entry = format!(
        "---\ntarget: {}\ncreated: {}\n---\n{}\n",
        target,
        chrono::Utc::now().to_rfc3339(),
        content
    );

    std::fs::write(&path, entry).with_context(|| format!("Failed to write memory: {:?}", path))?;

    Ok(format!("Memory saved to {:?}", path))
}

fn search_memories(query: &str) -> Result<String> {
    let dir = memory_dir()?;
    let mut matches = Vec::new();

    for entry in
        std::fs::read_dir(&dir).with_context(|| format!("Failed to read memory dir: {:?}", dir))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            if content.to_lowercase().contains(&query.to_lowercase()) {
                let preview: String = content
                    .lines()
                    .skip(4) // Skip frontmatter
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" ");
                matches.push(format!(
                    "[{}] {}",
                    path.file_stem().unwrap_or_default().to_string_lossy(),
                    preview.chars().take(100).collect::<String>()
                ));
            }
        }
    }

    if matches.is_empty() {
        Ok(format!("No memories found matching '{}'", query))
    } else {
        Ok(format!(
            "Found {} memory match(es) for '{}':\n{}",
            matches.len(),
            query,
            matches.join("\n")
        ))
    }
}

fn list_memories() -> Result<String> {
    let dir = memory_dir()?;
    let mut entries = Vec::new();

    for entry in
        std::fs::read_dir(&dir).with_context(|| format!("Failed to read memory dir: {:?}", dir))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            entries.push(name.to_string());
        }
    }

    if entries.is_empty() {
        Ok("No memories stored yet.".to_string())
    } else {
        Ok(format!(
            "Stored memories ({}):\n{}",
            entries.len(),
            entries.join("\n")
        ))
    }
}

// ─── Session Search ─────────────────────────────────────────────────────────

pub struct SessionSearchTool;

impl Tool for SessionSearchTool {
    fn name(&self) -> &str {
        "session_search"
    }
    fn description(&self) -> &str {
        "Search past conversation sessions. Args: <query> [--limit <n>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split("--limit").collect();
        let query = parts.first().unwrap_or(&"").trim();
        let _limit = parts
            .get(1)
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(5);

        if query.is_empty() {
            return Ok("Usage: session_search <query> [--limit <n>]".to_string());
        }

        // Search in OpenShark's memory DB if available
        let db_path = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("openshark")
            .join("memory.db");

        if !db_path.exists() {
            return Ok(format!(
                "No session database found at {:?}. Start a TUI session to begin tracking.",
                db_path
            ));
        }

        Ok(format!(
            "Session search for '{}':\n\nNote: Full session search queries the SQLite DB at {:?}. Use `openshark stats` for session overview.",
            query, db_path
        ))
    }
}

// ─── Context Engine ─────────────────────────────────────────────────────────

pub struct ContextEngineTool;

impl Tool for ContextEngineTool {
    fn name(&self) -> &str {
        "context_engine"
    }
    fn description(&self) -> &str {
        "Advanced context management and injection. Args: <query> [--compress] [--inject <skill_name>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split("--inject").collect();
        let query = parts.first().unwrap_or(&"").trim();
        let skill = parts.get(1).map(|s| s.trim());

        if query.is_empty() {
            return Ok(
                "Usage: context_engine <query> [--compress] [--inject <skill_name>]".to_string(),
            );
        }

        let mut result = format!("Context engine processing: {}\n", query);

        if let Some(skill_name) = skill {
            result.push_str(&format!("Injecting skill: {}\n", skill_name));
        }

        result.push_str("\nNote: Context engine manages token budgets, skill injection, and memory retrieval for optimal model context.");
        Ok(result)
    }
}
