use crate::memory::ToolCall;
use crate::providers::Message;
use crate::tools::{ToolSuggestion, detect_tool_suggestions, find_tool};
use crate::tui::App;
use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

pub(crate) fn handle_user_tool_invocation(app: &mut App, input: &str) -> Result<()> {
    let rest = &input[5..];
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    if parts.is_empty() {
        return Ok(());
    }

    let tool_name = parts[0];
    let args = parts.get(1).unwrap_or(&"");

    // SECURITY GATE: Check tool call before execution
    match app.security_engine.check_tool_call(tool_name, args) {
        crate::security::SecurityDecision::Allow => {
            // Proceed with execution
        }
        crate::security::SecurityDecision::RequireApproval { reason, risk_level } => {
            app.add_system_message(format!(
                "🔒 Security: Tool '{}' requires approval\n  Reason: {}\n  Risk: {:?}\n  Use 'y' to approve or 'n' to deny",
                tool_name, reason, risk_level
            ));
            // Store pending approval state could be added here
            return Ok(());
        }
        crate::security::SecurityDecision::Deny { reason } => {
            app.add_system_message(format!(
                "🚫 Security: Tool '{}' blocked\n  Reason: {}",
                tool_name, reason
            ));
            app.security_engine.audit(
                tool_name,
                args,
                false,
                crate::security::RiskLevel::Critical,
                &reason,
            );
            return Ok(());
        }
    }

    let found_tool =
        find_tool(tool_name).ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", tool_name))?;

    app.add_system_message(format!("🔧 Using tool: {}", tool_name));

    match found_tool.execute(args) {
        Ok(result) => {
            let sanitized = app.security_engine.sanitize_output(tool_name, &result);
            app.add_system_message(format!(
                "Result: {}",
                &sanitized[..sanitized.len().min(500)]
            ));

            let tool_call = ToolCall {
                id: Uuid::new_v4().to_string(),
                session_id: app.session_id.clone(),
                tool_name: tool_name.to_string(),
                args: args.to_string(),
                result: sanitized.clone(),
                success: true,
                created_at: Utc::now(),
            };
            let _ = app.memory.save_tool_call(&tool_call);
            app.tool_calls_count += 1;
            app.security_engine.audit(
                tool_name,
                args,
                true,
                crate::security::RiskLevel::Low,
                "approved",
            );

            app.model_messages.push(Message {
                role: "user".to_string(),
                content: format!("Tool {} returned: {}", tool_name, sanitized),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
        }
        Err(e) => {
            app.add_system_message(format!("Tool error: {}", e));
            let tool_call = ToolCall {
                id: Uuid::new_v4().to_string(),
                session_id: app.session_id.clone(),
                tool_name: tool_name.to_string(),
                args: args.to_string(),
                result: e.to_string(),
                success: false,
                created_at: Utc::now(),
            };
            let _ = app.memory.save_tool_call(&tool_call);
            app.security_engine.audit(
                tool_name,
                args,
                false,
                crate::security::RiskLevel::High,
                &e.to_string(),
            );
        }
    }

    Ok(())
}
pub(crate) fn detect_high_confidence_suggestion(content: &str) -> Option<ToolSuggestion> {
    let suggestions = detect_tool_suggestions(content);
    suggestions.into_iter().find(|s| s.confidence >= 0.6)
}
