//! Slash Commands — Quick shortcuts for common operations.
//!
//! Inspired by Claude Code and Cline. Type `/command` in the TUI
//! for instant actions without typing full prompts.
//!
//! Commands:
//!   /bug        — Report a bug with context
//!   /commit     — Generate commit message and commit
//!   /test       — Run tests for current changes
//!   /explain    — Explain selected code or last output
//!   /refactor   — Refactor selected code
//!   /fix        — Fix errors from last output
//!   /doc        — Generate documentation
//!   /clear      — Clear conversation history
//!   /context    — Show memory hierarchy summary
//!   /model      — Switch model
//!   /mode       — Switch agent mode (safe/full-send)
//!   /help       — Show available slash commands
//!   /stats      — Show session stats
//!   /export     — Export session to file
//!   /checkpoint — Save a checkpoint
//!   /restore    — Restore from checkpoint
//!   /diff       — Show git diff
//!   /branch     — Show/create git branch
//!   /pr         — Create a pull request
//!   /review     — Request code review
//!   /lint       — Run linter on changed files
//!   /yolo       — Toggle yolo mode (auto-approve all)

use std::collections::HashMap;

/// A slash command definition.
#[derive(Debug, Clone)]
pub struct SlashCommand {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    #[allow(dead_code)]
    pub description: &'static str,
    #[allow(dead_code)]
    pub usage: &'static str,
    #[allow(dead_code)]
    pub category: SlashCategory,
    #[allow(dead_code)]
    pub requires_args: bool,
    pub handler: fn(&str) -> SlashResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCategory {
    Git,
    Code,
    Session,
    Agent,
    Debug,
    Utility,
}

impl std::fmt::Display for SlashCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlashCategory::Git => write!(f, "Git"),
            SlashCategory::Code => write!(f, "Code"),
            SlashCategory::Session => write!(f, "Session"),
            SlashCategory::Agent => write!(f, "Agent"),
            SlashCategory::Debug => write!(f, "Debug"),
            SlashCategory::Utility => write!(f, "Utility"),
        }
    }
}

/// Result of executing a slash command.
#[derive(Debug, Clone)]
pub enum SlashResult {
    /// Execute a tool directly.
    Tool { name: String, args: String },
    /// Send a prompt to the model.
    Prompt(String),
    /// Toggle a boolean setting.
    Toggle { setting: String, value: bool },
    /// Switch to a different mode.
    SwitchMode(String),
    /// No-op / handled internally.
    Handled,
    /// Error.
    Error(String),
}

/// Registry of all slash commands.
pub struct SlashRegistry {
    commands: HashMap<String, SlashCommand>,
}

impl Default for SlashRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SlashRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
        };
        registry.register_all();
        registry
    }

    fn register_all(&mut self) {
        let commands = vec![
            SlashCommand {
                name: "bug",
                aliases: &["issue", "report"],
                description: "Report a bug with current context",
                usage: "/bug [description]",
                category: SlashCategory::Debug,
                requires_args: false,
                handler: |args| {
                    let desc = if args.is_empty() {
                        "Describe the bug you encountered"
                    } else {
                        args
                    };
                    SlashResult::Prompt(format!(
                        "I encountered a bug: {}. Please help me investigate and fix it. Look at the recent changes and error output to identify the root cause.",
                        desc
                    ))
                },
            },
            SlashCommand {
                name: "commit",
                aliases: &["git-commit", "save"],
                description: "Generate commit message and commit changes",
                usage: "/commit [message]",
                category: SlashCategory::Git,
                requires_args: false,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Prompt(
                            "Please generate a concise, conventional commit message for the current changes, then commit them using git.".to_string(),
                        )
                    } else {
                        SlashResult::Tool {
                            name: "git".to_string(),
                            args: format!("commit -m \"{}\"", args),
                        }
                    }
                },
            },
            SlashCommand {
                name: "test",
                aliases: &["t", "run-tests"],
                description: "Run tests for current changes",
                usage: "/test [path]",
                category: SlashCategory::Code,
                requires_args: false,
                handler: |args| {
                    let path = if args.is_empty() { "." } else { args };
                    SlashResult::Tool {
                        name: "test".to_string(),
                        args: format!("run {}", path),
                    }
                },
            },
            SlashCommand {
                name: "explain",
                aliases: &["x", "why"],
                description: "Explain selected code or last output",
                usage: "/explain [code or reference]",
                category: SlashCategory::Code,
                requires_args: false,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Prompt(
                            "Please explain the code or output from our last interaction. Break it down step by step.".to_string(),
                        )
                    } else {
                        SlashResult::Prompt(format!(
                            "Please explain this code in detail: {}\n\nBreak down what it does, why it works, and any potential issues.",
                            args
                        ))
                    }
                },
            },
            SlashCommand {
                name: "refactor",
                aliases: &["r", "rewrite"],
                description: "Refactor selected code",
                usage: "/refactor [description]",
                category: SlashCategory::Code,
                requires_args: false,
                handler: |args| {
                    let desc = if args.is_empty() {
                        "improve readability and performance"
                    } else {
                        args
                    };
                    SlashResult::Prompt(format!(
                        "Please refactor the following code to {}. Maintain the same behavior but improve the implementation.",
                        desc
                    ))
                },
            },
            SlashCommand {
                name: "fix",
                aliases: &["f", "repair"],
                description: "Fix errors from last output",
                usage: "/fix [error description]",
                category: SlashCategory::Debug,
                requires_args: false,
                handler: |args| {
                    let desc = if args.is_empty() {
                        "the errors in the last output"
                    } else {
                        args
                    };
                    SlashResult::Prompt(format!(
                        "Please fix {}. Analyze the error, identify the root cause, and provide a corrected solution.",
                        desc
                    ))
                },
            },
            SlashCommand {
                name: "doc",
                aliases: &["docs", "document"],
                description: "Generate documentation",
                usage: "/doc [target]",
                category: SlashCategory::Code,
                requires_args: false,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Prompt(
                            "Please generate comprehensive documentation for the code we've been working on. Include README, API docs, and inline comments where needed.".to_string(),
                        )
                    } else {
                        SlashResult::Prompt(format!(
                            "Please generate documentation for: {}. Include purpose, usage examples, parameters, and return values.",
                            args
                        ))
                    }
                },
            },
            SlashCommand {
                name: "clear",
                aliases: &["cls", "reset"],
                description: "Clear conversation history",
                usage: "/clear",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |_args| SlashResult::Handled,
            },
            SlashCommand {
                name: "context",
                aliases: &["ctx", "memory"],
                description: "Show memory hierarchy summary",
                usage: "/context",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |_args| SlashResult::Handled,
            },
            SlashCommand {
                name: "autoctx",
                aliases: &["auto-context", "context-mode"],
                description: "Toggle auto-context mode — auto-identify relevant files for queries",
                usage: "/autoctx [on|off]",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |args| {
                    match args {
                        "on" => SlashResult::Toggle {
                            setting: "auto_context".to_string(),
                            value: true,
                        },
                        "off" => SlashResult::Toggle {
                            setting: "auto_context".to_string(),
                            value: false,
                        },
                        _ => SlashResult::Toggle {
                            setting: "auto_context".to_string(),
                            value: true, // toggle
                        },
                    }
                },
            },
            SlashCommand {
                name: "ctx",
                aliases: &["pin", "unpin"],
                description: "Pin/unpin files to context: /ctx <path>, /ctx clear, /ctx list",
                usage: "/ctx <path> | /ctx clear | /ctx list | /ctx unpin <path>",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |args| {
                    let trimmed = args.trim();
                    if trimmed.is_empty() || trimmed == "list" {
                        SlashResult::Handled
                    } else if trimmed == "clear" {
                        SlashResult::Toggle {
                            setting: "ctx_clear".to_string(),
                            value: true,
                        }
                    } else if let Some(rest) = trimmed.strip_prefix("unpin ") {
                        SlashResult::Prompt(format!("__ctx_unpin__ {}", rest))
                    } else {
                        SlashResult::Prompt(format!("__ctx_pin__ {}", trimmed))
                    }
                },
            },
            SlashCommand {
                name: "search",
                aliases: &["find", "lookup"],
                description: "Search across all sessions (including archived) for messages matching a query",
                usage: "/search <query>",
                category: SlashCategory::Session,
                requires_args: true,
                handler: |args| {
                    if args.trim().is_empty() {
                        SlashResult::Error("Usage: /search <query>".to_string())
                    } else {
                        SlashResult::Prompt(format!("__search__ {}", args.trim()))
                    }
                },
            },
            SlashCommand {
                name: "plugin",
                aliases: &["plugins", "hook", "hooks"],
                description: "Plugin management: list, create, reload",
                usage: "/plugin [list|create <name>|reload]",
                category: SlashCategory::Utility,
                requires_args: false,
                handler: |args| {
                    let trimmed = args.trim();
                    if trimmed.is_empty() || trimmed == "list" {
                        SlashResult::Handled
                    } else if let Some(name) = trimmed.strip_prefix("create ") {
                        if name.is_empty() {
                            SlashResult::Error("Usage: /plugin create <name>".to_string())
                        } else {
                            SlashResult::Prompt(format!("__plugin_create__ {}", name))
                        }
                    } else if trimmed == "reload" {
                        SlashResult::Prompt("__plugin_reload__".to_string())
                    } else {
                        SlashResult::Error("Usage: /plugin [list|create <name>|reload]".to_string())
                    }
                },
            },
            SlashCommand {
                name: "swarm",
                aliases: &["multi", "ensemble"],
                description: "Query multiple providers in parallel and view consensus/divergence",
                usage: "/swarm <query>",
                category: SlashCategory::Agent,
                requires_args: true,
                handler: |args| {
                    if args.trim().is_empty() {
                        SlashResult::Error("Usage: /swarm <query>".to_string())
                    } else {
                        SlashResult::Prompt(format!("__swarm__ {}", args.trim()))
                    }
                },
            },
            SlashCommand {
                name: "index",
                aliases: &["idx", "lookup", "symbol"],
                description: "Search the code index for symbols (functions, structs, etc.)",
                usage: "/index <query>",
                category: SlashCategory::Utility,
                requires_args: true,
                handler: |args| {
                    if args.trim().is_empty() {
                        SlashResult::Error("Usage: /index <query>".to_string())
                    } else {
                        SlashResult::Prompt(format!("__index__ {}", args.trim()))
                    }
                },
            },
            SlashCommand {
                name: "model",
                aliases: &["m", "switch-model"],
                description: "Switch active model",
                usage: "/model <model-name>",
                category: SlashCategory::Agent,
                requires_args: true,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Error("Usage: /model <model-name>".to_string())
                    } else {
                        SlashResult::SwitchMode(format!("model:{}", args))
                    }
                },
            },
            SlashCommand {
                name: "mode",
                aliases: &["safe", "fullsend"],
                description: "Toggle agent mode (safe/full-send)",
                usage: "/mode [safe|fullsend]",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |args| {
                    match args {
                        "safe" => SlashResult::Toggle {
                            setting: "autonomous".to_string(),
                            value: false,
                        },
                        "fullsend" | "full-send" | "yolo" => SlashResult::Toggle {
                            setting: "autonomous".to_string(),
                            value: true,
                        },
                        _ => SlashResult::Toggle {
                            setting: "autonomous".to_string(),
                            value: true, // toggle
                        },
                    }
                },
            },
            SlashCommand {
                name: "agent",
                aliases: &["persona", "identity"],
                description: "Switch agent persona or list available agents",
                usage: "/agent [name]",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Prompt("List all available agent personas: OpenShark (🦈), synthclaw (🦞), The Architect (🏗️), The Debugger (🐛). Show which one is currently active and describe each briefly.".to_string())
                    } else {
                        SlashResult::Prompt(format!(
                            "Switch to the '{}' agent persona. Adopt their voice, style, and perspective immediately. Confirm the switch.",
                            args
                        ))
                    }
                },
            },
            SlashCommand {
                name: "soul",
                aliases: &["whoami", "persona"],
                description: "Display current agent's full identity and persona",
                usage: "/soul",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |_args| {
                    SlashResult::Prompt("Display your full identity, soul, and system prompt. Show your name, role, origin, purpose, tone, style, catchphrases, and behavioral rules. Be thorough.".to_string())
                },
            },
            SlashCommand {
                name: "help",
                aliases: &["h", "?"],
                description: "Show available slash commands",
                usage: "/help [category]",
                category: SlashCategory::Utility,
                requires_args: false,
                handler: |_args| SlashResult::Handled,
            },
            SlashCommand {
                name: "plugins",
                aliases: &["hooks", "extensions"],
                description: "List loaded plugin hooks",
                usage: "/plugins",
                category: SlashCategory::Utility,
                requires_args: false,
                handler: |_args| SlashResult::Handled,
            },
            SlashCommand {
                name: "stats",
                aliases: &["status", "info"],
                description: "Show session stats",
                usage: "/stats",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |_args| SlashResult::Handled,
            },
            SlashCommand {
                name: "status",
                aliases: &["st"],
                description: "Show live context — model, session, git branch, and working directory",
                usage: "/status",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |_args| SlashResult::Prompt("__status__".to_string()),
            },
            SlashCommand {
                name: "resume",
                aliases: &["continue", "last"],
                description: "Resume the latest conversation from the session archive",
                usage: "/resume [session-id]",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Prompt("__resume_latest__".to_string())
                    } else {
                        SlashResult::Prompt(format!("__resume__ {}", args))
                    }
                },
            },
            SlashCommand {
                name: "sethome",
                aliases: &["home", "set-channel"],
                description: "Mark current chat as the home channel for cron job deliveries",
                usage: "/sethome",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |_args| SlashResult::Prompt("__sethome__".to_string()),
            },
            SlashCommand {
                name: "map",
                aliases: &["repo-map", "structure"],
                description: "Build and display repo map — codebase structure overview",
                usage: "/map [path]",
                category: SlashCategory::Code,
                requires_args: false,
                handler: |args| {
                    let path = if args.is_empty() { "." } else { args };
                    SlashResult::Tool {
                        name: "repo_map".to_string(),
                        args: path.to_string(),
                    }
                },
            },
            SlashCommand {
                name: "export",
                aliases: &["save-session", "backup"],
                description: "Export session to file",
                usage: "/export [filename]",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |args| {
                    let _filename = if args.is_empty() {
                        format!(
                            "openshark-session-{}.json",
                            chrono::Utc::now().format("%Y%m%d-%H%M%S")
                        )
                    } else {
                        args.to_string()
                    };
                    SlashResult::Handled
                },
            },
            SlashCommand {
                name: "checkpoint",
                aliases: &["cp", "snapshot"],
                description: "Save a checkpoint",
                usage: "/checkpoint [name]",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |args| {
                    let name = if args.is_empty() {
                        format!("checkpoint-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"))
                    } else {
                        args.to_string()
                    };
                    SlashResult::Tool {
                        name: "checkpoint".to_string(),
                        args: format!("save {}", name),
                    }
                },
            },
            SlashCommand {
                name: "restore",
                aliases: &["revert", "rollback"],
                description: "Restore from checkpoint",
                usage: "/restore <checkpoint-name>",
                category: SlashCategory::Session,
                requires_args: true,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Error("Usage: /restore <checkpoint-name>".to_string())
                    } else {
                        SlashResult::Tool {
                            name: "checkpoint".to_string(),
                            args: format!("restore {}", args),
                        }
                    }
                },
            },
            SlashCommand {
                name: "diff",
                aliases: &["d", "changes"],
                description: "Show git diff",
                usage: "/diff [file]",
                category: SlashCategory::Git,
                requires_args: false,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Tool {
                            name: "git".to_string(),
                            args: "diff".to_string(),
                        }
                    } else {
                        SlashResult::Tool {
                            name: "git".to_string(),
                            args: format!("diff {}", args),
                        }
                    }
                },
            },
            SlashCommand {
                name: "branch",
                aliases: &["b", "br"],
                description: "Show or create git branch",
                usage: "/branch [name]",
                category: SlashCategory::Git,
                requires_args: false,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Tool {
                            name: "git".to_string(),
                            args: "branch".to_string(),
                        }
                    } else {
                        SlashResult::Tool {
                            name: "git".to_string(),
                            args: format!("checkout -b {}", args),
                        }
                    }
                },
            },
            SlashCommand {
                name: "pr",
                aliases: &["pull-request", "merge-request"],
                description: "Create a pull request",
                usage: "/pr [title]",
                category: SlashCategory::Git,
                requires_args: false,
                handler: |args| {
                    let title = if args.is_empty() {
                        "Auto-generated PR".to_string()
                    } else {
                        args.to_string()
                    };
                    SlashResult::Prompt(format!(
                        "Please create a pull request with the title: \"{}\". Generate a good description based on the recent commits and changes.",
                        title
                    ))
                },
            },
            SlashCommand {
                name: "review",
                aliases: &["cr", "code-review"],
                description: "Request code review",
                usage: "/review [file]",
                category: SlashCategory::Code,
                requires_args: false,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Tool {
                            name: "guardian".to_string(),
                            args: "review recent".to_string(),
                        }
                    } else {
                        SlashResult::Tool {
                            name: "guardian".to_string(),
                            args: format!("review {}", args),
                        }
                    }
                },
            },
            SlashCommand {
                name: "lint",
                aliases: &["l", "check"],
                description: "Run linter on changed files",
                usage: "/lint [path]",
                category: SlashCategory::Code,
                requires_args: false,
                handler: |args| {
                    let path = if args.is_empty() { "." } else { args };
                    SlashResult::Prompt(format!(
                        "Please run the appropriate linter on {} and fix any issues found. Use cargo clippy for Rust, eslint for JS, etc.",
                        path
                    ))
                },
            },
            SlashCommand {
                name: "yolo",
                aliases: &["auto", "approve-all"],
                description: "Toggle yolo mode (auto-approve all tools)",
                usage: "/yolo",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |_args| SlashResult::Toggle {
                    setting: "yolo".to_string(),
                    value: true,
                },
            },
            SlashCommand {
                name: "approve",
                aliases: &["a", "yes"],
                description: "Approve pending batch of tool suggestions",
                usage: "/approve [all|index]",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |args| {
                    if args == "all" || args.is_empty() {
                        SlashResult::Toggle {
                            setting: "batch_approve_all".to_string(),
                            value: true,
                        }
                    } else {
                        SlashResult::Toggle {
                            setting: format!("batch_approve:{}", args),
                            value: true,
                        }
                    }
                },
            },
            SlashCommand {
                name: "reject",
                aliases: &["n", "no", "skip"],
                description: "Reject pending batch of tool suggestions",
                usage: "/reject [all|index]",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |args| {
                    if args == "all" || args.is_empty() {
                        SlashResult::Toggle {
                            setting: "batch_reject_all".to_string(),
                            value: false,
                        }
                    } else {
                        SlashResult::Toggle {
                            setting: format!("batch_reject:{}", args),
                            value: false,
                        }
                    }
                },
            },
            SlashCommand {
                name: "plan",
                aliases: &["p"],
                description: "Switch to plan mode — analyze and propose strategy only, no edits",
                usage: "/plan",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |_args| SlashResult::Toggle {
                    setting: "plan_mode".to_string(),
                    value: true,
                },
            },
            SlashCommand {
                name: "act",
                aliases: &["a", "execute"],
                description: "Switch to act mode — execute tools and make changes",
                usage: "/act",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |_args| SlashResult::Toggle {
                    setting: "plan_mode".to_string(),
                    value: false,
                },
            },
            SlashCommand {
                name: "ask",
                aliases: &["q", "question"],
                description: "Ask a question without editing — read-only Q and A mode",
                usage: "/ask [question]",
                category: SlashCategory::Code,
                requires_args: false,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Prompt(
                            "I'm in ask mode — I'll answer your questions without making any changes to files. What would you like to know?".to_string(),
                        )
                    } else {
                        SlashResult::Prompt(format!(
                            "Answer this question without editing any files: {}",
                            args
                        ))
                    }
                },
            },
            SlashCommand {
                name: "compact",
                aliases: &["compress", "summarize"],
                description: "Compact conversation context — summarize and truncate history",
                usage: "/compact",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |_args| SlashResult::Toggle {
                    setting: "compact_context".to_string(),
                    value: true,
                },
            },
            SlashCommand {
                name: "architect",
                aliases: &["design", "plan-model"],
                description: "Switch to architect model for planning and design",
                usage: "/architect",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |_args| SlashResult::Toggle {
                    setting: "architect_mode".to_string(),
                    value: true,
                },
            },
            SlashCommand {
                name: "editor",
                aliases: &["code", "edit-model"],
                description: "Switch to editor model for code generation",
                usage: "/editor",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |_args| SlashResult::Toggle {
                    setting: "editor_mode".to_string(),
                    value: true,
                },
            },
            SlashCommand {
                name: "effort",
                aliases: &["thinking", "depth"],
                description: "Set effort level: low, medium, high, xhigh",
                usage: "/effort <low|medium|high|xhigh>",
                category: SlashCategory::Agent,
                requires_args: true,
                handler: |args| {
                    let level = args.trim().to_lowercase();
                    match level.as_str() {
                        "low" | "medium" | "high" | "xhigh" => {
                            SlashResult::SwitchMode(format!("effort:{}", level))
                        }
                        _ => {
                            SlashResult::Error("Usage: /effort <low|medium|high|xhigh>".to_string())
                        }
                    }
                },
            },
            SlashCommand {
                name: "headless",
                aliases: &["ci", "batch", "auto"],
                description: "Run a task in headless/background mode — non-interactive execution",
                usage: "/headless <task description>",
                category: SlashCategory::Agent,
                requires_args: true,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Error("Usage: /headless <task description>".to_string())
                    } else {
                        SlashResult::Tool {
                            name: "headless".to_string(),
                            args: args.to_string(),
                        }
                    }
                },
            },
            SlashCommand {
                name: "usage",
                aliases: &["cost", "tokens"],
                description: "Show session token usage and estimated cost",
                usage: "/usage",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |_args| SlashResult::Handled,
            },
            SlashCommand {
                name: "archive",
                aliases: &["hide", "stash"],
                description: "Archive the current session (hide from active list)",
                usage: "/archive",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |_args| SlashResult::Toggle {
                    setting: "archive_session".to_string(),
                    value: true,
                },
            },
            SlashCommand {
                name: "unarchive",
                aliases: &["restore-session", "unhide"],
                description: "Unarchive a session by ID",
                usage: "/unarchive <session-id>",
                category: SlashCategory::Session,
                requires_args: true,
                handler: |args| {
                    if args.is_empty() {
                        SlashResult::Error("Usage: /unarchive <session-id>".to_string())
                    } else {
                        SlashResult::Toggle {
                            setting: format!("unarchive_session:{}", args),
                            value: true,
                        }
                    }
                },
            },
            SlashCommand {
                name: "archived",
                aliases: &["hidden", "stashed"],
                description: "List archived sessions",
                usage: "/archived",
                category: SlashCategory::Session,
                requires_args: false,
                handler: |_args| SlashResult::Handled,
            },
            SlashCommand {
                name: "vim",
                aliases: &["vi"],
                description: "Toggle vim mode for input editing",
                usage: "/vim",
                category: SlashCategory::Utility,
                requires_args: false,
                handler: |_args| SlashResult::Toggle {
                    setting: "vim_mode".to_string(),
                    value: true,
                },
            },
            SlashCommand {
                name: "profile",
                aliases: &["perm", "security-profile"],
                description: "Switch permission profile: coding, review, safe, yolo, readonly",
                usage: "/profile [coding|review|safe|yolo|readonly]",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |args| {
                    let name = if args.is_empty() { "coding" } else { args };
                    match name {
                        "coding" | "review" | "safe" | "yolo" | "readonly" => {
                            SlashResult::SwitchMode(format!("profile:{}", name))
                        }
                        _ => SlashResult::Error(
                            "Usage: /profile <coding|review|safe|yolo|readonly>".to_string(),
                        ),
                    }
                },
            },
            SlashCommand {
                name: "profiles",
                aliases: &["perms", "security-profiles"],
                description: "List available permission profiles",
                usage: "/profiles",
                category: SlashCategory::Agent,
                requires_args: false,
                handler: |_args| SlashResult::Handled,
            },
            SlashCommand {
                name: "mouse",
                aliases: &["click"],
                description: "Toggle mouse support in TUI",
                usage: "/mouse",
                category: SlashCategory::Utility,
                requires_args: false,
                handler: |_args| SlashResult::Toggle {
                    setting: "mouse".to_string(),
                    value: true,
                },
            },
        ];

        for cmd in commands {
            self.commands.insert(cmd.name.to_string(), cmd.clone());
            for alias in cmd.aliases {
                self.commands.insert(alias.to_string(), cmd.clone());
            }
        }
    }

    /// Parse and execute a slash command from user input.
    /// Returns None if the input is not a slash command.
    pub fn execute(&self, input: &str) -> Option<SlashResult> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        let without_slash = &trimmed[1..];
        let parts: Vec<&str> = without_slash.splitn(2, ' ').collect();
        let cmd_name = parts.first()?;
        let args = parts.get(1).unwrap_or(&"").trim();

        let cmd = self.commands.get(*cmd_name)?;
        Some((cmd.handler)(args))
    }

    /// Get all commands for help display.
    #[allow(dead_code)]
    pub fn all_commands(&self) -> Vec<&SlashCommand> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for cmd in self.commands.values() {
            if seen.insert(cmd.name) {
                result.push(cmd);
            }
        }
        result.sort_by(|a, b| a.name.cmp(b.name));
        result
    }

    /// Get commands by category.
    #[allow(dead_code)]
    pub fn by_category(&self, category: SlashCategory) -> Vec<&SlashCommand> {
        self.all_commands()
            .into_iter()
            .filter(|c| c.category == category)
            .collect()
    }

    /// Format help text for display.
    #[allow(dead_code)]
    pub fn format_help(&self) -> String {
        let mut lines = vec![
            "🦈 Slash Commands".to_string(),
            "═══════════════════════════════════════════════════════════".to_string(),
            String::new(),
        ];

        let categories = [
            SlashCategory::Code,
            SlashCategory::Git,
            SlashCategory::Session,
            SlashCategory::Agent,
            SlashCategory::Debug,
            SlashCategory::Utility,
        ];

        for cat in &categories {
            lines.push(format!("📁 {}", cat));
            lines.push("─".repeat(50));
            for cmd in self.by_category(*cat) {
                lines.push(format!("  /{:<12} {}", cmd.name, cmd.description));
                lines.push(format!("               Usage: {}", cmd.usage));
            }
            lines.push(String::new());
        }

        lines.push("Tip: Type / followed by Tab to autocomplete.".to_string());
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = SlashRegistry::new();
        assert!(!registry.all_commands().is_empty());
    }

    #[test]
    fn test_slash_test_command() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/test");
        assert!(
            matches!(result, Some(SlashResult::Tool { name, args }) if name == "test" && args == "run .")
        );
    }

    #[test]
    fn test_slash_test_with_path() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/test src/");
        assert!(
            matches!(result, Some(SlashResult::Tool { name, args }) if name == "test" && args == "run src/")
        );
    }

    #[test]
    fn test_slash_commit_no_args() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/commit");
        assert!(matches!(result, Some(SlashResult::Prompt(_))));
    }

    #[test]
    fn test_slash_commit_with_message() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/commit fix bug");
        assert!(
            matches!(result, Some(SlashResult::Tool { name, args }) if name == "git" && args.contains("fix bug"))
        );
    }

    #[test]
    fn test_slash_clear() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/clear");
        assert!(matches!(result, Some(SlashResult::Handled)));
    }

    #[test]
    fn test_slash_mode_toggle() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/mode");
        assert!(
            matches!(result, Some(SlashResult::Toggle { setting, value: true }) if setting == "autonomous")
        );
    }

    #[test]
    fn test_slash_mode_safe() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/mode safe");
        assert!(
            matches!(result, Some(SlashResult::Toggle { setting, value: false }) if setting == "autonomous")
        );
    }

    #[test]
    fn test_slash_alias() {
        let registry = SlashRegistry::new();
        // /t is alias for /test
        let result = registry.execute("/t");
        assert!(matches!(result, Some(SlashResult::Tool { name, .. }) if name == "test"));
    }

    #[test]
    fn test_non_slash_input() {
        let registry = SlashRegistry::new();
        let result = registry.execute("hello world");
        assert!(result.is_none());
    }

    #[test]
    fn test_slash_help_formatting() {
        let registry = SlashRegistry::new();
        let help = registry.format_help();
        assert!(help.contains("/test"));
        assert!(help.contains("/commit"));
        assert!(help.contains("/bug"));
        assert!(help.contains("Code"));
        assert!(help.contains("Git"));
    }

    #[test]
    fn test_slash_diff() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/diff");
        assert!(
            matches!(result, Some(SlashResult::Tool { name, args }) if name == "git" && args == "diff")
        );
    }

    #[test]
    fn test_slash_branch_create() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/branch feature-x");
        assert!(
            matches!(result, Some(SlashResult::Tool { name, args }) if name == "git" && args.contains("feature-x"))
        );
    }

    #[test]
    fn test_slash_yolo() {
        let registry = SlashRegistry::new();
        let result = registry.execute("/yolo");
        assert!(
            matches!(result, Some(SlashResult::Toggle { setting, value: true }) if setting == "yolo")
        );
    }
}
