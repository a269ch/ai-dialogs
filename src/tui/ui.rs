use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Row, Table};
use unicode_width::UnicodeWidthStr;

use crate::cleaner::format_bytes;
use crate::tui::app::{App, case_insensitive_match_ranges, line_to_string, slice_spans};

pub fn draw_app(f: &mut Frame, app: &mut App) {
    let size = f.area();
    if size.height < 8 || size.width < 40 {
        let warn = Paragraph::new("Terminal window is too small! Please resize the window.")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        f.render_widget(warn, size);
        return;
    }

    if app.viewer.is_some() {
        draw_viewer(f, app);
    } else {
        draw_main(f, app);
    }

    if app.show_help {
        draw_help_popup(f);
    }

    if let Some(ref dialog) = app.confirm {
        draw_confirm_popup(f, dialog.message.as_str(), dialog.is_destructive);
    }

    if let Some(ref dialog) = app.transfer_dialog {
        draw_transfer_popup(f, dialog);
    }
}

fn draw_main(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_list(f, app, chunks[1]);
    draw_status_bar(f, app, chunks[2]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize;
    let is_trash = app.is_trash_view();

    let (title, stats, bar_style) = if is_trash {
        (
            " 🗑️  AI Dialogue Manager [TRASH / DELETED] ",
            format!(" In trash: {} ", app.store.trash_items.len()),
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        let filtered_count = app.filtered_items().len();
        let total_count = app.store.items.len() + app.external_items.len();
        (
            " 🤖 Universal AI Dialogue Manager (ai-dialogs) ",
            format!(
                " Total: {} | Filtered: {} | Trash: {} ",
                total_count,
                filtered_count,
                app.store.trash_items.len()
            ),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    };

    let title_w = UnicodeWidthStr::width(title);
    let stats_w = UnicodeWidthStr::width(stats.as_str());
    let pad = width.saturating_sub(title_w + stats_w);

    let top_line = Line::from(vec![
        Span::styled(title, bar_style),
        Span::styled(" ".repeat(pad), bar_style),
        Span::styled(stats, bar_style),
    ]);

    let mut filter_line_spans = vec![
        Span::styled(" Mode: [", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.filter_mode.as_str(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("] (Tab) | Agent: [", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.provider_name(),
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("] (P) | Sort: [", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.sort_mode.as_str(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("] (S)", Style::default().fg(Color::DarkGray)),
    ];

    if !app.search_query.is_empty() {
        filter_line_spans.push(Span::styled(
            " | Search: \"",
            Style::default().fg(Color::DarkGray),
        ));
        filter_line_spans.push(Span::styled(
            app.search_query.clone(),
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ));
        filter_line_spans.push(Span::styled(
            "\" (Esc to reset)",
            Style::default().fg(Color::DarkGray),
        ));
    }

    let header_widget = Paragraph::new(vec![top_line, Line::from(filter_line_spans)]);
    f.render_widget(header_widget, area);
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let items = app.filtered_items();
    let is_trash = app.is_trash_view();

    let header_titles: Vec<&str> = if is_trash {
        vec![
            "[✓]",
            "#",
            "ID",
            "DELETED / TIME",
            "MSGS",
            "SIZE",
            "TOPIC (TRASH)",
        ]
    } else {
        vec![
            "[✓]",
            "#",
            "AGENT",
            "ID",
            "DATE / TIME",
            "MSGS",
            "SIZE",
            "TOPIC / FIRST PROMPT",
        ]
    };

    let header_cells = header_titles.into_iter().map(|h| {
        Span::styled(
            h,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
    });
    let header_row = Row::new(header_cells).height(1);

    if items.is_empty() {
        let empty_msg = if is_trash {
            "Trash is empty (no deleted dialogues). Press [Tab] to return."
        } else {
            "No dialogues found. Adjust filter or search query."
        };
        let empty_p = Paragraph::new(format!("\n   {}", empty_msg))
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty_p, area);
        return;
    }

    let visible_rows = area.height.saturating_sub(1) as usize;
    if app.selected_idx >= items.len() {
        app.selected_idx = items.len().saturating_sub(1);
    }

    let mut start_idx = 0;
    if app.selected_idx >= visible_rows {
        start_idx = app.selected_idx - visible_rows + 1;
    }
    let end_idx = (start_idx + visible_rows).min(items.len());

    let mut rows: Vec<Row> = Vec::new();

    for (pos, item) in items[start_idx..end_idx].iter().enumerate() {
        let global_idx = start_idx + pos;
        let is_selected = global_idx == app.selected_idx;

        let mark_str = if item.is_marked { "[✓]" } else { "[ ]" };
        let num_str = format!("{:02}", global_idx + 1);
        let id_short: String = item.id.chars().take(8).collect();
        let date_str = if is_trash {
            item.deleted_date_str()
        } else {
            item.date_str()
        };
        let msgs_str = format!("{:>2}u/{:<2}m", item.user_msgs_count, item.model_msgs_count);
        let size_str = format!("{:>7}", format_bytes(item.size_bytes));
        let topic_str = item.topic.replace('\n', " ");

        let mut row_style = Style::default();
        if is_selected {
            row_style = Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD);
        } else if item.is_marked {
            row_style = Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD);
        } else if is_trash {
            row_style = Style::default().fg(Color::Red);
        } else if item.is_subagent {
            row_style = Style::default().fg(Color::Yellow);
        } else if item.is_empty {
            row_style = Style::default().fg(Color::DarkGray);
        }

        let agent_tag = item.agent.short_tag();
        let agent_style = if is_selected {
            row_style
        } else {
            match item.agent {
                crate::canonical::AgentKind::Antigravity => Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                crate::canonical::AgentKind::Claude => Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
                crate::canonical::AgentKind::Codex => Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
                crate::canonical::AgentKind::Grok => Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
                crate::canonical::AgentKind::Universal => Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            }
        };

        let cells = if is_trash {
            vec![
                Span::styled(mark_str, row_style),
                Span::styled(num_str, row_style),
                Span::styled(id_short, row_style),
                Span::styled(date_str, row_style),
                Span::styled(msgs_str, row_style),
                Span::styled(size_str, row_style),
                Span::styled(topic_str, row_style),
            ]
        } else {
            vec![
                Span::styled(mark_str, row_style),
                Span::styled(num_str, row_style),
                Span::styled(agent_tag, agent_style),
                Span::styled(id_short, row_style),
                Span::styled(date_str, row_style),
                Span::styled(msgs_str, row_style),
                Span::styled(size_str, row_style),
                Span::styled(topic_str, row_style),
            ]
        };

        rows.push(Row::new(cells).style(row_style));
    }

    let widths = if is_trash {
        vec![
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Length(18),
            Constraint::Length(9),
            Constraint::Length(10),
            Constraint::Min(20),
        ]
    } else {
        vec![
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(18),
            Constraint::Length(9),
            Constraint::Length(10),
            Constraint::Min(20),
        ]
    };

    let table = Table::new(rows, widths).header(header_row);

    f.render_widget(table, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize;

    if app.is_searching {
        let prompt = format!(" Search keyword or ID: {}█", app.search_input);
        let p = Paragraph::new(prompt).style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        f.render_widget(p, area);
        return;
    }

    let status_left = format!(" {} ", app.status_msg);
    let keys = if app.is_trash_view() {
        if width >= 105 {
            " [Enter] Read | [U] Restore | [D] Delete permanently | [C] Empty trash | [Tab] Active | [?] Help | [Q] Quit "
        } else if width >= 80 {
            " [Enter] Read | [U] Restore | [D] Delete | [Tab] Active | [?] Help | [Q] Quit "
        } else {
            " [Enter] Read | [U] Restore | [D] Delete | [?] Help | [Q] Quit "
        }
    } else if width >= 120 {
        " [Enter] Read | [O] Resume | [Space] Mark | [M] Migrate | [P] Agent | [S] Sort | [D] Trash | [/] Search | [?] Help | [Q] Quit "
    } else if width >= 95 {
        " [Enter] Read | [O] Resume | [M] Migrate | [P] Agent | [D] Trash | [/] Search | [?] Help | [Q] Quit "
    } else {
        " [Enter] Read | [O] Resume | [M] Migrate | [?] Help | [Q] Quit "
    };

    let left_w = UnicodeWidthStr::width(status_left.as_str());
    let keys_w = UnicodeWidthStr::width(keys);
    let pad = width.saturating_sub(left_w + keys_w);

    let status_line = Line::from(vec![
        Span::styled(
            status_left,
            Style::default().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(
            " ".repeat(pad),
            Style::default().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(
            keys,
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let p = Paragraph::new(status_line);
    f.render_widget(p, area);
}

fn draw_viewer(f: &mut Frame, app: &mut App) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(size);

    let Some(ref mut viewer) = app.viewer else {
        return;
    };

    let content_w = chunks[1].width as usize;
    let content_h = chunks[1].height as usize;
    viewer.set_viewport(content_w, content_h);

    let is_trash = viewer.item.is_in_trash;
    let total_lines = viewer.rendered_lines.len();
    let max_y = viewer.max_scroll_y(content_h);
    let pct = (viewer.scroll_y * 100).checked_div(max_y).unwrap_or(100);

    let trash_tag = if is_trash { " [IN TRASH]" } else { "" };
    let search_info = if !viewer.search_kw.is_empty() {
        format!(" | Search: \"{}\"", viewer.search_kw)
    } else {
        String::new()
    };
    let short_id: String = viewer.item.id.chars().take(8).collect();
    let head_left = format!(
        " 📖 {}{} | Page {}/{} ({}%) | X:{}{}",
        short_id,
        trash_tag,
        viewer.scroll_y + 1,
        total_lines,
        pct,
        viewer.scroll_x,
        search_info
    );

    let head_right = if is_trash {
        " [↑/↓/←/→] Scroll | [U] Restore | [D] Delete permanently | [Q] Back "
    } else {
        " [↑/↓/←/→] Scroll | [/] Search | [u/m] Jump | [E] Export | [Q] Back "
    };

    let left_w = UnicodeWidthStr::width(head_left.as_str());
    let right_w = UnicodeWidthStr::width(head_right);
    let pad = (size.width as usize).saturating_sub(left_w + right_w);

    let top_style = if is_trash {
        Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    };

    let head_line = Line::from(vec![
        Span::styled(head_left, top_style),
        Span::styled(" ".repeat(pad), top_style),
        Span::styled(head_right, top_style),
    ]);
    f.render_widget(Paragraph::new(head_line), chunks[0]);

    let start_y = viewer.scroll_y;
    let end_y = (start_y + content_h).min(total_lines);

    let mut visible_lines: Vec<Line> = Vec::new();

    for line_idx in start_y..end_y {
        let base_line = &viewer.rendered_lines[line_idx];

        let styled_spans = if !viewer.search_kw.is_empty() {
            highlight_search_in_line(base_line, &viewer.search_kw)
        } else {
            base_line.spans.clone()
        };

        let sliced = slice_spans(&styled_spans, viewer.scroll_x, content_w);
        visible_lines.push(Line::from(sliced));
    }

    let body_widget = Paragraph::new(visible_lines);
    f.render_widget(body_widget, chunks[1]);

    if viewer.is_searching {
        let prompt = format!(" Search text: {}█", viewer.search_input);
        let p = Paragraph::new(prompt).style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        f.render_widget(p, chunks[2]);
    } else {
        let log_name = viewer
            .item
            .transcript_path
            .as_ref()
            .or(Some(&viewer.item.db_path))
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("N/A");

        let topic_short: String = viewer.item.topic.chars().take(45).collect();
        let foot_text = format!(" Dialogue: {} | Log: {} ", topic_short, log_name);
        let foot_right = " [O] Resume | [Q / Esc] Back ";
        let left_w = UnicodeWidthStr::width(foot_text.as_str());
        let right_w = UnicodeWidthStr::width(foot_right);
        let pad = (size.width as usize).saturating_sub(left_w + right_w);

        let foot_line = Line::from(vec![
            Span::styled(
                foot_text,
                Style::default().fg(Color::Black).bg(Color::White),
            ),
            Span::styled(
                " ".repeat(pad),
                Style::default().fg(Color::Black).bg(Color::White),
            ),
            Span::styled(
                foot_right,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        f.render_widget(Paragraph::new(foot_line), chunks[2]);
    }
}

fn highlight_search_in_line(line: &Line, search_kw: &str) -> Vec<Span<'static>> {
    let full_text = line_to_string(line);
    let matches = case_insensitive_match_ranges(&full_text, search_kw);
    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let mut new_spans = Vec::new();
    let mut span_start = 0;
    for span in &line.spans {
        let content = span.content.as_ref();
        let span_end = span_start + content.len();
        let mut last = 0;
        for matched in &matches {
            let start = matched.start.max(span_start);
            let end = matched.end.min(span_end);
            if start >= end {
                continue;
            }
            let start = start - span_start;
            let end = end - span_start;
            if start > last {
                new_spans.push(Span::styled(content[last..start].to_string(), span.style));
            }
            new_spans.push(Span::styled(
                content[start..end].to_string(),
                span.style.patch(highlight),
            ));
            last = end;
        }
        if last < content.len() {
            new_spans.push(Span::styled(content[last..].to_string(), span.style));
        }
        span_start = span_end;
    }

    new_spans
}

fn draw_confirm_popup(f: &mut Frame, message: &str, is_destructive: bool) {
    let area = centered_rect_fixed(72, 7, f.area());
    f.render_widget(Clear, area);

    let border_color = if is_destructive {
        Color::Red
    } else {
        Color::Yellow
    };
    let title = if is_destructive {
        " ⛔ Permanent Deletion "
    } else {
        " ⚠️ Confirmation "
    };

    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    let content_lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", message),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "       [ Yes (Y / Enter) ]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "       [ No (N / Esc) ]",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let p = Paragraph::new(content_lines).block(block);
    f.render_widget(p, area);
}

fn draw_help_popup(f: &mut Frame) {
    let area = centered_rect_fixed(78, 27, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(
            " 📖 AI Dialogue Manager Shortcuts ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let help_lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  ↑ / k, ↓ / j     : Navigate dialogue list",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  PgUp, PgDn       : Page up / down",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  Home / End / g/G : Jump to top / bottom",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  Enter / v / r    : Open dialogue (Markdown, tables, syntax highlighting)",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  Space            : Mark / unmark dialogue [✓]",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  a / A            : Select all / deselect all",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  t / T            : Toggle Trash / Active dialogues view",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  u / U            : Restore dialogue from trash (in trash view)",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  d / x            : Move to trash / Delete permanently (in trash view)",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  b / D            : Batch delete marked dialogues",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  c / C            : Empty trash (in trash view) / Purge empty sessions",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  /                : Search by ID or keyword",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  Esc              : Reset search / close popup",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  Tab / f          : Cycle filter (All/User/Subagent/Empty/Marked/Trash)",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  p / P            : Cycle agent filter (All/AGY/Claude/Codex/Grok/Universal)",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  m / M            : Transfer / migrate selected dialogue to another agent",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  o / O            : Open / resume dialogue in terminal CLI agent",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  s / S            : Cycle sort order (Newest/Oldest/Size/Messages)",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  e / E            : Export dialogue to Markdown file",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  R / F5           : Reload dialogues from disk",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  ?                : Show this help popup",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  q / Q            : Quit application",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close help...",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )),
    ];

    let p = Paragraph::new(help_lines).block(block);
    f.render_widget(p, area);
}

fn draw_transfer_popup(f: &mut Frame, dialog: &crate::tui::app::TransferDialog) {
    let area = centered_rect_fixed(72, 14, f.area());
    f.render_widget(Clear, area);

    let border_color = Color::Cyan;
    let block = Block::default()
        .title(Span::styled(
            " 🔄 Transfer Dialogue to Another Agent ",
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    let short_id: String = dialog.item.id.chars().take(12).collect();
    let topic_short: String = dialog.item.topic.chars().take(38).collect();

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Dialogue:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} ({})", short_id, topic_short),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Source Agent: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{} [{}]",
                    dialog.item.agent.display_name(),
                    dialog.item.agent.short_tag()
                ),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Select Destination Agent:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
        )),
    ];

    for (idx, target) in dialog.target_options.iter().enumerate() {
        let is_sel = idx == dialog.selected_idx;
        let (prefix, style) = if is_sel {
            (
                "   ▶ ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ("     ", Style::default().fg(Color::White))
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::Cyan)),
            Span::styled(
                format!(" {:<28} [{}] ", target.display_name(), target.short_tag()),
                style,
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "  [↑/↓ / k/j] Change  ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            "[Enter / Y] Confirm Transfer  ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "[Esc / Q] Cancel",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    ]));

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, area);
}

fn centered_rect_fixed(width: u16, height: u16, r: Rect) -> Rect {
    let w = width.min(r.width.saturating_sub(2));
    let h = height.min(r.height.saturating_sub(2));
    let x = r.x + (r.width.saturating_sub(w)) / 2;
    let y = r.y + (r.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn highlights_unicode_matches_on_original_character_boundaries() {
        for (text, query, expected) in [
            ("İstanbul", "i", "İ"),
            ("Ⱥbc ⱥ", "ⱥ", "Ⱥⱥ"),
            ("i\u{307}stanbul", "İ", "i\u{307}"),
            ("İİ", "i", "İİ"),
            ("Москва", "МОС", "Мос"),
            ("plain text", "", ""),
        ] {
            let spans = highlight_search_in_line(&Line::from(text), query);
            let reconstructed: String = spans.iter().map(|span| span.content.as_ref()).collect();
            let highlighted: String = spans
                .iter()
                .filter(|span| span.style.bg == Some(Color::Yellow))
                .map(|span| span.content.as_ref())
                .collect();
            assert_eq!(reconstructed, text);
            assert_eq!(highlighted, expected);
        }
    }

    #[test]
    fn matches_cross_styled_spans_and_preserve_unmatched_styles() {
        let italic = Style::default().add_modifier(Modifier::ITALIC);
        let line = Line::from(vec![
            Span::styled("istan", Style::default().fg(Color::Cyan)),
            Span::styled("bul and ", italic),
            Span::raw("ISTANBUL"),
        ]);
        let spans = highlight_search_in_line(&line, "istanbul");
        let highlighted: String = spans
            .iter()
            .filter(|span| span.style.bg == Some(Color::Yellow))
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(highlighted, "istanbulISTANBUL");
        assert!(
            spans
                .iter()
                .any(|span| span.content == " and " && span.style == italic)
        );
    }
}
