use crate::canonical::AgentKind;
use crate::error::{AppError, Result};
use crate::models::DialogueItem;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchConfig {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

pub fn find_binary(name: &str) -> Option<PathBuf> {
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    if let Some(home) = dirs::home_dir() {
        let candidates = [
            home.join(".local/bin").join(name),
            home.join(".grok/bin").join(name),
            home.join(".cargo/bin").join(name),
            home.join(".codex/bin").join(name),
        ];
        if let Some(found) = candidates.into_iter().find(|c| c.is_file()) {
            return Some(found);
        }
    }

    let system_candidates = [
        PathBuf::from("/usr/local/bin").join(name),
        PathBuf::from("/opt/homebrew/bin").join(name),
        PathBuf::from("/usr/bin").join(name),
        PathBuf::from("/bin").join(name),
    ];
    system_candidates.into_iter().find(|c| c.is_file())
}

fn resolve_candidate_dir(candidate: PathBuf) -> Option<PathBuf> {
    let clean = if let Some(s) = candidate.to_str() {
        let trimmed = s.trim_start_matches("file://").trim_matches('"');
        PathBuf::from(trimmed)
    } else {
        candidate
    };

    if let Some(home) = dirs::home_dir() {
        if clean.is_dir() && clean != home {
            return Some(clean);
        }
    } else if clean.is_dir() {
        return Some(clean);
    }

    let dir_name = clean.file_name()?.to_str()?;
    if dir_name.is_empty() || dir_name == "/" {
        return None;
    }

    if let Ok(cur) = env::current_dir()
        && cur.file_name().and_then(|f| f.to_str()) == Some(dir_name)
        && cur.is_dir()
    {
        return Some(cur);
    }

    if let Some(home) = dirs::home_dir() {
        let common_parents = [
            "Rust",
            "Projects",
            "Developer",
            "Development",
            "src",
            "Workspace",
            "work",
            "code",
            "GolandProjects",
            "IdeaProjects",
            "RustroverProjects",
            "Desktop",
            "Documents",
        ];
        for parent in common_parents {
            let p = home.join(parent).join(dir_name);
            if p.is_dir() {
                return Some(p);
            }
        }
    }

    if clean.is_dir() {
        return Some(clean);
    }

    None
}

pub fn extract_cwd(item: &DialogueItem) -> Option<PathBuf> {
    if let Some(ref path) = item.transcript_path
        && let Ok(file) = File::open(path)
    {
        let reader = BufReader::new(file);
        for line in reader.lines().take(250).map_while(std::result::Result::ok) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(cwd) = val.get("cwd").and_then(|v| v.as_str())
                    && let Some(resolved) = resolve_candidate_dir(PathBuf::from(cwd))
                {
                    return Some(resolved);
                }
                if let Some(payload) = val.get("payload")
                    && let Some(cwd) = payload.get("cwd").and_then(|v| v.as_str())
                    && let Some(resolved) = resolve_candidate_dir(PathBuf::from(cwd))
                {
                    return Some(resolved);
                }
                let git_val = val
                    .get("git")
                    .or_else(|| val.get("payload").and_then(|p| p.get("git")));
                if let Some(git) = git_val
                    && let Some(repo_url) = git.get("repository_url").and_then(|r| r.as_str())
                {
                    let trimmed = repo_url.trim_end_matches(".git");
                    if let Some(name) = trimmed.rsplit(['/', ':']).next()
                        && !name.is_empty()
                        && let Some(resolved) = resolve_candidate_dir(PathBuf::from(name))
                    {
                        return Some(resolved);
                    }
                }
            }
        }
    }

    if let Some(ref path) = item.transcript_path
        && let Some(parent) = path.parent()
        && let Some(folder_name) = parent.file_name().and_then(|f| f.to_str())
        && folder_name.starts_with('-')
    {
        let reconstructed = folder_name.replace('-', "/");
        if let Some(resolved) = resolve_candidate_dir(PathBuf::from(reconstructed)) {
            return Some(resolved);
        }
        if let Some(last_comp) = folder_name.rsplit('-').next()
            && let Some(resolved) = resolve_candidate_dir(PathBuf::from(last_comp))
        {
            return Some(resolved);
        }
    }

    let agy_log = item
        .brain_path
        .join(".system_generated/logs/transcript.jsonl");
    if let Ok(file) = File::open(agy_log) {
        let reader = BufReader::new(file);
        for line in reader.lines().take(250).map_while(std::result::Result::ok) {
            if (line.contains("Cwd") || line.contains("workspace"))
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(&line)
                && let Some(tool_calls) = val.get("tool_calls").and_then(|t| t.as_array())
            {
                for tc in tool_calls {
                    if let Some(cwd) = tc
                        .get("args")
                        .and_then(|a| a.get("Cwd"))
                        .and_then(|c| c.as_str())
                        && let Some(resolved) = resolve_candidate_dir(PathBuf::from(cwd))
                    {
                        return Some(resolved);
                    }
                }
            }
        }
    }

    if item.db_path.is_file()
        && let Ok(conn) = rusqlite::Connection::open_with_flags(
            &item.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        && let Ok(mut stmt) = conn
            .prepare("SELECT workspace_uris FROM conversation_summaries WHERE conversation_id = ?1")
        && let Ok(uris_json) = stmt.query_row([&item.id], |row| row.get::<_, String>(0))
        && let Ok(uris) = serde_json::from_str::<Vec<String>>(&uris_json)
    {
        for uri in uris {
            let path_str = uri.trim_start_matches("file://");
            if let Some(resolved) = resolve_candidate_dir(PathBuf::from(path_str)) {
                return Some(resolved);
            }
        }
    }

    None
}

fn heal_codex_lineage_if_needed(path: &Path) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let Some(first_newline) = content.find('\n') else {
        return;
    };
    let first_line = &content[..first_newline];
    let Ok(mut val) = serde_json::from_str::<serde_json::Value>(first_line) else {
        return;
    };

    let Some(payload) = val.get_mut("payload") else {
        return;
    };

    let Some(parent_id) = payload.get("forked_from_id").and_then(|v| v.as_str()) else {
        return;
    };
    let parent_uuid = parent_id.to_string();

    let parent_exists = if let Some(sessions_dir) = path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
    {
        walkdir::WalkDir::new(sessions_dir)
            .into_iter()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(&parent_uuid))
    } else {
        false
    };

    if !parent_exists {
        if let Some(obj) = payload.as_object_mut() {
            obj.remove("forked_from_id");
            obj.remove("forked_from_ordinal_exclusive");
            obj.remove("history_mode");
            obj.remove("history_base");
        }
        val["ordinal"] = serde_json::json!(0);
        if let Ok(new_first_line) = serde_json::to_string(&val) {
            let rest = &content[first_newline..];
            let new_content = format!("{}{}", new_first_line, rest);
            let _ = std::fs::write(path, new_content);
        }
    }
}

pub fn prepare_launch(item: &DialogueItem) -> Result<LaunchConfig> {
    let cwd = extract_cwd(item);

    match item.agent {
        AgentKind::Antigravity => {
            let bin = find_binary("agy")
                .or_else(|| find_binary("antigravity"))
                .ok_or_else(|| {
                    AppError::General(
                        "Antigravity CLI ('agy') not found in PATH or standard directories"
                            .to_string(),
                    )
                })?;
            Ok(LaunchConfig {
                program: bin,
                args: vec!["--conversation".to_string(), item.id.clone()],
                cwd,
            })
        }
        AgentKind::Claude => {
            let bin = find_binary("claude").ok_or_else(|| {
                AppError::General(
                    "Claude Code CLI ('claude') not found in PATH or standard directories"
                        .to_string(),
                )
            })?;
            Ok(LaunchConfig {
                program: bin,
                args: vec!["--resume".to_string(), item.id.clone()],
                cwd,
            })
        }
        AgentKind::Codex => {
            if let Some(ref p) = item.transcript_path {
                heal_codex_lineage_if_needed(p);
            }
            let bin = find_binary("codex").ok_or_else(|| {
                AppError::General(
                    "OpenAI Codex CLI ('codex') not found in PATH or standard directories"
                        .to_string(),
                )
            })?;
            let mut args = vec!["resume".to_string()];
            if let Some(ref dir) = cwd {
                args.push("-C".to_string());
                args.push(dir.to_string_lossy().to_string());
            }
            args.push(item.id.clone());
            Ok(LaunchConfig {
                program: bin,
                args,
                cwd,
            })
        }
        AgentKind::Grok => {
            let bin = find_binary("grok").ok_or_else(|| {
                AppError::General(
                    "Grok CLI ('grok') not found in PATH or standard directories".to_string(),
                )
            })?;
            Ok(LaunchConfig {
                program: bin,
                args: vec!["--continue".to_string()],
                cwd,
            })
        }
        AgentKind::Universal => {
            let editor = env::var("EDITOR")
                .ok()
                .or_else(|| env::var("VISUAL").ok())
                .unwrap_or_else(|| "nano".to_string());
            let bin = find_binary(&editor)
                .or_else(|| find_binary("nano"))
                .or_else(|| find_binary("vim"))
                .or_else(|| find_binary("open"))
                .ok_or_else(|| {
                    AppError::General(
                        "No text editor or viewer found to open Universal JSON session".to_string(),
                    )
                })?;
            let target_path = item
                .transcript_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| item.id.clone());
            Ok(LaunchConfig {
                program: bin,
                args: vec![target_path],
                cwd,
            })
        }
    }
}

pub fn run_interactive(item: &DialogueItem) -> Result<ExitStatus> {
    let config = prepare_launch(item)?;
    let mut cmd = Command::new(&config.program);
    cmd.args(&config.args);
    if let Some(ref dir) = config.cwd {
        cmd.current_dir(dir);
    }
    let status = cmd.status().map_err(|e| {
        AppError::General(format!(
            "Failed to launch '{}': {}",
            config.program.display(),
            e
        ))
    })?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_system_binary() {
        let sh = find_binary("sh");
        assert!(sh.is_some());
    }

    #[test]
    fn test_prepare_launch_args() {
        let item = DialogueItem::new_external(
            "test-id".to_string(),
            AgentKind::Claude,
            "Topic".to_string(),
            None,
            1,
            1,
            100,
            None,
        );
        if let Ok(cfg) = prepare_launch(&item) {
            assert_eq!(cfg.args, vec!["--resume", "test-id"]);
        }

        let codex_item = DialogueItem::new_external(
            "codex-id".to_string(),
            AgentKind::Codex,
            "Topic".to_string(),
            None,
            1,
            1,
            100,
            None,
        );
        if let Ok(cfg) = prepare_launch(&codex_item) {
            assert_eq!(cfg.args, vec!["resume", "codex-id"]);
        }

        let agy_item = DialogueItem::new_external(
            "agy-id".to_string(),
            AgentKind::Antigravity,
            "Topic".to_string(),
            None,
            1,
            1,
            100,
            None,
        );
        if let Ok(cfg) = prepare_launch(&agy_item) {
            assert_eq!(cfg.args, vec!["--conversation", "agy-id"]);
        }

        let grok_item = DialogueItem::new_external(
            "grok-id".to_string(),
            AgentKind::Grok,
            "Topic".to_string(),
            None,
            1,
            1,
            100,
            None,
        );
        if let Ok(cfg) = prepare_launch(&grok_item) {
            assert_eq!(cfg.args, vec!["--continue"]);
        }
    }

    #[test]
    fn test_resolve_candidate_dir() {
        let fake = PathBuf::from("/Users/alekseiche/RustroverProjects/ai-dialogs");
        let resolved = resolve_candidate_dir(fake);
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().file_name().unwrap(), "ai-dialogs");
    }

    #[test]
    fn test_codex_launch_args_with_cwd() {
        let dir = std::env::temp_dir().join("test_codex_launcher_dir");
        let _ = std::fs::create_dir_all(&dir);
        let log = dir.join("session.jsonl");
        let content = format!("{{\"payload\":{{\"cwd\":\"{}\"}}}}\n", dir.display());
        let _ = std::fs::write(&log, content);

        let codex_item = DialogueItem::new_external(
            "codex-id".to_string(),
            AgentKind::Codex,
            "Topic".to_string(),
            None,
            1,
            1,
            100,
            Some(log),
        );
        if let Ok(cfg) = prepare_launch(&codex_item) {
            assert_eq!(
                cfg.args,
                vec!["resume", "-C", dir.to_str().unwrap(), "codex-id"]
            );
            assert_eq!(cfg.cwd, Some(dir.clone()));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_actual_codex_session_resolution() {
        let path = PathBuf::from(
            "/Users/alekseiche/.codex/sessions/2026/09/12/rollout-2026-09-12T13-12-25-01a0969b-24b2-7380-9eab-fa1dcf4fb33f.jsonl",
        );
        if path.is_file() {
            let item = DialogueItem::new_external(
                "01a0969b-24b2-7380-9eab-fa1dcf4fb33f".to_string(),
                AgentKind::Codex,
                "Topic".to_string(),
                None,
                1,
                1,
                100,
                Some(path),
            );
            let cwd = extract_cwd(&item);
            assert!(cwd.is_some());
            assert_eq!(cwd.as_ref().unwrap().file_name().unwrap(), "ai-dialogs");
            let cfg = prepare_launch(&item).unwrap();
            assert!(cfg.args.contains(&"-C".to_string()));
            let c_idx = cfg.args.iter().position(|a| a == "-C").unwrap();
            assert_eq!(cfg.args[c_idx + 1], cwd.unwrap().to_str().unwrap());
        }
    }
}
