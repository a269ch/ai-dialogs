use regex::Regex;
use std::sync::LazyLock;

static RE_METADATA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<ADDITIONAL_METADATA>.*?</ADDITIONAL_METADATA>").unwrap());
static RE_SETTINGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<USER_SETTINGS_CHANGE>.*?</USER_SETTINGS_CHANGE>").unwrap());
static RE_SKILLS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<SKILL_INSTRUCTIONS>.*?</SKILL_INSTRUCTIONS>").unwrap());
static RE_REQUEST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?USER_REQUEST>").unwrap());

pub fn clean_user_content(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let t = RE_METADATA.replace_all(raw, "");
    let t = RE_SETTINGS.replace_all(&t, "");
    let t = RE_SKILLS.replace_all(&t, "");
    let t = RE_REQUEST.replace_all(&t, "");
    t.trim().to_string()
}

pub fn is_system_noise(content: &str) -> bool {
    let c = content.trim();
    if c.is_empty() {
        return true;
    }
    if c.starts_with("{{ CHECKPOINT") || c.starts_with("{{CHECKPOINT") {
        return true;
    }
    if c.contains("Tool is running as a background task") {
        return true;
    }
    if c.starts_with("Created At:")
        && (c.contains("Completed At:")
            || c.contains("The command exited")
            || c.contains("Tool is running"))
    {
        return true;
    }
    if c.starts_with("File Path:  file:///") || c.starts_with("File Path: file:///") {
        return true;
    }
    if c.starts_with("{\"name\":")
        && (c.contains("sizeBytes") || c.contains("Summary: This directory"))
    {
        return true;
    }
    false
}

pub fn format_bytes(bytes: u64) -> String {
    let mut size = bytes as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if size < 1024.0 {
            if unit == "B" {
                return format!("{} B", bytes);
            } else {
                return format!("{:.1} {}", size, unit);
            }
        }
        size /= 1024.0;
    }
    format!("{:.1} TB", size)
}

pub fn format_date(dt_str: Option<&str>) -> String {
    match dt_str {
        Some(dt) if !dt.is_empty() => {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(dt) {
                parsed.format("%Y-%m-%d %H:%M").to_string()
            } else if dt.len() >= 16 {
                dt[..16].replace('T', " ")
            } else {
                dt.to_string()
            }
        }
        _ => "----/--/-- --:--".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_user_content() {
        let raw = "<ADDITIONAL_METADATA>\ntime=now\n</ADDITIONAL_METADATA><USER_REQUEST>Hello world</USER_REQUEST>";
        assert_eq!(clean_user_content(raw), "Hello world");

        let raw2 = "<SKILL_INSTRUCTIONS>do something</SKILL_INSTRUCTIONS>Test request";
        assert_eq!(clean_user_content(raw2), "Test request");
    }

    #[test]
    fn test_is_system_noise() {
        assert!(is_system_noise("{{ CHECKPOINT 1 }}"));
        assert!(is_system_noise("Tool is running as a background task"));
        assert!(is_system_noise(
            "Created At: 2026-09-11\nCompleted At: 2026-09-11"
        ));
        assert!(!is_system_noise("Hello, please help me write Rust code!"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1073741824), "1.0 GB");
    }

    #[test]
    fn test_format_date() {
        assert_eq!(
            format_date(Some("2026-09-11T16:10:28Z")),
            "2026-09-11 16:10"
        );
        assert_eq!(format_date(None), "----/--/-- --:--");
    }
}
