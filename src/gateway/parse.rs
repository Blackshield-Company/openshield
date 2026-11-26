//! Shared parsing for channel text commands and approval button payloads.

/// Built-in text or slash commands understood by the supervisor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinCommand {
    Pair { code: String },
    New,
    Status,
    Help,
    Shield { message: Option<String> },
}

/// Decision encoded in an approval button `callback_data` / `custom_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonDecision {
    Approve,
    Reject,
    ApproveAlways,
}

impl ButtonDecision {
    /// Wire string accepted by `POST /api/v1/approvals/:id`.
    pub fn as_str(self) -> &'static str {
        match self {
            ButtonDecision::Approve => "approve",
            ButtonDecision::Reject => "reject",
            ButtonDecision::ApproveAlways => "approve_always",
        }
    }
}

/// Encode an approval button payload. Fits Telegram's 64-byte callback cap.
pub fn encode_approval_button(decision: ButtonDecision, approval_id: &str) -> String {
    let tag = match decision {
        ButtonDecision::Approve => "a",
        ButtonDecision::Reject => "r",
        ButtonDecision::ApproveAlways => "w",
    };
    format!("os:{tag}:{approval_id}")
}

/// Parse `os:a:<id>`, `os:r:<id>`, or `os:w:<id>`.
pub fn parse_approval_button(data: &str) -> Option<(ButtonDecision, String)> {
    let rest = data.strip_prefix("os:")?;
    let (tag, id) = rest.split_once(':')?;
    if id.is_empty() || id.len() > 80 {
        return None;
    }
    let decision = match tag {
        "a" => ButtonDecision::Approve,
        "r" => ButtonDecision::Reject,
        "w" => ButtonDecision::ApproveAlways,
        _ => return None,
    };
    Some((decision, id.to_string()))
}

/// Parse a chat message into a built-in command.
///
/// Handles `/pair`, `/pair@BotName`, `/new`, `/status`, `/help`, `/shield`
/// and the Discord-style `/shield hello`. Unknown slash commands return
/// `None` so the supervisor can ignore them. Bare text is not a command.
pub fn parse_text_command(text: &str) -> Option<BuiltinCommand> {
    let text = text.trim();
    if !text.starts_with('/') {
        return None;
    }
    let without_slash = &text[1..];
    let (raw_name, rest) = match without_slash.split_once(char::is_whitespace) {
        Some((n, r)) => (n, r.trim()),
        None => (without_slash, ""),
    };
    // Telegram appends @BotName to commands in groups.
    let name = raw_name
        .split('@')
        .next()
        .unwrap_or(raw_name)
        .to_ascii_lowercase();
    match name.as_str() {
        "pair" => {
            let code = rest.trim().to_string();
            Some(BuiltinCommand::Pair { code })
        }
        "new" => Some(BuiltinCommand::New),
        "status" => Some(BuiltinCommand::Status),
        "help" => Some(BuiltinCommand::Help),
        "shield" | "chat" => {
            let message = if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            };
            Some(BuiltinCommand::Shield { message })
        }
        _ => None,
    }
}

/// Redact secrets and PII from text that will leave the process toward a
/// chat platform.
pub fn redact_outbound(text: &str) -> String {
    let secrets = crate::security::redact_secrets(text);
    crate::security::quick_redact(&secrets)
}

/// A token is usable when it is present, non-empty, and not a leftover
/// `${ENV}` placeholder.
pub fn token_is_resolved(token: &Option<String>) -> bool {
    match token {
        Some(t) => {
            let t = t.trim();
            !t.is_empty() && !(t.starts_with("${") && t.ends_with('}'))
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pair_and_mention() {
        assert_eq!(
            parse_text_command("  /pair  abc-123  "),
            Some(BuiltinCommand::Pair {
                code: "abc-123".into()
            })
        );
        assert_eq!(
            parse_text_command("/pair@OpenShieldBot deadbeef"),
            Some(BuiltinCommand::Pair {
                code: "deadbeef".into()
            })
        );
    }

    #[test]
    fn parse_simple_commands() {
        assert_eq!(parse_text_command("/new"), Some(BuiltinCommand::New));
        assert_eq!(parse_text_command("/status"), Some(BuiltinCommand::Status));
        assert_eq!(parse_text_command("/help"), Some(BuiltinCommand::Help));
        assert_eq!(
            parse_text_command("/shield hello there"),
            Some(BuiltinCommand::Shield {
                message: Some("hello there".into())
            })
        );
        assert_eq!(
            parse_text_command("/chat"),
            Some(BuiltinCommand::Shield { message: None })
        );
        assert!(parse_text_command("hello").is_none());
        assert!(parse_text_command("/unknown").is_none());
    }

    #[test]
    fn approval_button_roundtrip() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        let data = encode_approval_button(ButtonDecision::Approve, id);
        assert!(data.len() <= 64);
        assert_eq!(
            parse_approval_button(&data),
            Some((ButtonDecision::Approve, id.to_string()))
        );
        assert_eq!(
            parse_approval_button(&encode_approval_button(ButtonDecision::Reject, id)),
            Some((ButtonDecision::Reject, id.to_string()))
        );
        assert_eq!(
            parse_approval_button(&encode_approval_button(ButtonDecision::ApproveAlways, id)),
            Some((ButtonDecision::ApproveAlways, id.to_string()))
        );
        assert!(parse_approval_button("nope").is_none());
        assert!(parse_approval_button("os:x:id").is_none());
    }

    #[test]
    fn redact_strips_keys() {
        let key = format!("sk-{}", "a".repeat(48));
        let out = redact_outbound(&format!("token {key} and user@example.com"));
        assert!(!out.contains(&key));
        assert!(!out.contains("user@example.com"));
    }

    #[test]
    fn token_resolution() {
        assert!(!token_is_resolved(&None));
        assert!(!token_is_resolved(&Some(String::new())));
        assert!(!token_is_resolved(&Some("${TELEGRAM_BOT_TOKEN}".into())));
        assert!(token_is_resolved(&Some("123:ABC".into())));
    }
}
