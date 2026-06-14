use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use unicode_width::UnicodeWidthChar;

use super::ascii_art;
use super::bookmarks;
use super::command_palette;
use super::image_display;
use super::syntax_highlight;
use super::{App, AppMode};
use super::{
    accent_style, bg_style, border_style, error_style, focused_border_style, highlight_style,
    muted_style, selection_style, shark_style, text_style, title_style, tool_style,
};
use crate::tools::get_tools;
use crate::tui::theme::current_theme;

pub(crate) fn draw_ui(f: &mut Frame, app: &mut App) {
    if app.mode == AppMode::Splash {
        draw_splash_screen(f);
        return;
    }

    let size = f.area();

    let main_layout = if app.sidebar_expanded {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
            .split(size)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(0), Constraint::Percentage(100)])
            .split(size)
    };

    if app.sidebar_expanded {
        draw_sidebar(f, app, main_layout[0]);
    }

    let chat_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(input_bar_height(app, main_layout[1].width)),
        ])
        .split(main_layout[1]);

    app.chat_area_rect = Some(chat_layout[0]);
    draw_chat_area(f, app, chat_layout[0]);
    draw_input_bar(f, app, chat_layout[1]);

    if app.mode == AppMode::ToolApproval {
        draw_tool_approval_popup(f, app);
    }

    if app.mode == AppMode::DiffPreview {
        draw_diff_preview_popup(f, app);
    }

    if app.show_comparison {
        draw_comparison_overlay(f, app);
    }

    // Command palette overlay (drawn last so it's on top)
    command_palette::draw_command_palette(f, &app.command_palette, size);

    // Bookmark manager overlay
    bookmarks::draw_bookmark_manager(f, &app.bookmark_manager, size);
}

pub(crate) fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    // Single outer border for the whole sidebar — no nested boxes
    let sidebar_block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style())
        .style(bg_style());

    let inner = sidebar_block.inner(area);
    f.render_widget(sidebar_block, area);

    // Compact vertical layout: header → session → shortcuts → tools/skills → perf
    let sidebar_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Compact logo + tagline
            Constraint::Length(9), // Session info (7 lines + padding)
            Constraint::Length(9), // Shortcuts (7 lines + padding)
            Constraint::Length(8), // Tools/Skills (up to 6 with tab header)
            Constraint::Min(3),    // Performance (flexible)
        ])
        .split(inner);

    // Compact header: harness name + version (hardcoded, separate from agent identity)
    let mut header_lines = vec![Line::from(vec![
        Span::styled("🦞 ", shark_style()),
        Span::styled("openshark", highlight_style()),
        Span::styled(format!(" v{}", crate::VERSION), muted_style()),
    ])];
    if !app.config.agent.tagline.is_empty() {
        header_lines.push(Line::from(vec![Span::styled(
            app.config.agent.tagline.clone(),
            muted_style(),
        )]));
    }
    let header = Paragraph::new(Text::from(header_lines))
        .alignment(Alignment::Center)
        .style(bg_style());
    f.render_widget(header, sidebar_layout[0]);

    // Session info — no inner border, just styled text with section header
    let ctx_used = app.context_used();
    let ctx_pct = (ctx_used * 100)
        .checked_div(app.model_context_length)
        .unwrap_or(0)
        .min(100);
    let ctx_color = if ctx_pct > 80 {
        error_style()
    } else if ctx_pct > 50 {
        accent_style()
    } else {
        text_style()
    };

    let session_info = vec![
        Line::from(vec![
            Span::styled("Session  ", muted_style()),
            Span::styled(
                &app.session_id[..8.min(app.session_id.len())],
                highlight_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Model    ", muted_style()),
            Span::styled(&app.model, accent_style()),
        ]),
        Line::from(vec![
            Span::styled("Max Ctx  ", muted_style()),
            Span::styled(format!("{}", app.model_context_length), text_style()),
        ]),
        Line::from(vec![
            Span::styled("Ctx Used ", muted_style()),
            Span::styled(format!("{} ({}%)", ctx_used, ctx_pct), ctx_color),
        ]),
        Line::from(vec![
            Span::styled("Duration ", muted_style()),
            Span::styled(app.session_duration(), text_style()),
        ]),
        Line::from(vec![
            Span::styled("Tokens   ", muted_style()),
            Span::styled(app.tokens_used.to_string(), text_style()),
        ]),
        Line::from(vec![
            Span::styled("Tools    ", muted_style()),
            Span::styled(app.tool_calls_count.to_string(), text_style()),
        ]),
    ];
    let session = Paragraph::new(Text::from(session_info))
        .block(
            Block::default()
                .title(" Session ")
                .title_style(title_style())
                .borders(Borders::TOP)
                .border_style(border_style()),
        )
        .style(bg_style());
    f.render_widget(session, sidebar_layout[1]);

    // Shortcuts — clean two-column layout
    let shortcuts = vec![
        Line::from(vec![
            Span::styled("Ctrl+C×2", accent_style()),
            Span::styled(" Quit", muted_style()),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+L  ", accent_style()),
            Span::styled("Clear chat", muted_style()),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+B  ", accent_style()),
            Span::styled("Toggle sidebar", muted_style()),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+P  ", accent_style()),
            Span::styled("Model selector", muted_style()),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+A  ", accent_style()),
            Span::styled("Autonomous mode", muted_style()),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+T  ", accent_style()),
            Span::styled("Cycle theme", muted_style()),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+S  ", accent_style()),
            Span::styled("Tools/Skills", muted_style()),
        ]),
        Line::from(vec![
            Span::styled("↑/↓     ", accent_style()),
            Span::styled("Scroll", muted_style()),
        ]),
        Line::from(vec![
            Span::styled("PgUp/Dn ", accent_style()),
            Span::styled("Fast scroll", muted_style()),
        ]),
    ];
    let shortcuts_para = Paragraph::new(Text::from(shortcuts))
        .block(
            Block::default()
                .title(" Shortcuts ")
                .title_style(title_style())
                .borders(Borders::TOP)
                .border_style(border_style()),
        )
        .style(bg_style());
    f.render_widget(shortcuts_para, sidebar_layout[2]);

    // Tools / Skills — tabbed view with scrolling
    let (tab_title, tab_items): (String, Vec<Line>) = if app.sidebar_tab == 0 {
        let all_tools = get_tools();
        let tools: Vec<Line> = all_tools
            .iter()
            .skip(app.sidebar_scroll)
            .take(6)
            .map(|t| {
                let desc = t.description();
                let desc_short = &desc[..desc.len().min(22)];
                Line::from(vec![
                    Span::styled(format!("{:<10}", t.name()), tool_style()),
                    Span::styled(desc_short.to_string(), muted_style()),
                ])
            })
            .collect();
        (format!(" Tools [{}] ", all_tools.len()), tools)
    } else if app.sidebar_tab == 1 {
        let skills: Vec<Line> = app
            .skill_registry
            .as_ref()
            .map(|reg| {
                reg.all_skills()
                    .iter()
                    .skip(app.sidebar_scroll)
                    .take(6)
                    .map(|skill| {
                        let desc = &skill.description;
                        let desc_short = &desc[..desc.len().min(22)];
                        Line::from(vec![
                            Span::styled(format!("{:<10}", &skill.name), tool_style()),
                            Span::styled(desc_short.to_string(), muted_style()),
                        ])
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                vec![
                    Line::from(vec![Span::styled("No skills loaded", muted_style())]),
                    Line::from(vec![Span::styled(
                        "Add .md files to ~/.config/openshark/skills/",
                        muted_style(),
                    )]),
                ]
            });
        let count = app
            .skill_registry
            .as_ref()
            .map(|r| r.all_skills().len())
            .unwrap_or(0);
        (format!(" Skills [{}] ", count), skills)
    } else if app.sidebar_tab == 4 {
        // Files tab — project file tree
        let file_lines: Vec<Line> = if app.file_tree.is_empty() {
            vec![Line::from(vec![Span::styled(
                "No files scanned",
                muted_style(),
            )])]
        } else {
            app.file_tree
                .iter()
                .enumerate()
                .skip(app.sidebar_scroll)
                .take(20)
                .map(|(i, entry)| {
                    let is_selected = i == app.file_tree_selected;
                    let style = if is_selected {
                        highlight_style().add_modifier(Modifier::BOLD)
                    } else {
                        text_style()
                    };
                    Line::from(vec![Span::styled(entry.clone(), style)])
                })
                .collect()
        };
        (" Files ".to_string(), file_lines)
    } else if app.sidebar_tab == 2 {
        // Swarm tab
        let swarm_lines: Vec<Line> = if !app.swarm_agents.is_empty() {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Status: ", muted_style()),
                    Span::styled(
                        if app.swarm_running {
                            "🟢 Running"
                        } else {
                            "⏹ Idle"
                        },
                        text_style(),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Agents: ", muted_style()),
                    Span::styled(format!("{}", app.swarm_agents.len()), text_style()),
                ]),
                Line::from(vec![]),
            ];
            for agent in app.swarm_agents.iter().skip(app.sidebar_scroll).take(6) {
                let status_icon = match agent.status {
                    crate::swarm::AgentStatus::Idle => "⏸",
                    crate::swarm::AgentStatus::Working { .. } => "🟡",
                    crate::swarm::AgentStatus::Reviewing { .. } => "👁",
                    crate::swarm::AgentStatus::WaitingForConsensus { .. } => "⏳",
                    crate::swarm::AgentStatus::Error { .. } => "❌",
                    crate::swarm::AgentStatus::Completed { .. } => "✅",
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", status_icon), text_style()),
                    Span::styled(format!("{:<10}", agent.name), tool_style()),
                    Span::styled(format!("cycles:{}", agent.cycles_completed), muted_style()),
                ]));
            }
            lines
        } else {
            vec![
                Line::from(vec![Span::styled("No swarm active", muted_style())]),
                Line::from(vec![Span::styled("Ctrl+W to activate", muted_style())]),
                Line::from(vec![]),
                Line::from(vec![Span::styled("Commands:", muted_style())]),
                Line::from(vec![Span::styled("/swarm init <prompt>", accent_style())]),
                Line::from(vec![Span::styled("/swarm start", accent_style())]),
                Line::from(vec![Span::styled("/swarm stop", accent_style())]),
                Line::from(vec![Span::styled("/swarm status", accent_style())]),
            ]
        };
        (" Swarm ".to_string(), swarm_lines)
    } else {
        // Inspector tab — per-agent raw output
        let inspector_lines: Vec<Line> = if !app.agent_streams.is_empty() {
            let mut lines = vec![
                Line::from(vec![Span::styled("Agent Inspector", title_style())]),
                Line::from(vec![]),
            ];
            for (_, state) in app.agent_streams.iter().skip(app.sidebar_scroll).take(8) {
                let role_color = match state.role.as_str() {
                    "Architect" => current_theme().accent,
                    "Implementer" => current_theme().success,
                    "Reviewer" => current_theme().highlight,
                    "Tester" => current_theme().error,
                    _ => current_theme().accent,
                };
                let role_style = Style::default().fg(role_color).add_modifier(Modifier::BOLD);
                let status = if state.is_streaming {
                    "🟡 streaming"
                } else {
                    "⏹ done"
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("🐝 {} ", state.agent_name), role_style),
                    Span::styled(format!("({}) {}", state.role, status), muted_style()),
                ]));
                // Show truncated content preview with code detection
                let preview = &state.content[..state.content.len().min(120)];
                let has_code =
                    preview.contains("```") || preview.contains("fn ") || preview.contains("def ");
                if has_code {
                    lines.push(Line::from(vec![
                        Span::styled("  📄 ".to_string(), muted_style()),
                        Span::styled(preview.replace('\n', " "), muted_style()),
                    ]));
                } else {
                    lines.push(Line::from(vec![Span::styled(
                        format!("  {}", preview.replace('\n', " ")),
                        muted_style(),
                    )]));
                }
                if !state.tool_results.is_empty() {
                    let expanded = app.agent_tool_expanded.contains(&state.agent_id);
                    let expand_icon = if expanded { "▼" } else { "▶" };
                    lines.push(Line::from(vec![Span::styled(
                        format!("  {} 🔧 {} tools", expand_icon, state.tool_results.len()),
                        tool_style(),
                    )]));
                    if expanded {
                        for (tool_name, result, success) in &state.tool_results {
                            let icon = if *success { "✅" } else { "❌" };
                            lines.push(Line::from(vec![Span::styled(
                                format!("    {} {}", icon, tool_name),
                                muted_style(),
                            )]));
                            for res_line in result.lines().take(4) {
                                lines.push(Line::from(vec![Span::styled(
                                    format!(
                                        "      {}",
                                        res_line.chars().take(50).collect::<String>()
                                    ),
                                    muted_style(),
                                )]));
                            }
                        }
                    }
                }
                lines.push(Line::from(vec![]));
            }
            lines
        } else {
            vec![
                Line::from(vec![Span::styled("No agent data", muted_style())]),
                Line::from(vec![Span::styled(
                    "Start a swarm to inspect agents",
                    muted_style(),
                )]),
            ]
        };
        (" Inspector ".to_string(), inspector_lines)
    };

    let tools_para = Paragraph::new(Text::from(tab_items))
        .block(
            Block::default()
                .title(tab_title)
                .title_style(title_style())
                .borders(Borders::TOP)
                .border_style(border_style()),
        )
        .style(bg_style());
    f.render_widget(tools_para, sidebar_layout[3]);

    // Performance — per-session metrics (streaming) or swarm stats
    let perf_lines = if app.session_perf.requests > 0 {
        vec![
            Line::from(vec![
                Span::styled("First token: ", muted_style()),
                Span::styled(
                    format!("{}ms", app.session_perf.avg_first_token()),
                    text_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Total latency: ", muted_style()),
                Span::styled(
                    format!("{}ms", app.session_perf.avg_total_latency()),
                    text_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Tool exec: ", muted_style()),
                Span::styled(
                    format!("{}ms", app.session_perf.avg_tool_exec()),
                    text_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Requests: ", muted_style()),
                Span::styled(app.session_perf.requests.to_string(), text_style()),
            ]),
        ]
    } else if app.swarm_running {
        // Show swarm stats when no streaming perf data
        let active = app
            .swarm_agents
            .iter()
            .filter(|a| matches!(a.status, crate::swarm::AgentStatus::Working { .. }))
            .count();
        let completed = app
            .swarm_agents
            .iter()
            .filter(|a| matches!(a.status, crate::swarm::AgentStatus::Completed { .. }))
            .count();
        let errors = app
            .swarm_agents
            .iter()
            .filter(|a| matches!(a.status, crate::swarm::AgentStatus::Error { .. }))
            .count();
        vec![
            Line::from(vec![Span::styled("Swarm active", accent_style())]),
            Line::from(vec![
                Span::styled("Working: ", muted_style()),
                Span::styled(format!("{}", active), text_style()),
                Span::styled(" | Done: ", muted_style()),
                Span::styled(format!("{}", completed), text_style()),
                Span::styled(" | Err: ", muted_style()),
                Span::styled(
                    format!("{}", errors),
                    if errors > 0 {
                        error_style()
                    } else {
                        text_style()
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("Cycles: ", muted_style()),
                Span::styled(
                    format!(
                        "{}",
                        app.swarm_agents
                            .iter()
                            .map(|a| a.cycles_completed)
                            .sum::<usize>()
                    ),
                    text_style(),
                ),
            ]),
        ]
    } else {
        vec![
            Line::from(vec![Span::styled("No performance data yet", muted_style())]),
            Line::from(vec![Span::styled(
                "Start chatting to collect metrics",
                muted_style(),
            )]),
        ]
    };
    let perf = Paragraph::new(Text::from(perf_lines))
        .block(
            Block::default()
                .title(" Performance ")
                .title_style(title_style())
                .borders(Borders::TOP)
                .border_style(border_style()),
        )
        .style(bg_style());
    f.render_widget(perf, sidebar_layout[4]);
}

pub(crate) fn draw_chat_area(f: &mut Frame, app: &App, area: Rect) {
    let chat_block = Block::default()
        .title(" Chat ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(if app.focused_pane == 1 {
            focused_border_style()
        } else {
            border_style()
        })
        .style(bg_style());

    let inner = chat_block.inner(area);
    f.render_widget(chat_block, area);

    let visible_height = inner.height as usize;
    let visible = app.visible_messages(visible_height);

    let mut lines: Vec<Line> = Vec::new();

    for msg in visible {
        let user_name = if app.config.user_name.is_empty() {
            "user"
        } else {
            &app.config.user_name
        };
        let agent_name = &app.config.agent.display_name;

        let (role_style, content_style, prefix, display_role) = match msg.role.as_str() {
            "user" => (
                accent_style(),
                text_style(),
                "❯ ".to_string(),
                user_name.to_string(),
            ),
            "assistant" => {
                let agent_emoji = if app.config.agent.emoji.is_empty() {
                    "🦞"
                } else {
                    &app.config.agent.emoji
                };
                (
                    shark_style(),
                    text_style(),
                    format!("{} ", agent_emoji),
                    agent_name.to_string(),
                )
            }
            "system" => {
                // Use error styling for error messages
                let is_error = msg.content.contains("Error:")
                    || msg.content.contains("error:")
                    || msg.content.contains("Failed")
                    || msg.content.contains("failed");
                if is_error {
                    (
                        error_style(),
                        error_style(),
                        "⚠ ".to_string(),
                        "system".to_string(),
                    )
                } else {
                    (
                        muted_style(),
                        muted_style(),
                        "ℹ ".to_string(),
                        "system".to_string(),
                    )
                }
            }
            _ => (
                text_style(),
                text_style(),
                "  ".to_string(),
                msg.role.clone(),
            ),
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, role_style),
            Span::styled(display_role, role_style.add_modifier(Modifier::BOLD)),
        ]));

        // Image attachment indicator with rich metadata
        if let Some(ref images) = msg.images {
            for img in images {
                let info = image_display::extract_image_info(img);
                lines.push(Line::from(vec![Span::styled(
                    format!("  {}", info.format_indicator()),
                    muted_style().add_modifier(Modifier::ITALIC),
                )]));
                // Show ASCII placeholder for the image
                for placeholder_line in info.ascii_placeholder() {
                    lines.push(Line::from(vec![Span::styled(
                        placeholder_line,
                        muted_style(),
                    )]));
                }
            }
        }

        // Render content with syntax highlighting for assistant messages
        if msg.role == "assistant" {
            // Render assistant messages with syntax highlighting + inline markdown
            let highlighted = syntax_highlight::extract_and_highlight(&msg.content);
            for (is_code, block_lines) in highlighted {
                if is_code {
                    lines.push(Line::from(vec![Span::styled(
                        "┌─ code ──────────────────────────────",
                        muted_style(),
                    )]));
                    for hl_line in block_lines {
                        lines.push(hl_line);
                    }
                    lines.push(Line::from(vec![Span::styled(
                        "└─────────────────────────────────────",
                        muted_style(),
                    )]));
                } else {
                    for hl_line in block_lines {
                        // Apply inline markdown rendering to plain text blocks
                        let rendered = syntax_highlight::render_markdown_line(&hl_line.to_string());
                        lines.push(rendered);
                    }
                }
            }
        } else {
            for content_line in msg.content.lines() {
                // Welcome logo lines use purple for visibility against dark bg
                let line_style = if msg.role == "system" && content_line.contains('█') {
                    Style::default()
                        .fg(current_theme().border_unfocused)
                        .add_modifier(Modifier::BOLD)
                } else {
                    content_style
                };
                lines.push(Line::from(vec![Span::styled(content_line, line_style)]));
            }
        }

        // Multi-model response indicator on assistant messages
        if msg.role == "assistant" && !msg.multi_model_responses.is_empty() {
            let count = msg.multi_model_responses.len();
            lines.push(Line::from(vec![Span::styled(
                format!(
                    "📊 {} alternate response{} — Ctrl+V to compare",
                    count,
                    if count == 1 { "" } else { "s" }
                ),
                muted_style().add_modifier(Modifier::ITALIC),
            )]));
        }

        lines.push(Line::from(""));
    }

    // ── Swarm Agent Streaming ──────────────────────────────────────────────
    for (_, state) in app.agent_streams.iter() {
        if state.is_streaming || !state.content.is_empty() {
            let role_color = match state.role.as_str() {
                "Architect" => current_theme().accent,
                "Implementer" => current_theme().success,
                "Reviewer" => current_theme().highlight,
                "Tester" => current_theme().error,
                "DevOps" => current_theme().accent_secondary,
                "Security" => current_theme().error,
                "Documentation" => current_theme().muted,
                "Project Manager" => current_theme().title,
                _ => current_theme().accent,
            };
            let role_style = Style::default().fg(role_color).add_modifier(Modifier::BOLD);

            lines.push(Line::from(vec![
                Span::styled("🐝 ", role_style),
                Span::styled(format!("{} — {}", state.agent_name, state.role), role_style),
            ]));

            // Use syntax highlighting for code blocks in agent content
            let highlighted = syntax_highlight::extract_and_highlight(&state.content);
            for (is_code, block_lines) in highlighted {
                if is_code {
                    // Add a subtle code block border
                    lines.push(Line::from(vec![Span::styled(
                        "┌─ code ──────────────────────────────",
                        muted_style(),
                    )]));
                    for hl_line in block_lines {
                        lines.push(hl_line);
                    }
                    lines.push(Line::from(vec![Span::styled(
                        "└─────────────────────────────────────",
                        muted_style(),
                    )]));
                } else {
                    for hl_line in block_lines {
                        lines.push(hl_line);
                    }
                }
            }

            if state.is_streaming {
                lines.push(Line::from(vec![Span::styled("▌", role_style)]));
            }

            lines.push(Line::from(""));
        }
    }

    // ── Apply selection highlight ──────────────────────────────────────────
    if app.mouse_state.selecting
        && let (Some(start), Some(end)) = (
            app.mouse_state.selection_start,
            app.mouse_state.selection_end,
        )
    {
        let sel_top = start.1.min(end.1);
        let sel_bottom = start.1.max(end.1);
        // Convert absolute terminal rows to content-relative rows
        let content_top = inner.y;
        let rel_top = sel_top.saturating_sub(content_top);
        let rel_bottom = sel_bottom.saturating_sub(content_top);
        for (row_idx, line) in lines.iter_mut().enumerate() {
            let row = row_idx as u16;
            if row >= rel_top && row <= rel_bottom {
                *line = Line::from(
                    line.spans
                        .iter()
                        .map(|s| Span::styled(s.content.clone(), selection_style()))
                        .collect::<Vec<_>>(),
                );
            }
        }
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: true })
        .style(bg_style());

    f.render_widget(paragraph, inner);

    if app.messages.len() > visible_height {
        let mut scrollbar_state = ScrollbarState::new(app.messages.len())
            .position(app.scroll)
            .viewport_content_length(visible_height);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(accent_style())
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        f.render_stateful_widget(
            scrollbar,
            inner.inner(Margin {
                vertical: 0,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

pub(crate) fn draw_input_bar(f: &mut Frame, app: &App, area: Rect) {
    // Split into status line + input area
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    // ── Status line ─────────────────────────────────────────────────────────
    let mut status_spans = vec![];

    // YOLO mode indicator
    if app.yolo_mode {
        status_spans.push(Span::styled(
            "🤘YOLO ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }

    // Checkpoint depth
    let cp_depth = app.checkpoint_stack.undo_len();
    if cp_depth > 0 {
        status_spans.push(Span::styled(
            format!("💾{} ", cp_depth),
            Style::default().fg(Color::Cyan),
        ));
    }

    // Session cost
    let (total_tokens, total_cost) = crate::providers::get_session_usage();
    if total_tokens > 0 {
        status_spans.push(Span::styled(
            format!("💰${:.4} ", total_cost),
            Style::default().fg(Color::Yellow),
        ));
    }

    // Model
    status_spans.push(Span::styled(
        format!("🤖{} ", app.model),
        Style::default().fg(Color::Magenta),
    ));

    // Token count for this session
    if app.tokens_used > 0 {
        status_spans.push(Span::styled(
            format!("📊{}t ", app.tokens_used),
            Style::default().fg(Color::Green),
        ));
    }

    let status_line =
        Paragraph::new(Line::from(status_spans)).style(Style::default().bg(Color::Black));
    f.render_widget(status_line, layout[0]);

    // ── Input block ─────────────────────────────────────────────────────────
    let input_block = Block::default()
        .title(" Input ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(if app.focused_pane == 2 {
            focused_border_style()
        } else {
            border_style()
        })
        .style(bg_style());

    let inner = input_block.inner(layout[1]);
    f.render_widget(input_block, layout[1]);

    let input_text = if app.input.is_empty() {
        if app.mode == AppMode::ToolApproval {
            "Tool suggestion pending. Press 'y' to execute, 'n' to skip."
        } else if app.swarm_running {
            "🐝 Agents working..."
        } else {
            "Type a message or command..."
        }
    } else {
        &app.input
    };

    let style = if app.input.is_empty() {
        muted_style()
    } else {
        text_style()
    };

    // Add a visible prompt prefix
    let display_text = if app.input.is_empty() {
        input_text.to_string()
    } else {
        format!("❯ {}", app.input)
    };

    let paragraph = Paragraph::new(display_text.clone())
        .style(style)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, inner);

    // Always show cursor at the correct position
    let available_width = inner.width as usize;
    let cursor_offset = if app.input.is_empty() {
        0
    } else {
        "❯ ".len()
    }; // byte offset of prefix in display_text
    let (cursor_x, cursor_y) = compute_wrapped_cursor_position(
        &display_text,
        app.cursor_position + cursor_offset,
        available_width,
        inner.x,
        inner.y,
    );
    f.set_cursor_position((cursor_x, cursor_y));
}

/// Compute the visual height the input bar needs based on text length and wrap width.
pub(crate) fn input_bar_height(app: &App, area_width: u16) -> u16 {
    let available_width = area_width.saturating_sub(2).max(1) as usize; // minus borders
    let text = if app.input.is_empty() {
        // Placeholder text length
        "Type a message or command...".len()
    } else {
        app.input.len()
    };
    let explicit_lines = app.input.matches('\n').count() + 1;
    let wrapped_lines = text.div_ceil(available_width); // ceil division
    let lines = explicit_lines.max(wrapped_lines);
    let lines = lines.max(1);
    // Cap at 8 lines so it doesn't eat the whole chat area
    let capped = lines.min(8);
    // +3: +1 for status line, +2 for input block borders
    (capped as u16) + 3
}

/// Compute the actual screen (x, y) for the cursor given a text buffer,
/// a cursor byte position, and the available wrap width.
/// Simulates ratatui's wrap behavior to place the cursor correctly.
pub(crate) fn compute_wrapped_cursor_position(
    text: &str,
    cursor_pos: usize,
    wrap_width: usize,
    base_x: u16,
    base_y: u16,
) -> (u16, u16) {
    if text.is_empty() || cursor_pos == 0 {
        return (base_x, base_y);
    }

    let pos = cursor_pos.min(text.len());
    let safe_pos = text.ceil_char_boundary(pos);
    let before_cursor = &text[..safe_pos];

    // Simulate wrapping: walk through chars, tracking line breaks
    let mut line = 0usize;
    let mut col = 0usize;

    for ch in before_cursor.chars() {
        let ch_w = ch.width().unwrap_or(1);
        if ch == '\n' {
            line += 1;
            col = 0;
        } else if col + ch_w > wrap_width && wrap_width > 0 {
            // Wrap to next line
            line += 1;
            col = ch_w;
        } else {
            col += ch_w;
        }
    }

    (base_x + col as u16, base_y + line as u16)
}

pub(crate) fn draw_tool_approval_popup(f: &mut Frame, app: &App) {
    let area = f.area();
    let popup_area = centered_rect(60, 40, area);

    let clear = Clear;
    f.render_widget(clear, popup_area);

    let block = Block::default()
        .title(" Tool Approval ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(focused_border_style())
        .style(bg_style());

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    if let Some(ref suggestion) = app.pending_suggestion {
        let content = vec![
            Line::from(vec![
                Span::styled("Model suggests using: ", text_style()),
                Span::styled(&suggestion.tool_name, tool_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Arguments: ", muted_style()),
                Span::styled(&suggestion.args, text_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Confidence: ", muted_style()),
                Span::styled(
                    format!("{:.0}%", suggestion.confidence * 100.0),
                    highlight_style(),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", text_style()),
                Span::styled("y", accent_style()),
                Span::styled(" to execute or ", text_style()),
                Span::styled("n", error_style()),
                Span::styled(" to skip", text_style()),
            ]),
        ];

        let paragraph = Paragraph::new(Text::from(content)).style(bg_style());
        f.render_widget(paragraph, inner);
    }
}

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Draw the diff preview popup (80% x 70% popup with scrollable diff).
pub(crate) fn draw_diff_preview_popup(f: &mut Frame, app: &App) {
    let area = f.area();
    let popup_area = centered_rect(80, 70, area);

    let clear = Clear;
    f.render_widget(clear, popup_area);

    let block = Block::default()
        .title(" Diff Preview ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(focused_border_style())
        .style(bg_style());

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let mut lines = vec![
        Line::from(vec![Span::styled(
            "📝 Proposed file edit:",
            highlight_style(),
        )]),
        Line::from(""),
    ];

    if let Some(ref diff) = app.pending_diff {
        for diff_line in diff.lines().skip(app.diff_scroll) {
            let styled_line = if diff_line.starts_with('+') {
                Line::from(vec![Span::styled(
                    diff_line,
                    Style::default().fg(ratatui::style::Color::Green),
                )])
            } else if diff_line.starts_with('-') {
                Line::from(vec![Span::styled(
                    diff_line,
                    Style::default().fg(ratatui::style::Color::Red),
                )])
            } else if diff_line.starts_with("@@") {
                Line::from(vec![Span::styled(
                    diff_line,
                    Style::default().fg(ratatui::style::Color::Cyan),
                )])
            } else {
                Line::from(vec![Span::styled(diff_line, text_style())])
            };
            lines.push(styled_line);
        }
    } else {
        lines.push(Line::from("No diff available."));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Press ", text_style()),
        Span::styled("y", accent_style()),
        Span::styled(" to approve diff, ", text_style()),
        Span::styled("n", error_style()),
        Span::styled(" to skip", text_style()),
    ]));
    lines.push(Line::from(vec![Span::styled(
        "↑/↓ or PgUp/PgDn to scroll",
        muted_style(),
    )]));

    let paragraph = Paragraph::new(Text::from(lines))
        .style(bg_style())
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
}

/// Draw the multi-model comparison overlay (90% × 85% popup).
pub(crate) fn draw_comparison_overlay(f: &mut Frame, app: &App) {
    let area = f.area();
    let popup_area = centered_rect(90, 85, area);

    let clear = Clear;
    f.render_widget(clear, popup_area);

    let block = Block::default()
        .title(" 📊 Multi-Model Comparison ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(focused_border_style())
        .style(bg_style());

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    // Find the assistant message with the most secondary responses
    let target_msg = app
        .messages
        .iter()
        .filter(|m| m.role == "assistant")
        .max_by_key(|m| m.multi_model_responses.len());

    let mut lines: Vec<Line> = Vec::new();

    if let Some(msg) = target_msg {
        if msg.multi_model_responses.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "No alternate responses available yet.",
                muted_style(),
            )]));
            lines.push(Line::from(vec![Span::styled(
                "Enable multi-model mode with /multi and send a message.",
                muted_style(),
            )]));
        } else {
            // Header — primary model
            lines.push(Line::from(vec![
                Span::styled("Primary: ", muted_style()),
                Span::styled(&app.model, highlight_style().add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(""));

            // Primary response (truncated to first 10 lines for header view)
            for line in msg.content.lines().take(10) {
                lines.push(Line::from(vec![Span::styled(line, text_style())]));
            }
            if msg.content.lines().count() > 10 {
                lines.push(Line::from(vec![Span::styled("...", muted_style())]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "─".repeat(inner.width as usize),
                border_style(),
            )]));
            lines.push(Line::from(""));

            // Secondary responses
            for (idx, sec) in msg.multi_model_responses.iter().enumerate() {
                let is_selected = idx == app.comparison_selected;
                let marker = if is_selected { "▶ " } else { "  " };
                let name_style = if is_selected {
                    highlight_style().add_modifier(Modifier::BOLD)
                } else {
                    accent_style()
                };

                lines.push(Line::from(vec![
                    Span::styled(
                        marker,
                        if is_selected {
                            highlight_style()
                        } else {
                            muted_style()
                        },
                    ),
                    Span::styled(&sec.model_name, name_style),
                    Span::styled("  |  ", muted_style()),
                    Span::styled(format!("{}ms", sec.latency_ms), text_style()),
                    Span::styled("  |  ", muted_style()),
                    Span::styled(format!("{} tokens", sec.tokens), text_style()),
                ]));

                // Show content for selected response, preview for others
                let preview_lines = if is_selected { 20 } else { 3 };
                for line in sec.content.lines().take(preview_lines) {
                    lines.push(Line::from(vec![
                        Span::styled("    ", muted_style()),
                        Span::styled(
                            line,
                            if is_selected {
                                text_style()
                            } else {
                                muted_style()
                            },
                        ),
                    ]));
                }
                if sec.content.lines().count() > preview_lines {
                    lines.push(Line::from(vec![Span::styled("    ...", muted_style())]));
                }
                lines.push(Line::from(""));
            }

            lines.push(Line::from(vec![Span::styled(
                "↑/↓ to navigate • Ctrl+V to close",
                muted_style(),
            )]));
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            "No assistant messages found.",
            muted_style(),
        )]));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: true })
        .style(bg_style());
    f.render_widget(paragraph, inner);
}

#[allow(dead_code)]
pub(crate) fn draw_chat_header(_f: &mut Frame, _area: Rect) {
    // Removed — welcome message is now in chat history
}

/// Full-screen splash screen — DOS title screen aesthetic.
/// Shows the OpenShark wordmark, fin, waves, and tagline.
/// Press any key to dismiss and enter the chat TUI.
pub(crate) fn draw_splash_screen(f: &mut Frame) {
    let area = f.area();

    // Solid background fill
    let bg = Block::default().style(bg_style());
    f.render_widget(bg, area);

    let banner_text = ascii_art::welcome_banner(area.width as usize);
    let banner_lines: Vec<Line> = banner_text
        .lines()
        .map(|line| {
            // Colorize different parts of the banner
            if line.contains('▪') {
                // Wordmark / fin — purple/pink
                Line::from(vec![Span::styled(
                    line,
                    Style::default()
                        .fg(current_theme().accent_secondary)
                        .add_modifier(Modifier::BOLD),
                )])
            } else if line.contains("Fast. Precise. Hungry.") {
                // Tagline — hot pink
                Line::from(vec![Span::styled(
                    line,
                    Style::default()
                        .fg(current_theme().accent_secondary)
                        .add_modifier(Modifier::BOLD),
                )])
            } else if line.contains('≈') {
                // Waves — cyan/blue tones
                Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(current_theme().accent),
                )])
            } else {
                Line::from(vec![Span::styled(line, text_style())])
            }
        })
        .collect();

    let banner = Paragraph::new(Text::from(banner_lines)).style(bg_style());

    // Center vertically: calculate offset to place banner in middle of screen
    let banner_height = banner_text.lines().count() as u16;
    let vertical_offset = (area.height.saturating_sub(banner_height)) / 2;
    let banner_area = Rect {
        x: area.x,
        y: area.y + vertical_offset,
        width: area.width,
        height: banner_height.min(area.height),
    };

    f.render_widget(banner, banner_area);

    // "Press any key" prompt at bottom
    let prompt = Paragraph::new(Text::from(vec![Line::from(vec![Span::styled(
        "Press any key to start",
        Style::default()
            .fg(current_theme().muted)
            .add_modifier(Modifier::ITALIC),
    )])]))
    .alignment(Alignment::Center)
    .style(bg_style());

    let prompt_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(3),
        width: area.width,
        height: 1,
    };
    f.render_widget(prompt, prompt_area);
}
