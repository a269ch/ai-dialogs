use regex::Regex;
use std::sync::LazyLock;
use unicode_width::UnicodeWidthStr;

pub static RE_INLINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        (\[(?P<link_text>[^\]]+)\]\((?P<link_url>[^)]+)\)) |
        (`(?P<code>[^`]+)`) |
        (\*\*(?P<bold>[^*]+)\*\*) |
        (__([^_]+)__) |
        (~~(?P<strike>[^~]+)~~) |
        (\*(?P<italic>[^*]+)\*)
        ",
    )
    .unwrap()
});

pub fn keywords_for_lang(lang: &str) -> &'static [&'static str] {
    match lang.to_lowercase().as_str() {
        "rust" | "rs" => &[
            "fn", "let", "mut", "struct", "enum", "impl", "trait", "pub", "use", "mod", "match",
            "if", "else", "for", "while", "loop", "return", "where", "async", "await", "self",
            "Self", "const", "static", "type", "ref",
        ],
        "python" | "py" => &[
            "def", "class", "import", "from", "return", "if", "elif", "else", "for", "while", "in",
            "is", "not", "and", "or", "try", "except", "finally", "with", "as", "lambda", "yield",
            "async", "await", "None", "True", "False",
        ],
        "javascript" | "js" | "typescript" | "ts" => &[
            "function",
            "const",
            "let",
            "var",
            "return",
            "if",
            "else",
            "for",
            "while",
            "import",
            "export",
            "from",
            "class",
            "extends",
            "async",
            "await",
            "new",
            "this",
            "typeof",
            "instanceof",
            "null",
            "undefined",
            "true",
            "false",
        ],
        "sh" | "bash" | "zsh" => &[
            "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "case", "esac", "echo",
            "exit", "return", "export", "local", "source",
        ],
        _ => &[],
    }
}

pub fn is_table_separator(line: &str) -> bool {
    let clean = line.replace(['|', '-', ':', ' '], "");
    clean.is_empty()
}

pub fn compute_table_widths(rows: &[Vec<String>]) -> Vec<usize> {
    if rows.is_empty() {
        return Vec::new();
    }
    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if num_cols == 0 {
        return Vec::new();
    }

    let mut col_widths = vec![0; num_cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            let w = UnicodeWidthStr::width(cell.as_str());
            if w > col_widths[i] {
                col_widths[i] = w;
            }
        }
    }
    col_widths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords_for_lang() {
        assert!(keywords_for_lang("rust").contains(&"fn"));
        assert!(keywords_for_lang("rs").contains(&"impl"));
        assert!(keywords_for_lang("python").contains(&"def"));
        assert!(keywords_for_lang("unknown").is_empty());
    }

    #[test]
    fn test_is_table_separator() {
        assert!(is_table_separator("|---|---|"));
        assert!(is_table_separator("|:---|---:|"));
        assert!(!is_table_separator("| a | b |"));
    }

    #[test]
    fn test_compute_table_widths() {
        let rows = vec![
            vec!["Short".to_string(), "Much longer text".to_string()],
            vec!["Longer word".to_string(), "Tiny".to_string()],
        ];
        let widths = compute_table_widths(&rows);
        assert_eq!(widths, vec![11, 16]);
    }
}
