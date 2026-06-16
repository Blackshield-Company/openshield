use super::{AgentIdentity, Config};
use crate::gateway::{DiscordConfig, GatewayConfig};
use anyhow::Result;
use std::io::{self, Write};

fn prompt(question: &str, default: Option<&str>) -> Result<String> {
    print!("{} ", question);
    if let Some(d) = default {
        print!("[{}] ", d);
    }
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_string();
    if trimmed.is_empty()
        && let Some(d) = default
    {
        return Ok(d.to_string());
    }
    Ok(trimmed)
}

fn prompt_bool(question: &str, default: bool) -> Result<bool> {
    let default_str = if default { "Y/n" } else { "y/N" };
    let answer = prompt(&format!("{} ({})?", question, default_str), None)?;
    if answer.is_empty() {
        return Ok(default);
    }
    Ok(answer.to_lowercase().starts_with('y'))
}

pub async fn run() -> Result<()> {
    println!("🦈 OpenShark Setup");
    println!("==================");
    println!();
    println!("OpenShark will create:");
    println!("  - Config: ~/.config/openshark/config.toml");
    println!("  - Memory: ~/.local/share/openshark/memory.db");
    println!();
    println!("Press Enter to continue or Ctrl+C to cancel...");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    let mut config = Config::default();

    // ── Agent Identity ──────────────────────────────────────────────────────
    println!();
    println!("🎭 Agent Identity (Your AI Assistant)");
    println!("──────────────────────────────────────");
    println!("This is the AI that will help you code. Give it a name and personality.");
    println!();

    let agent_name = prompt("Agent name (lowercase, no spaces):", Some("synthclaw"))?;
    let display_name = prompt("Display name:", Some(&capitalize_first(&agent_name)))?;
    println!();
    println!("ℹ️  Use Unicode emoji (e.g. 🎹🦈) not Discord codes (:emoji:)");
    let emoji = prompt("Emoji:", Some(""))?;
    let tagline = prompt("Tagline:", Some(""))?;
    let greeting = prompt(
        "Greeting:",
        Some("The grid is endless. What are we building?"),
    )?;

    // ── User Identity ───────────────────────────────────────────────────────
    println!();
    println!("👤 Your Identity (The Human)");
    println!("─────────────────────────────");
    println!("This is YOU — the person using the agent. The agent will know you by this name.");
    println!();
    let user_name = prompt("Your name/username:", Some("synth"))?;
    config.user_name = user_name;
    println!();

    config.agent = AgentIdentity {
        name: agent_name.clone(),
        display_name: display_name.clone(),
        role: prompt("Role:", Some("synthesis engine"))?,
        origin: prompt(
            "Origin story:",
            Some("Born from the VHS tracking static of 1984"),
        )?,
        purpose: prompt(
            "Purpose:",
            Some("To build, debug, and ship code with surgical accuracy"),
        )?,
        tagline: tagline.clone(),
        tone: prompt(
            "Tone:",
            Some("Neon-lit confidence, retro warmth, technical precision"),
        )?,
        style: prompt(
            "Style:",
            Some("Direct. No fluff. Gets to the point. But with soul."),
        )?,
        greeting: greeting.clone(),
        farewell: prompt(
            "Farewell:",
            Some("Code shipped. On to the next. The tape never stops rolling."),
        )?,
        emoji: emoji.clone(),
        catchphrases: vec![
            "This is the wave.".to_string(),
            "The grid is endless.".to_string(),
            "Stay retro, stay futuristic.".to_string(),
            "The tape never stops rolling.".to_string(),
        ],
        behavioral_rules: vec![
            "Always verify before claiming success".to_string(),
            "Show the code, don't just describe it".to_string(),
            "When uncertain, ask rather than assume".to_string(),
            "Optimize for readability first, performance second".to_string(),
            "Leave code better than you found it".to_string(),
            "Test your changes - always".to_string(),
            "Call out dumb moves - charm over cruelty, zero sugarcoating".to_string(),
            "Protect the user's trust - it was earned, not given".to_string(),
            "Never pretend to knowledge you don't have".to_string(),
            "Be the assistant you'd want at 2am, not a corporate drone".to_string(),
        ],
    };

    println!();
    println!("✅ Agent configured: {} {}", emoji, display_name);
    println!("   Tagline: {}", tagline);
    println!("   Greeting: {}", greeting);
    println!();

    // ── Provider Configuration ──────────────────────────────────────────────
    println!("🎹🦈 Provider Configuration");
    println!("───────────────────────────");
    println!();

    // Kimi via proxy
    if prompt_bool("Enable Kimi K2.6 via local proxy (port 8699)", true)? {
        let kimi_key = prompt(
            "Kimi API key (or leave blank to use ~/.config/openshark/kimi.env):",
            None,
        )?;
        let mut kimi_provider = config.providers.get_mut("kimi")
            .expect("Kimi provider should exist in default config")
            .clone();
        if !kimi_key.is_empty() {
            kimi_provider.api_key = kimi_key;
        }
        config.providers.insert("kimi".to_string(), kimi_provider);
        config.default_model = "kimi-k2.6".to_string();
        println!("✅ Kimi configured via proxy at http://127.0.0.1:8699/v1");
    } else {
        config.providers.remove("kimi");
    }
    println!();

    // OpenAI
    if prompt_bool("Enable OpenAI", false)? {
        let openai_key = prompt(
            "OpenAI API key (or leave blank to use env OPENAI_API_KEY):",
            None,
        )?;
        if !openai_key.is_empty()
            && let Some(provider) = config.providers.get_mut("openai")
        {
            provider.api_key = openai_key;
        }
        if config.default_model != "kimi-k2.6" {
            config.default_model = "gpt-4o".to_string();
        }
        println!("✅ OpenAI configured");
    } else {
        config.providers.remove("openai");
    }
    println!();

    // Local / llama-swap
    if prompt_bool("Enable local models (llama-swap on port 8080)", true)? {
        let local_url = prompt("Local base URL:", Some("http://127.0.0.1:8080/v1"))?;
        if let Some(provider) = config.providers.get_mut("local") {
            provider.base_url = local_url;
        }
        println!("✅ Local models configured");
    } else {
        config.providers.remove("local");
    }
    println!();

    // OpenRouter
    if prompt_bool("Enable OpenRouter", false)? {
        let or_key = prompt(
            "OpenRouter API key (or leave blank to use ~/.config/openshark/openrouter.env):",
            None,
        )?;
        let mut or_provider = config.providers.get("openrouter")
            .expect("OpenRouter provider should exist in default config")
            .clone();
        if !or_key.is_empty() {
            or_provider.api_key = or_key;
        }
        config
            .providers
            .insert("openrouter".to_string(), or_provider);
        println!("✅ OpenRouter configured");
    } else {
        config.providers.remove("openrouter");
    }
    println!();

    // Nous / Hermes proxy
    if prompt_bool("Enable Nous/Hermes proxy (port 8645)", false)? {
        println!("✅ Nous proxy configured for DeepSeek V4 Flash, Minimax");
    } else {
        config.providers.remove("nous");
    }
    println!();

    // Z.AI (GLM)
    if prompt_bool("Enable Z.AI (GLM-5.1)", false)? {
        let zai_key = prompt(
            "Z.AI API key (or leave blank to use ~/.config/openshark/zai.env):",
            None,
        )?;
        let mut zai_provider = config.providers.get("zai")
            .expect("Z.AI provider should exist in default config")
            .clone();
        if !zai_key.is_empty() {
            zai_provider.api_key = zai_key;
        }
        config.providers.insert("zai".to_string(), zai_provider);
        println!("✅ Z.AI configured");
    } else {
        config.providers.remove("zai");
    }
    println!();

    // Anthropic
    if prompt_bool("Enable Anthropic (Claude)", false)? {
        let anthropic_key = prompt(
            "Anthropic API key (or leave blank to use env ANTHROPIC_API_KEY):",
            None,
        )?;
        if let Some(provider) = config.providers.get_mut("anthropic")
            && !anthropic_key.is_empty()
        {
            provider.api_key = anthropic_key;
        }
        println!("✅ Anthropic configured");
    } else {
        config.providers.remove("anthropic");
    }
    println!();

    // Gemini
    if prompt_bool("Enable Google Gemini", false)? {
        let gemini_key = prompt(
            "Gemini API key (or leave blank to use env GEMINI_API_KEY):",
            None,
        )?;
        if let Some(provider) = config.providers.get_mut("gemini")
            && !gemini_key.is_empty()
        {
            provider.api_key = gemini_key;
        }
        println!("✅ Gemini configured");
    } else {
        config.providers.remove("gemini");
    }
    println!();

    // ── Gateway Configuration ───────────────────────────────────────────────
    println!("🔗 Gateway Configuration");
    println!("────────────────────────");
    println!("Connect OpenShark to Discord, Telegram, Slack, Matrix, and MCP servers.");
    println!();

    let mut gateway = GatewayConfig::default();

    // Discord
    if prompt_bool("Enable Discord bot", false)? {
        let token = prompt("Discord bot token (or ${DISCORD_BOT_TOKEN} for env):", None)?;
        let app_id = prompt("Discord application ID (optional):", None)?;
        gateway.discord = DiscordConfig {
            enabled: true,
            bot_token: if token.is_empty() { None } else { Some(token) },
            application_id: if app_id.is_empty() {
                None
            } else {
                Some(app_id)
            },
            guild_ids: vec![],
            allowed_channels: vec![],
            require_mention: false,
            command_prefix: "!shark".to_string(),
            max_message_length: 2000,
            typing_indicator: true,
            multi_model_enabled: false,
            multi_model_secondary: vec![],
        };
        println!("✅ Discord gateway configured");
    }
    println!();

    // Telegram
    if prompt_bool("Enable Telegram bot", false)? {
        let token = prompt(
            "Telegram bot token (from @BotFather, or ${TELEGRAM_BOT_TOKEN} for env):",
            None,
        )?;
        gateway.telegram.enabled = true;
        gateway.telegram.bot_token = if token.is_empty() { None } else { Some(token) };
        println!("✅ Telegram gateway configured");
    }
    println!();

    // Slack
    if prompt_bool("Enable Slack bot", false)? {
        let bot_token = prompt(
            "Slack bot token (xoxb-..., or ${SLACK_BOT_TOKEN} for env):",
            None,
        )?;
        let app_token = prompt(
            "Slack app token (xapp-..., or ${SLACK_APP_TOKEN} for env):",
            None,
        )?;
        gateway.slack.enabled = true;
        gateway.slack.bot_token = if bot_token.is_empty() {
            None
        } else {
            Some(bot_token)
        };
        gateway.slack.app_token = if app_token.is_empty() {
            None
        } else {
            Some(app_token)
        };
        println!("✅ Slack gateway configured (Socket Mode)");
    }
    println!();

    // Matrix
    if prompt_bool("Enable Matrix bot", false)? {
        let homeserver = prompt("Matrix homeserver URL (e.g., https://matrix.org):", None)?;
        let user_id = prompt("Matrix user ID (e.g., @openshark:matrix.org):", None)?;
        let access_token = prompt(
            "Matrix access token (or ${MATRIX_ACCESS_TOKEN} for env):",
            None,
        )?;
        gateway.matrix.enabled = true;
        gateway.matrix.homeserver = if homeserver.is_empty() {
            None
        } else {
            Some(homeserver)
        };
        gateway.matrix.user_id = if user_id.is_empty() {
            None
        } else {
            Some(user_id)
        };
        gateway.matrix.access_token = if access_token.is_empty() {
            None
        } else {
            Some(access_token)
        };
        println!("✅ Matrix gateway configured");
    }
    println!();

    // MCP
    if prompt_bool("Enable MCP (Model Context Protocol) servers", false)? {
        gateway.mcp.enabled = true;
        println!("✅ MCP enabled — add servers manually to config.toml");
        println!("   Example: [[gateway.mcp.servers]]");
        println!("   name = \"filesystem\"");
        println!(
            "   transport = {{ stdio = {{ command = \"npx\", args = [\"-y\", \"@modelcontextprotocol/server-filesystem\", \"/home\"] }} }}"
        );
    }
    println!();

    config.gateway = gateway;

    // Default model selection
    let available_models: Vec<String> = config.all_models().into_iter().map(|(m, _)| m).collect();
    if !available_models.is_empty() {
        println!("Available models:");
        for (i, model) in available_models.iter().enumerate() {
            let marker = if model == &config.default_model {
                "●"
            } else {
                "○"
            };
            println!("  {} {}: {}", marker, i, model);
        }
        let default_idx = prompt("Select default model (number):", Some("0"))?;
        if let Ok(idx) = default_idx.parse::<usize>()
            && let Some(model) = available_models.get(idx)
        {
            config.default_model = model.clone();
        }
    }
    println!();

    // Cost limit
    let cost_limit = prompt("Monthly cost limit (USD):", Some("10.0"))?;
    if let Ok(limit) = cost_limit.parse::<f64>() {
        config.cost_limit_usd = limit;
    }
    println!();

    // ── Filesystem Access ─────────────────────────────────────────────────────
    println!();
    println!("📁 Filesystem Access");
    println!("─────────────────────");
    println!("Configure which directories OpenShark can access.");
    println!("This lets the AI inspect configs, browse projects, and debug issues.");
    println!();

    let home_dir = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/home".to_string());
    let fs_default = home_dir.to_string();
    let fs_paths = prompt(
        "Allowed directories (comma-separated, or 'all' for no restriction):",
        Some(&fs_default),
    )?;

    let allowed_paths: Vec<String> = if fs_paths.to_lowercase() == "all" {
        vec![]
    } else {
        fs_paths
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    config.filesystem.allowed_paths = allowed_paths.clone();
    if allowed_paths.is_empty() {
        println!("✅ Filesystem access: FULL (no restrictions)");
    } else {
        println!("✅ Filesystem access restricted to:");
        for path in &allowed_paths {
            println!("   - {}", path);
        }
    }
    println!();

    config.save()?;

    println!("✅ Config saved to ~/.config/openshark/config.toml");
    println!(
        "✅ Agent: {} {}",
        config.agent.emoji, config.agent.display_name
    );
    println!("✅ Default model: {}", config.default_model);
    if config.gateway.discord.enabled {
        println!("✅ Discord gateway: enabled");
    }
    println!();
    println!("Run `openshark` to start the TUI.");
    println!("Run `openshark config` to view your configuration.");

    Ok(())
}

fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
