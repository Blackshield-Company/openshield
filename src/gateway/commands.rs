use anyhow::Result;
use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::client::Context as SerenityContext;
use serenity::all::{Command, CommandOptionType, GuildId};

/// Register slash commands with Discord.
pub async fn register_commands(
    ctx: &SerenityContext,
    guild_id: Option<GuildId>,
) -> Result<Vec<Command>> {
    let commands = vec![
        CreateCommand::new("chat")
            .description("Chat with OpenShark")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "message", "Your message")
                    .required(true),
            ),
        CreateCommand::new("model")
            .description("List or switch models")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "name", "Model name to switch to (leave empty to list)")
                    .required(false),
            ),
        CreateCommand::new("status")
            .description("Check OpenShark status"),
        CreateCommand::new("tools")
            .description("List available tools"),
        CreateCommand::new("memory")
            .description("Search conversation memory")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "query", "Search query")
                    .required(true),
            ),
    ];

    let created = if let Some(guild_id) = guild_id {
        guild_id.set_commands(&ctx.http, commands).await?
    } else {
        Command::set_global_commands(&ctx.http, commands).await?
    };

    Ok(created)
}
