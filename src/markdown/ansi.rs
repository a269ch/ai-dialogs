use regex::Regex;
use unicode_width::UnicodeWidthStr;

use crate::markdown::ast::{AlertType, MarkdownBlock, parse_markdown_blocks};
use crate::markdown::syntax::{RE_INLINE, compute_table_widths, keywords_for_lang};

pub fn render_ansi(text: &str, max_width: usize) -> String {
    let mut out = String::new();
    let blocks = parse_markdown_blocks(text);

    for block in blocks {
        match block {
            MarkdownBlock::Header { level, text } => {
                let color_code = match level {
                    1 => "\x1b[1;38;5;39m━━━ ",
                    2 => "\x1b[1;38;5;213m── ",
                    _ => "\x1b[1;38;5;222m▸ ",
                };
                let suffix = match level {
                    1 => " ━━━\x1b[0m\n\n",
                    2 => " ──\x1b[0m\n",
                    _ => "\x1b[0m\n",
                };
                out.push('\n');
                out.push_str(color_code);
                out.push_str(&inline_ansi(text));
                out.push_str(suffix);
            }
            MarkdownBlock::CodeBlock { lang, lines } => {
                let title = if !lang.is_empty() {
                    format!("  {} ", lang)
                } else {
                    " Code ".to_string()
                };
                let w = UnicodeWidthStr::width(title.as_str());
                let rem = max_width.saturating_sub(w + 3);
                out.push_str(&format!(
                    "\x1b[38;5;240m╭─\x1b[1;38;5;220m{}\x1b[0;38;5;240m{}╮\x1b[0m\n",
                    title,
                    "─".repeat(rem)
                ));

                for code_line in lines {
                    let highlighted = highlight_code_line(code_line, lang);
                    out.push_str(&format!("\x1b[38;5;240m│\x1b[0m {}\n", highlighted));
                }

                let border = "─".repeat(max_width.saturating_sub(2));
                out.push_str(&format!("\x1b[38;5;240m╰{}╯\x1b[0m\n", border));
            }
            MarkdownBlock::Table(rows) => {
                out.push_str(&render_table_ansi(&rows));
            }
            MarkdownBlock::Alert { kind, text } => {
                let (icon, label, color) = match kind {
                    AlertType::Note => ("ℹ️", "NOTE:", "\x1b[1;38;5;39m"),
                    AlertType::Tip => ("💡", "TIP:", "\x1b[1;38;5;77m"),
                    AlertType::Warning => ("⚠️", "WARNING:", "\x1b[1;38;5;196m"),
                    AlertType::Important => ("⭐", "IMPORTANT:", "\x1b[1;38;5;220m"),
                };
                out.push_str(&format!("  {} {}{}\x1b[0m\n", icon, color, label));
                if !text.is_empty() {
                    out.push_str(&format!("  \x1b[38;5;244m│\x1b[0m {}\n", inline_ansi(text)));
                }
            }
            MarkdownBlock::Quote(quote) => {
                out.push_str(&format!(
                    "\x1b[38;5;244m│ \x1b[38;5;252m{}\x1b[0m\n",
                    inline_ansi(quote)
                ));
            }
            MarkdownBlock::BulletItem { indent, text } => {
                let pad = " ".repeat(indent);
                out.push_str(&format!(
                    "{}  \x1b[38;5;75m•\x1b[0m {}\n",
                    pad,
                    inline_ansi(text)
                ));
            }
            MarkdownBlock::NumberedItem { indent, num, text } => {
                let pad = " ".repeat(indent);
                out.push_str(&format!(
                    "{}  \x1b[38;5;75m{}\x1b[0m {}\n",
                    pad,
                    num,
                    inline_ansi(text)
                ));
            }
            MarkdownBlock::HorizontalRule => {
                out.push_str(&format!(
                    "\x1b[38;5;238m{}\x1b[0m\n",
                    "─".repeat(max_width.min(80))
                ));
            }
            MarkdownBlock::Paragraph(p) => {
                out.push_str(&inline_ansi(p));
                out.push('\n');
            }
            MarkdownBlock::EmptyLine => {
                out.push('\n');
            }
        }
    }

    out
}

pub fn inline_ansi(text: &str) -> String {
    RE_INLINE
        .replace_all(text, |caps: &regex::Captures| {
            if let Some(m) = caps.name("link_text") {
                let url = caps.name("link_url").map(|u| u.as_str()).unwrap_or("");
                format!(
                    "\x1b[4;38;5;75m{}\x1b[0m (\x1b[38;5;244m{}\x1b[0m)",
                    m.as_str(),
                    url
                )
            } else if let Some(m) = caps.name("code") {
                format!("\x1b[1;38;5;221;48;5;236m {} \x1b[0m", m.as_str())
            } else if let Some(m) = caps.name("bold") {
                format!("\x1b[1;97m{}\x1b[0m", m.as_str())
            } else if let Some(m) = caps.name("strike") {
                format!("\x1b[9;38;5;244m{}\x1b[0m", m.as_str())
            } else if let Some(m) = caps.name("italic") {
                format!("\x1b[3;38;5;250m{}\x1b[0m", m.as_str())
            } else {
                caps.get(0).unwrap().as_str().to_string()
            }
        })
        .to_string()
}

fn render_table_ansi(rows: &[Vec<String>]) -> String {
    let col_widths = compute_table_widths(rows);
    if col_widths.is_empty() {
        return String::new();
    }
    let num_cols = col_widths.len();

    let mut out = String::new();

    out.push('┌');
    for (i, w) in col_widths.iter().enumerate() {
        out.push_str(&"─".repeat(w + 2));
        if i + 1 < num_cols {
            out.push('┬');
        }
    }
    out.push_str("┐\n");

    for (r_idx, row) in rows.iter().enumerate() {
        out.push('│');
        for (i, cell) in row.iter().enumerate() {
            let cw = UnicodeWidthStr::width(cell.as_str());
            let pad = col_widths[i].saturating_sub(cw);
            if r_idx == 0 {
                out.push_str(&format!(
                    " \x1b[1;38;5;117m{}\x1b[0m{} │",
                    cell,
                    " ".repeat(pad)
                ));
            } else {
                out.push_str(&format!(" {}{} │", inline_ansi(cell), " ".repeat(pad)));
            }
        }
        out.push('\n');

        if r_idx == 0 && rows.len() > 1 {
            out.push('├');
            for (i, w) in col_widths.iter().enumerate() {
                out.push_str(&"─".repeat(w + 2));
                if i + 1 < num_cols {
                    out.push('┼');
                }
            }
            out.push_str("┤\n");
        }
    }

    out.push('└');
    for (i, w) in col_widths.iter().enumerate() {
        out.push_str(&"─".repeat(w + 2));
        if i + 1 < num_cols {
            out.push('┴');
        }
    }
    out.push_str("┘\n");

    out
}

fn highlight_code_line(line: &str, lang: &str) -> String {
    let keywords = keywords_for_lang(lang);
    if keywords.is_empty() {
        return format!("\x1b[38;5;253m{}\x1b[0m", line);
    }

    let mut result = line.to_string();
    for &kw in keywords {
        let pattern = format!(r"\b{}\b", kw);
        if let Ok(re) = Regex::new(&pattern) {
            result = re
                .replace_all(&result, |_: &regex::Captures| {
                    format!("\x1b[1;38;5;75m{}\x1b[0m", kw)
                })
                .to_string();
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inline_ansi_bold_and_code() {
        let input = "This is **bold** and `code` test";
        let rendered = inline_ansi(input);
        assert!(rendered.contains("bold"));
        assert!(rendered.contains("code"));
    }

    #[test]
    fn test_render_ansi_headers_and_tables() {
        let input = "# Test Header\n\n| Col 1 | Col 2 |\n|---|---|\n| Val 1 | Val 2 |";
        let out = render_ansi(input, 80);
        assert!(out.contains("Test Header"));
        assert!(out.contains("Val 1"));
        assert!(out.contains("Val 2"));
    }
}
