use anyhow::Result;
use serenity::all::{Command, CommandOptionType, GuildId};
use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::client::Context as SerenityContext;

/// Register slash commands with Discord.
pub async fn register_commands(
    ctx: &SerenityContext,
    guild_id: Option<GuildId>,
) -> Result<Vec<Command>> {
    let commands = vec![
        // ─── Core Chat ───
        CreateCommand::new("chat")
            .description("Chat with OpenShark")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "message", "Your message")
                    .required(true),
            ),
        CreateCommand::new("new").description("Start a fresh conversation (clear history)"),
        CreateCommand::new("system")
            .description("Set a custom system prompt for this channel")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "prompt",
                    "The system prompt to use",
                )
                .required(true),
            ),
        CreateCommand::new("reset").description("Reset to default system prompt and clear history"),
        // ─── Model Management ───
        CreateCommand::new("model")
            .description("List or switch active LLM model")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "name",
                    "Model name to switch to (leave empty to list)",
                )
                .required(false),
            ),
        CreateCommand::new("models").description("List all available models with details"),
        // ─── Multi-Model ───
        CreateCommand::new("multi")
            .description("Control multi-model comparison mode")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "action",
                    "Action: on, off, toggle, set",
                )
                .required(false),
            )
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "models",
                    "Comma-separated model names for comparison (used with action:set)",
                )
                .required(false),
            ),
        // ─── Agent / Task ───
        CreateCommand::new("agent")
            .description("Run an autonomous agent task")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "task", "Task description")
                    .required(true),
            ),
        // ─── Tools ───
        CreateCommand::new("tools").description("List available tools"),
        CreateCommand::new("tool")
            .description("Execute a specific tool directly")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "name", "Tool name")
                    .required(true),
            )
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "args", "Tool arguments")
                    .required(true),
            ),
        // ─── Memory ───
        CreateCommand::new("memory")
            .description("Search conversation memory")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "query", "Search query")
                    .required(true),
            ),
        CreateCommand::new("remember")
            .description("Save a fact to long-term memory")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "fact", "Fact to remember")
                    .required(true),
            ),
        // ─── Status / Info ───
        CreateCommand::new("status").description("Check OpenShark status"),
        CreateCommand::new("stats").description("Show usage statistics"),
        // ─── Settings ───
        CreateCommand::new("settings")
            .description("View or change channel settings")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "key",
                    "Setting key (typing_indicator, max_history, require_mention)",
                )
                .required(false),
            )
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "value", "New value")
                    .required(false),
            ),
        // ─── Help ───
        CreateCommand::new("help").description("Show available commands and usage"),
    ];

    let created = if let Some(guild_id) = guild_id {
        guild_id.set_commands(&ctx.http, commands).await?
    } else {
        Command::set_global_commands(&ctx.http, commands).await?
    };

    Ok(created)
}
