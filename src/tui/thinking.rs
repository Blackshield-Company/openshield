/// Split content into thinking/reasoning and regular content.
/// Handles <think>...</think> blocks from Kimi models.
pub(crate) fn split_thinking_content(content: &str) -> (String, String) {
    let mut thinking = String::new();
    let mut regular = String::new();
    let mut in_think = false;
    let mut think_buffer = String::new();
    let mut regular_buffer = String::new();

    // Simple state machine to parse think tags
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Check for <think> or </think>
            let mut tag = String::new();
            tag.push(ch);
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '>' {
                    tag.push(chars.next().unwrap());
                    break;
                } else {
                    tag.push(chars.next().unwrap());
                }
            }

            if tag == "<think>" {
                // Flush regular buffer
                if !regular_buffer.is_empty() {
                    regular.push_str(&regular_buffer);
                    regular_buffer.clear();
                }
                in_think = true;
            } else if tag == "</think>" {
                // Flush think buffer
                if !think_buffer.is_empty() {
                    if !thinking.is_empty() {
                        thinking.push('\n');
                    }
                    thinking.push_str(&think_buffer);
                    think_buffer.clear();
                }
                in_think = false;
            } else {
                // Not a think tag, treat as regular content
                if in_think {
                    think_buffer.push_str(&tag);
                } else {
                    regular_buffer.push_str(&tag);
                }
            }
        } else {
            if in_think {
                think_buffer.push(ch);
            } else {
                regular_buffer.push(ch);
            }
        }
    }

    // Flush remaining buffers
    if !think_buffer.is_empty() {
        if !thinking.is_empty() {
            thinking.push('\n');
        }
        thinking.push_str(&think_buffer);
    }
    if !regular_buffer.is_empty() {
        regular.push_str(&regular_buffer);
    }

    (thinking, regular)
}

/// Emergency truncate: remove oldest non-system messages until estimated tokens
/// are below `target_tokens`. Preserves system prompt and most recent messages.
pub(crate) fn emergency_truncate_messages(
    messages: &mut Vec<crate::providers::Message>,
    target_tokens: usize,
) {
    loop {
        let estimated = crate::memory::compression::estimate_tokens(messages);
        if estimated <= target_tokens || messages.len() <= 2 {
            break;
        }
        // Find oldest non-system message to remove
        let remove_idx = messages
            .iter()
            .enumerate()
            .skip(1) // Never remove index 0 (system prompt)
            .find(|(_, m)| m.role != "system")
            .map(|(i, _)| i);
        if let Some(idx) = remove_idx {
            messages.remove(idx);
        } else {
            break;
        }
    }
}

