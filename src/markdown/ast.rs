use crate::markdown::syntax::is_table_separator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertType {
    Note,
    Tip,
    Warning,
    Important,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownBlock<'a> {
    Header {
        level: usize,
        text: &'a str,
    },
    CodeBlock {
        lang: &'a str,
        lines: Vec<&'a str>,
    },
    Table(Vec<Vec<String>>),
    Alert {
        kind: AlertType,
        text: &'a str,
    },
    Quote(&'a str),
    BulletItem {
        indent: usize,
        text: &'a str,
    },
    NumberedItem {
        indent: usize,
        num: &'a str,
        text: &'a str,
    },
    HorizontalRule,
    Paragraph(&'a str),
    EmptyLine,
}

pub fn parse_markdown_blocks(text: &str) -> Vec<MarkdownBlock<'_>> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.starts_with("```") {
            let lang = trimmed.trim_start_matches('`').trim();
            let mut code_lines = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                code_lines.push(lines[i]);
                i += 1;
            }
            if i < lines.len() {
                i += 1;
            }
            blocks.push(MarkdownBlock::CodeBlock {
                lang,
                lines: code_lines,
            });
            continue;
        }

        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            let mut table_rows = Vec::new();
            while i < lines.len()
                && lines[i].trim().starts_with('|')
                && lines[i].trim().ends_with('|')
            {
                let r = lines[i].trim();
                if !is_table_separator(r) {
                    let cols: Vec<String> = r
                        .trim_matches('|')
                        .split('|')
                        .map(|c| c.trim().to_string())
                        .collect();
                    table_rows.push(cols);
                }
                i += 1;
            }
            if !table_rows.is_empty() {
                blocks.push(MarkdownBlock::Table(table_rows));
            }
            continue;
        }

        if let Some(h) = trimmed.strip_prefix("# ") {
            blocks.push(MarkdownBlock::Header {
                level: 1,
                text: h.trim(),
            });
            i += 1;
            continue;
        }
        if let Some(h) = trimmed.strip_prefix("## ") {
            blocks.push(MarkdownBlock::Header {
                level: 2,
                text: h.trim(),
            });
            i += 1;
            continue;
        }
        if let Some(h) = trimmed.strip_prefix("### ") {
            blocks.push(MarkdownBlock::Header {
                level: 3,
                text: h.trim(),
            });
            i += 1;
            continue;
        }

        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            blocks.push(MarkdownBlock::HorizontalRule);
            i += 1;
            continue;
        }

        if let Some(content) = trimmed.strip_prefix('>') {
            let content = content.trim();
            if let Some(rest) = content.strip_prefix("[!NOTE]") {
                blocks.push(MarkdownBlock::Alert {
                    kind: AlertType::Note,
                    text: rest.trim(),
                });
            } else if let Some(rest) = content.strip_prefix("[!TIP]") {
                blocks.push(MarkdownBlock::Alert {
                    kind: AlertType::Tip,
                    text: rest.trim(),
                });
            } else if let Some(rest) = content
                .strip_prefix("[!WARNING]")
                .or_else(|| content.strip_prefix("[!CAUTION]"))
            {
                blocks.push(MarkdownBlock::Alert {
                    kind: AlertType::Warning,
                    text: rest.trim(),
                });
            } else if let Some(rest) = content.strip_prefix("[!IMPORTANT]") {
                blocks.push(MarkdownBlock::Alert {
                    kind: AlertType::Important,
                    text: rest.trim(),
                });
            } else {
                blocks.push(MarkdownBlock::Quote(content));
            }
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            let indent = line.len() - line.trim_start().len();
            blocks.push(MarkdownBlock::BulletItem {
                indent,
                text: rest.trim(),
            });
            i += 1;
            continue;
        }

        if let Some((num, rest)) = parse_numbered_item(trimmed) {
            let indent = line.len() - line.trim_start().len();
            blocks.push(MarkdownBlock::NumberedItem {
                indent,
                num,
                text: rest,
            });
            i += 1;
            continue;
        }

        if trimmed.is_empty() {
            blocks.push(MarkdownBlock::EmptyLine);
            i += 1;
            continue;
        }

        blocks.push(MarkdownBlock::Paragraph(line));
        i += 1;
    }

    blocks
}

fn parse_numbered_item(line: &str) -> Option<(&str, &str)> {
    let dot_pos = line.find(". ")?;
    let num_part = &line[..=dot_pos];
    if num_part[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
        Some((num_part, line[dot_pos + 2..].trim()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_blocks() {
        let md = "# Title\n\n```rust\nfn main() {}\n```\n\n> [!NOTE]\n> A note\n\n- Item 1\n1. Numbered\n";
        let blocks = parse_markdown_blocks(md);
        assert!(matches!(
            &blocks[0],
            MarkdownBlock::Header {
                level: 1,
                text: "Title"
            }
        ));
        assert!(matches!(&blocks[1], MarkdownBlock::EmptyLine));
        assert!(matches!(
            &blocks[2],
            MarkdownBlock::CodeBlock { lang: "rust", .. }
        ));
        assert!(matches!(&blocks[3], MarkdownBlock::EmptyLine));
        assert!(matches!(
            &blocks[4],
            MarkdownBlock::Alert {
                kind: AlertType::Note,
                text: ""
            }
        ));
    }

    #[test]
    fn test_parse_numbered_item() {
        let res = parse_numbered_item("1. First item");
        assert_eq!(res, Some(("1.", "First item")));

        let res2 = parse_numbered_item("not a number");
        assert_eq!(res2, None);
    }
}
