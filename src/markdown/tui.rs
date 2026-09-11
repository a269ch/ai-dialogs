use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::markdown::ast::{AlertType, MarkdownBlock, parse_markdown_blocks};
use crate::markdown::syntax::{RE_INLINE, compute_table_widths, keywords_for_lang};

pub fn render_markdown_tui(text: &str, max_width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let usable_width = max_width.max(20);
    let blocks = parse_markdown_blocks(text);

    for block in blocks {
        match block {
            MarkdownBlock::Header { level, text } => {
                let (prefix, suffix, color) = match level {
                    1 => ("━━━ ", " ━━━", Color::Cyan),
                    2 => ("── ", " ──", Color::LightMagenta),
                    _ => ("◆ ", "", Color::Yellow),
                };
                if level <= 2 {
                    out.push(Line::from(""));
                }
                out.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(color)),
                    Span::styled(
                        text.to_string(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(suffix, Style::default().fg(color)),
                ]));
                if level <= 2 {
                    out.push(Line::from(""));
                }
            }
            MarkdownBlock::CodeBlock { lang, lines } => {
                let title = if !lang.is_empty() {
                    format!("  {} ", lang)
                } else {
                    " Code ".to_string()
                };
                let w = UnicodeWidthStr::width(title.as_str());
                let rem = usable_width.saturating_sub(w + 3);
                out.push(Line::from(vec![
                    Span::styled("╭─", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        title,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{}╮", "─".repeat(rem)),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));

                for code_line in lines {
                    let highlighted = highlight_code_line_tui(code_line, lang);
                    let mut spans = vec![Span::styled("│ ", Style::default().fg(Color::DarkGray))];
                    spans.extend(highlighted);
                    out.push(Line::from(spans));
                }

                let border = "─".repeat(usable_width.saturating_sub(2));
                out.push(Line::from(Span::styled(
                    format!("╰{}╯", border),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            MarkdownBlock::Table(rows) => {
                out.extend(render_table_tui(&rows));
            }
            MarkdownBlock::Alert { kind, text } => {
                let (icon, label, color) = match kind {
                    AlertType::Note => ("  ℹ️  ", "NOTE:", Color::Cyan),
                    AlertType::Tip => ("  💡 ", "TIP:", Color::Green),
                    AlertType::Warning => ("  ⚠️  ", "WARNING:", Color::Red),
                    AlertType::Important => ("  ⭐ ", "IMPORTANT:", Color::Yellow),
                };
                out.push(Line::from(vec![
                    Span::styled(icon, Style::default().fg(color)),
                    Span::styled(
                        label,
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]));
                if !text.is_empty() {
                    let wrapped = wrap_inline_tui(
                        text,
                        usable_width.saturating_sub(6),
                        Style::default().fg(Color::LightCyan),
                    );
                    for l in wrapped {
                        let mut spans =
                            vec![Span::styled("  │ ", Style::default().fg(Color::Cyan))];
                        spans.extend(l.spans);
                        out.push(Line::from(spans));
                    }
                }
            }
            MarkdownBlock::Quote(quote) => {
                let wrapped = wrap_inline_tui(
                    quote,
                    usable_width.saturating_sub(6),
                    Style::default().fg(Color::LightCyan),
                );
                for l in wrapped {
                    let mut spans = vec![Span::styled("  │ ", Style::default().fg(Color::Cyan))];
                    spans.extend(l.spans);
                    out.push(Line::from(spans));
                }
            }
            MarkdownBlock::BulletItem { indent, text } => {
                let pad = " ".repeat(indent);
                let wrapped = wrap_inline_tui(
                    text,
                    usable_width.saturating_sub(indent + 6),
                    Style::default(),
                );
                for (idx, l) in wrapped.into_iter().enumerate() {
                    let prefix = if idx == 0 {
                        format!("{}  • ", pad)
                    } else {
                        format!("{}    ", pad)
                    };
                    let mut spans =
                        vec![Span::styled(prefix, Style::default().fg(Color::DarkGray))];
                    spans.extend(l.spans);
                    out.push(Line::from(spans));
                }
            }
            MarkdownBlock::NumberedItem { indent, num, text } => {
                let pad = " ".repeat(indent);
                let num_len = num.len();
                let wrapped = wrap_inline_tui(
                    text,
                    usable_width.saturating_sub(indent + num_len + 4),
                    Style::default(),
                );
                for (idx, l) in wrapped.into_iter().enumerate() {
                    let prefix = if idx == 0 {
                        format!("{}  {} ", pad, num)
                    } else {
                        format!("{}{} ", pad, " ".repeat(num_len + 3))
                    };
                    let mut spans =
                        vec![Span::styled(prefix, Style::default().fg(Color::DarkGray))];
                    spans.extend(l.spans);
                    out.push(Line::from(spans));
                }
            }
            MarkdownBlock::HorizontalRule => {
                let rule = "─".repeat(usable_width.saturating_sub(4));
                out.push(Line::from(Span::styled(
                    format!("  {}", rule),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            MarkdownBlock::Paragraph(p) => {
                let wrapped = wrap_inline_tui(p, usable_width.saturating_sub(2), Style::default());
                out.extend(wrapped);
            }
            MarkdownBlock::EmptyLine => {
                out.push(Line::from(""));
            }
        }
    }

    out
}

fn render_table_tui(rows: &[Vec<String>]) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let col_widths = compute_table_widths(rows);
    if col_widths.is_empty() {
        return out;
    }
    let num_cols = col_widths.len();

    let mut top = String::from("┌");
    for (i, w) in col_widths.iter().enumerate() {
        top.push_str(&"─".repeat(w + 2));
        if i + 1 < num_cols {
            top.push('┬');
        }
    }
    top.push('┐');
    out.push(Line::from(Span::styled(
        top,
        Style::default().fg(Color::DarkGray),
    )));

    for (r_idx, row) in rows.iter().enumerate() {
        let mut spans = vec![Span::styled("│", Style::default().fg(Color::DarkGray))];
        for (i, cell) in row.iter().enumerate() {
            let cw = UnicodeWidthStr::width(cell.as_str());
            let pad = col_widths[i].saturating_sub(cw);
            let style = if r_idx == 0 {
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            spans.push(Span::raw(" "));
            spans.push(Span::styled(cell.clone(), style));
            spans.push(Span::raw(" ".repeat(pad + 1)));
            spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
        }
        out.push(Line::from(spans));

        if r_idx == 0 && rows.len() > 1 {
            let mut mid = String::from("├");
            for (i, w) in col_widths.iter().enumerate() {
                mid.push_str(&"─".repeat(w + 2));
                if i + 1 < num_cols {
                    mid.push('┼');
                }
            }
            mid.push('┤');
            out.push(Line::from(Span::styled(
                mid,
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let mut bot = String::from("└");
    for (i, w) in col_widths.iter().enumerate() {
        bot.push_str(&"─".repeat(w + 2));
        if i + 1 < num_cols {
            bot.push('┴');
        }
    }
    bot.push('┘');
    out.push(Line::from(Span::styled(
        bot,
        Style::default().fg(Color::DarkGray),
    )));

    out
}

pub fn tokenize_inline(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut last_idx = 0;

    for mat in RE_INLINE.find_iter(text) {
        let start = mat.start();
        let end = mat.end();

        if start > last_idx {
            spans.push(Span::styled(text[last_idx..start].to_string(), base_style));
        }

        if let Some(caps) = RE_INLINE.captures(&text[start..end]) {
            if let Some(m) = caps.name("link_text") {
                let url = caps.name("link_url").map(|u| u.as_str()).unwrap_or("");
                spans.push(Span::styled(
                    m.as_str().to_string(),
                    base_style
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                spans.push(Span::styled(
                    format!(" ({})", url),
                    base_style.fg(Color::DarkGray),
                ));
            } else if let Some(m) = caps.name("code") {
                spans.push(Span::styled(
                    format!(" {} ", m.as_str()),
                    Style::default()
                        .fg(Color::Yellow)
                        .bg(Color::Rgb(40, 44, 52))
                        .add_modifier(Modifier::BOLD),
                ));
            } else if let Some(m) = caps.name("bold") {
                spans.push(Span::styled(
                    m.as_str().to_string(),
                    base_style.add_modifier(Modifier::BOLD),
                ));
            } else if let Some(m) = caps.name("strike") {
                spans.push(Span::styled(
                    m.as_str().to_string(),
                    base_style
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT),
                ));
            } else if let Some(m) = caps.name("italic") {
                spans.push(Span::styled(
                    m.as_str().to_string(),
                    base_style.add_modifier(Modifier::ITALIC),
                ));
            } else {
                spans.push(Span::styled(mat.as_str().to_string(), base_style));
            }
        }
        last_idx = end;
    }

    if last_idx < text.len() {
        spans.push(Span::styled(text[last_idx..].to_string(), base_style));
    }

    spans
}

pub fn wrap_inline_tui(text: &str, max_width: usize, base_style: Style) -> Vec<Line<'static>> {
    let raw_spans = tokenize_inline(text, base_style);
    let mut words: Vec<Vec<Span<'static>>> = Vec::new();
    let mut cur_word: Vec<Span<'static>> = Vec::new();

    for span in raw_spans {
        let content = span.content.into_owned();
        let style = span.style;

        let mut start = 0;
        let bytes = content.as_bytes();
        let len = bytes.len();

        while start < len {
            let is_whitespace = bytes[start].is_ascii_whitespace();
            let mut end = start + 1;
            while end < len && bytes[end].is_ascii_whitespace() == is_whitespace {
                end += 1;
            }

            let slice = &content[start..end];
            if is_whitespace {
                if !cur_word.is_empty() {
                    words.push(cur_word);
                    cur_word = Vec::new();
                }
                words.push(vec![Span::styled(slice.to_string(), style)]);
            } else {
                cur_word.push(Span::styled(slice.to_string(), style));
            }
            start = end;
        }
    }
    if !cur_word.is_empty() {
        words.push(cur_word);
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut cur_line: Vec<Span<'static>> = Vec::new();
    let mut cur_len = 0;

    for w in words {
        let is_space = w.len() == 1 && w[0].content.chars().all(|c| c.is_whitespace());
        if is_space && cur_len == 0 {
            continue;
        }

        let w_len: usize = w
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();

        if cur_len + w_len <= max_width {
            cur_line.extend(w);
            cur_len += w_len;
            continue;
        }

        if cur_len > 0 && !is_space {
            lines.push(Line::from(cur_line));
            cur_line = Vec::new();
            cur_len = 0;
        }

        if w_len > max_width {
            for span in w {
                let s_content = span.content.into_owned();
                let s_style = span.style;
                let chunks = split_string_to_fit(&s_content, max_width);
                for chk in chunks {
                    let chk_len = UnicodeWidthStr::width(chk.as_str());
                    if cur_len + chk_len > max_width && cur_len > 0 {
                        lines.push(Line::from(cur_line));
                        cur_line = Vec::new();
                        cur_len = 0;
                    }
                    cur_line.push(Span::styled(chk, s_style));
                    cur_len += chk_len;
                }
            }
        } else if !is_space {
            cur_line.extend(w);
            cur_len += w_len;
        }
    }

    if !cur_line.is_empty() {
        lines.push(Line::from(cur_line));
    }

    if lines.is_empty() {
        vec![Line::from("")]
    } else {
        lines
    }
}

fn split_string_to_fit(text: &str, max_w: usize) -> Vec<String> {
    if max_w == 0 {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0;

    for ch in text.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
        if cur_w + cw > max_w && !cur.is_empty() {
            chunks.push(cur);
            cur = String::new();
            cur_w = 0;
        }
        cur.push(ch);
        cur_w += cw;
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

fn highlight_code_line_tui(line: &str, lang: &str) -> Vec<Span<'static>> {
    let keywords = keywords_for_lang(lang);
    if keywords.is_empty() {
        return vec![Span::styled(
            line.to_string(),
            Style::default().fg(Color::White),
        )];
    }

    let mut spans = Vec::new();
    let mut cur_word = String::new();

    let flush_word = |word: &str, spans: &mut Vec<Span<'static>>| {
        if keywords.contains(&word) {
            spans.push(Span::styled(
                word.to_string(),
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if word.starts_with('"') || word.starts_with('\'') {
            spans.push(Span::styled(
                word.to_string(),
                Style::default().fg(Color::Green),
            ));
        } else {
            spans.push(Span::styled(
                word.to_string(),
                Style::default().fg(Color::White),
            ));
        }
    };

    for ch in line.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            cur_word.push(ch);
        } else {
            if !cur_word.is_empty() {
                flush_word(&cur_word, &mut spans);
                cur_word.clear();
            }
            spans.push(Span::styled(
                ch.to_string(),
                Style::default().fg(Color::White),
            ));
        }
    }
    if !cur_word.is_empty() {
        flush_word(&cur_word, &mut spans);
    }

    spans
}
