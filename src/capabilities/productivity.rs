//! Productivity capabilities — todo lists, cron jobs, skill management.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::tools::Tool;

// ─── Todo Tool ──────────────────────────────────────────────────────────────

static TODO_STATE: Mutex<Option<TodoState>> = Mutex::new(None);

struct TodoState {
    items: Vec<TodoItem>,
    next_id: usize,
}

struct TodoItem {
    id: usize,
    content: String,
    status: TodoStatus,
    created_at: String,
}

enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

impl std::fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodoStatus::Pending => write!(f, "pending"),
            TodoStatus::InProgress => write!(f, "in_progress"),
            TodoStatus::Completed => write!(f, "completed"),
            TodoStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

fn with_todo_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut TodoState) -> R,
{
    let mut guard = TODO_STATE.lock().expect("TODO state mutex poisoned");
    if guard.is_none() {
        *guard = Some(TodoState {
            items: Vec::new(),
            next_id: 1,
        });
    }
    f(guard.as_mut().expect("TODO state should be initialized"))
}

pub struct TodoTool;

impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }
    fn description(&self) -> &str {
        "Task planning and management. Args: --add <task> | --complete <id> | --cancel <id> | --list | --clear"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let trimmed = args.trim();
        if trimmed.starts_with("--add ") {
            let task = trimmed.strip_prefix("--add ").unwrap_or("").trim();
            add_todo(task)
        } else if trimmed.starts_with("--complete ") {
            let id = trimmed.strip_prefix("--complete ").unwrap_or("").trim();
            complete_todo(id)
        } else if trimmed.starts_with("--cancel ") {
            let id = trimmed.strip_prefix("--cancel ").unwrap_or("").trim();
            cancel_todo(id)
        } else if trimmed == "--list" || trimmed.is_empty() {
            list_todos()
        } else if trimmed == "--clear" {
            clear_todos()
        } else {
            Ok(
                "Usage: todo --add <task> | --complete <id> | --cancel <id> | --list | --clear"
                    .to_string(),
            )
        }
    }
}

fn add_todo(task: &str) -> Result<String> {
    if task.is_empty() {
        return Ok("Usage: todo --add <task description>".to_string());
    }
    Ok(with_todo_state(|state| {
        let id = state.next_id;
        state.next_id += 1;
        state.items.push(TodoItem {
            id,
            content: task.to_string(),
            status: TodoStatus::Pending,
            created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string(),
        });
        format!("Added todo #{}: {}", id, task)
    }))
}

fn complete_todo(id_str: &str) -> Result<String> {
    let id = id_str.parse::<usize>().unwrap_or(0);
    if id == 0 {
        return Ok("Invalid todo ID. Use --list to see IDs.".to_string());
    }
    Ok(with_todo_state(|state| {
        if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
            item.status = TodoStatus::Completed;
            format!("Completed todo #{}: {}", id, item.content)
        } else {
            format!("Todo #{} not found", id)
        }
    }))
}

fn cancel_todo(id_str: &str) -> Result<String> {
    let id = id_str.parse::<usize>().unwrap_or(0);
    if id == 0 {
        return Ok("Invalid todo ID. Use --list to see IDs.".to_string());
    }
    Ok(with_todo_state(|state| {
        if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
            item.status = TodoStatus::Cancelled;
            format!("Cancelled todo #{}: {}", id, item.content)
        } else {
            format!("Todo #{} not found", id)
        }
    }))
}

fn list_todos() -> Result<String> {
    Ok(with_todo_state(|state| {
        if state.items.is_empty() {
            return "No todos. Add one with: todo --add <task>".to_string();
        }

        let mut lines = vec![format!("Todos ({} total):", state.items.len())];
        for item in &state.items {
            let icon = match item.status {
                TodoStatus::Pending => "○",
                TodoStatus::InProgress => "◐",
                TodoStatus::Completed => "✓",
                TodoStatus::Cancelled => "✗",
            };
            lines.push(format!(
                "  {} #{} [{}] {} ({})",
                icon, item.id, item.status, item.content, item.created_at
            ));
        }
        lines.join("\n")
    }))
}

fn clear_todos() -> Result<String> {
    Ok(with_todo_state(|state| {
        let count = state.items.len();
        state.items.clear();
        state.next_id = 1;
        format!("Cleared {} todo(s)", count)
    }))
}

// ─── Cronjob Tool ───────────────────────────────────────────────────────────

pub struct CronjobTool;

impl Tool for CronjobTool {
    fn name(&self) -> &str {
        "cronjob"
    }
    fn description(&self) -> &str {
        "Schedule recurring tasks. Args: --create --schedule <cron> --prompt <task> [--name <name>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let trimmed = args.trim();
        if trimmed.starts_with("--create ") {
            let rest = trimmed.strip_prefix("--create ").unwrap_or("").trim();
            create_cronjob(rest)
        } else if trimmed == "--list" {
            list_cronjobs()
        } else if trimmed.starts_with("--remove ") {
            let name = trimmed.strip_prefix("--remove ").unwrap_or("").trim();
            remove_cronjob(name)
        } else {
            Ok("Usage: cronjob --create --schedule <cron> --prompt <task> [--name <name>] | --list | --remove <name>".to_string())
        }
    }
}

fn cronjob_dir() -> Result<PathBuf> {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("openshark")
        .join("cronjobs");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create cronjob dir: {:?}", dir))?;
    Ok(dir)
}

fn create_cronjob(args: &str) -> Result<String> {
    // Parse --schedule and --prompt
    let mut schedule = None;
    let mut prompt = None;
    let mut name = None;

    let parts: Vec<&str> = args.split("--").collect();
    for part in parts.iter().skip(1) {
        let trimmed = part.trim();
        if trimmed.starts_with("schedule ") {
            schedule = Some(trimmed.strip_prefix("schedule ").unwrap_or("").trim());
        } else if trimmed.starts_with("prompt ") {
            prompt = Some(trimmed.strip_prefix("prompt ").unwrap_or("").trim());
        } else if trimmed.starts_with("name ") {
            name = Some(trimmed.strip_prefix("name ").unwrap_or("").trim());
        }
    }

    let schedule = schedule.unwrap_or("0 9 * * *");
    let prompt = prompt.unwrap_or("");
    let default_name = format!("job_{}", chrono::Utc::now().timestamp());
    let name = name.unwrap_or(&default_name);

    if prompt.is_empty() {
        return Ok(
            "Usage: cronjob --create --schedule <cron> --prompt <task> [--name <name>]".to_string(),
        );
    }

    let dir = cronjob_dir()?;
    let path = dir.join(format!("{}.toml", name));

    let content = format!(
        "name = \"{}\"\nschedule = \"{}\"\nprompt = \"{}\"\ncreated = \"{}\"\nenabled = true\n",
        name,
        schedule,
        prompt.replace("\"", "\\\""),
        chrono::Utc::now().to_rfc3339()
    );

    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write cronjob: {:?}", path))?;

    Ok(format!(
        "Cronjob '{}' created:\n  Schedule: {}\n  Prompt: {}\n  File: {:?}",
        name, schedule, prompt, path
    ))
}

fn list_cronjobs() -> Result<String> {
    let dir = cronjob_dir()?;
    let mut jobs = Vec::new();

    for entry in
        std::fs::read_dir(&dir).with_context(|| format!("Failed to read cronjob dir: {:?}", dir))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "toml").unwrap_or(false) {
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            jobs.push(name.to_string());
        }
    }

    if jobs.is_empty() {
        Ok("No cronjobs configured.".to_string())
    } else {
        Ok(format!(
            "Configured cronjobs ({}):\n{}",
            jobs.len(),
            jobs.join("\n")
        ))
    }
}

fn remove_cronjob(name: &str) -> Result<String> {
    let dir = cronjob_dir()?;
    let path = dir.join(format!("{}.toml", name));

    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to remove cronjob: {:?}", path))?;
        Ok(format!("Removed cronjob '{}'", name))
    } else {
        Ok(format!("Cronjob '{}' not found", name))
    }
}

// ─── Skills Tool ────────────────────────────────────────────────────────────

pub struct SkillsTool;

impl Tool for SkillsTool {
    fn name(&self) -> &str {
        "skills"
    }
    fn description(&self) -> &str {
        "Load and manage skills. Args: --list | --view <name> | --create <name> <content> | --delete <name>"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let trimmed = args.trim();
        if trimmed == "--list" || trimmed.is_empty() {
            list_skills()
        } else if trimmed.starts_with("--view ") {
            let name = trimmed.strip_prefix("--view ").unwrap_or("").trim();
            view_skill(name)
        } else if trimmed.starts_with("--create ") {
            let rest = trimmed.strip_prefix("--create ").unwrap_or("").trim();
            create_skill(rest)
        } else if trimmed.starts_with("--delete ") {
            let name = trimmed.strip_prefix("--delete ").unwrap_or("").trim();
            delete_skill(name)
        } else {
            Ok("Usage: skills --list | --view <name> | --create <name> <content> | --delete <name>".to_string())
        }
    }
}

fn skills_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("openshark")
        .join("skills");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create skills dir: {:?}", dir))?;
    Ok(dir)
}

fn list_skills() -> Result<String> {
    let dir = skills_dir()?;
    let mut skills = Vec::new();

    for entry in
        std::fs::read_dir(&dir).with_context(|| format!("Failed to read skills dir: {:?}", dir))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            skills.push(name.to_string());
        }
    }

    if skills.is_empty() {
        Ok("No skills found. Create one with: skills --create <name> <content>".to_string())
    } else {
        Ok(format!("Skills ({}):\n{}", skills.len(), skills.join("\n")))
    }
}

fn view_skill(name: &str) -> Result<String> {
    let dir = skills_dir()?;
    let path = dir.join(format!("{}.md", name));

    if !path.exists() {
        return Ok(format!("Skill '{}' not found", name));
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read skill: {:?}", path))?;
    Ok(format!("Skill '{}':\n{}", name, content))
}

fn create_skill(args: &str) -> Result<String> {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    let name = parts.first().unwrap_or(&"").trim();
    let content = parts.get(1).unwrap_or(&"").trim();

    if name.is_empty() {
        return Ok("Usage: skills --create <name> <markdown content>".to_string());
    }

    let dir = skills_dir()?;
    let path = dir.join(format!("{}.md", name));

    let full_content = if content.starts_with("---") {
        content.to_string()
    } else {
        format!(
            "---\nname: {}\ndescription: Auto-created skill\ntriggers:\n  - {}\ntags:\n  - auto\n---\n\n{}",
            name, name, content
        )
    };

    std::fs::write(&path, full_content)
        .with_context(|| format!("Failed to write skill: {:?}", path))?;

    Ok(format!("Created skill '{}' at {:?}", name, path))
}

fn delete_skill(name: &str) -> Result<String> {
    let dir = skills_dir()?;
    let path = dir.join(format!("{}.md", name));

    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to delete skill: {:?}", path))?;
        Ok(format!("Deleted skill '{}'", name))
    } else {
        Ok(format!("Skill '{}' not found", name))
    }
}
