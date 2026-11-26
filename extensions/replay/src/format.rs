//! Plain-text formatting for `openshield replay list` and `open`.

use crate::store::{SessionListItem, SharePack, TimelineEvent};

/// Format a session list as a table.
pub fn format_list(items: &[SessionListItem]) -> String {
    if items.is_empty() {
        return "no sessions found".to_string();
    }
    let mut lines = vec![format!(
        "{:<24} {:<16} {:>6} {:>8} {:>12}  {}",
        "SESSION", "MODEL", "MSGS", "TOKENS", "COST(est)", "TITLE"
    )];
    for item in items {
        let cost = item
            .estimated_cost_usd
            .map(|c| format!("${c:.4}"))
            .unwrap_or_else(|| "-".to_string());
        let title = item.title.clone().unwrap_or_default();
        let title = title.replace('\n', " ");
        lines.push(format!(
            "{:<24} {:<16} {:>6} {:>8} {:>12}  {}",
            trunc(&item.id, 24),
            trunc(&item.model, 16),
            item.message_count,
            item.tokens_used,
            cost,
            title
        ));
    }
    lines.join("\n")
}

/// Format a session timeline for the terminal.
pub fn format_timeline(pack: &SharePack) -> String {
    let mut lines = Vec::new();
    let cost = pack
        .metadata
        .estimated_cost_usd
        .map(|c| {
            if pack.metadata.cost_is_estimate {
                format!("{c:.6} (estimate)")
            } else {
                format!("{c:.6}")
            }
        })
        .unwrap_or_else(|| "-".to_string());
    lines.push(format!(
        "session: {}  model: {}  tokens: {}  cost: {}",
        pack.session_id, pack.model, pack.metadata.tokens_used, cost
    ));
    if let Some(report) = &pack.redaction {
        lines.push(format!(
            "redaction: {} replacement(s) in {:?}",
            report.replacements, report.categories
        ));
    }
    lines.push(String::new());
    for event in &pack.timeline {
        match event {
            TimelineEvent::Message {
                timestamp,
                role,
                content,
                tokens_used,
                estimated_cost_usd,
            } => {
                let extra = match (tokens_used, estimated_cost_usd) {
                    (Some(t), Some(c)) => format!("  tokens={t} cost≈${c:.6}"),
                    (Some(t), None) => format!("  tokens={t}"),
                    _ => String::new(),
                };
                lines.push(format!(
                    "[{}] message {role}{extra}",
                    timestamp.to_rfc3339()
                ));
                lines.push(indent(content));
            }
            TimelineEvent::ToolCall {
                timestamp,
                id,
                tool_name,
                args,
                result,
                success,
            } => {
                let status = if *success { "ok" } else { "fail" };
                lines.push(format!(
                    "[{}] tool {tool_name} {status}  id={id}",
                    timestamp.to_rfc3339()
                ));
                lines.push(indent(&format!("args: {args}")));
                lines.push(indent(&format!("result: {result}")));
            }
            TimelineEvent::Approval {
                timestamp,
                id,
                tool,
                args,
                risk,
                reason,
                decision,
            } => {
                lines.push(format!(
                    "[{}] approval {decision}  tool={tool} risk={risk} id={id}",
                    timestamp.to_rfc3339()
                ));
                lines.push(indent(&format!("reason: {reason}")));
                lines.push(indent(&format!("args: {args}")));
            }
            TimelineEvent::Audit {
                timestamp,
                tool,
                approved,
                risk_level,
                reason,
            } => {
                let decision = if *approved { "allowed" } else { "denied" };
                lines.push(format!(
                    "[{}] audit {decision}  tool={tool} risk={risk_level}",
                    timestamp.to_rfc3339()
                ));
                lines.push(indent(&format!("reason: {reason}")));
            }
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn trunc(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 1 {
        s.chars().take(max).collect()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}
