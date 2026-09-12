use crate::canonical::{
    AgentKind, CanonicalDialogue, CanonicalMessage, CanonicalRole, CanonicalToolCall,
};
use crate::error::{AppError, Result};
use crate::models::DialogueItem;
use crate::providers::DialogueProvider;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

pub struct ClaudeProvider {
    base_dir: PathBuf,
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeProvider {
    pub fn new() -> Self {
        let base_dir = AgentKind::Claude
            .default_storage_dir()
            .unwrap_or_else(|| PathBuf::from("."));
        Self::with_base_dir(base_dir)
    }

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn projects_dir(&self) -> PathBuf {
        self.base_dir.join("projects")
    }

    fn collect_jsonl_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let projects = self.projects_dir();
        if !projects.is_dir() {
            return files;
        }

        if let Ok(entries) = fs::read_dir(&projects) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if let Ok(sub_entries) = fs::read_dir(&p) {
                        for sub in sub_entries.flatten() {
                            let sub_p = sub.path();
                            if sub_p.is_file()
                                && sub_p.extension().and_then(|e| e.to_str()) == Some("jsonl")
                            {
                                files.push(sub_p);
                            }
                        }
                    }
                } else if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    files.push(p);
                }
            }
        }
        files
    }
}

impl DialogueProvider for ClaudeProvider {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn is_available(&self) -> bool {
        self.base_dir.is_dir()
    }

    fn base_dir(&self) -> Option<PathBuf> {
        Some(self.base_dir.clone())
    }

    fn list_dialogues(&self) -> Result<Vec<DialogueItem>> {
        let mut items = Vec::new();
        for path in self.collect_jsonl_files() {
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size == 0 {
                continue;
            }

            let file = match File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };

            let reader = BufReader::new(file);
            let mut user_count = 0;
            let mut model_count = 0;
            let mut first_user_msg = String::new();
            let mut created_at = None;
            let mut user_messages = Vec::new();

            for line in reader.lines().map_while(std::result::Result::ok) {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    if created_at.is_none() {
                        created_at = val
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                    }
                    let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    let role = val
                        .get("message")
                        .and_then(|m| m.get("role"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("");

                    if msg_type == "user" || role == "user" {
                        user_count += 1;
                        if let Some(content) = extract_claude_text(&val) {
                            user_messages.push(content);
                        }
                        if first_user_msg.is_empty()
                            && let Some(content) = extract_claude_text(&val)
                        {
                            let cleaned = content.lines().next().unwrap_or("").trim().to_string();
                            if !cleaned.starts_with("<local-command") && !cleaned.is_empty() {
                                first_user_msg = cleaned;
                            }
                        }
                    } else if msg_type == "assistant" || role == "assistant" {
                        model_count += 1;
                    }
                }
            }

            let topic = if !first_user_msg.is_empty() {
                first_user_msg.chars().take(90).collect()
            } else if model_count > 0 {
                "[Claude session without user messages]".to_string()
            } else {
                "[Empty session]".to_string()
            };

            let mut item = DialogueItem::new_external(
                stem,
                AgentKind::Claude,
                topic,
                created_at,
                user_count,
                model_count,
                size,
                Some(path),
            );
            item.user_messages = user_messages;
            items.push(item);
        }

        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    fn load_canonical(&self, id: &str) -> Result<CanonicalDialogue> {
        let items = self.list_dialogues()?;
        let item = crate::providers::resolve_dialogue(&items, id, Some(AgentKind::Claude))?
            .ok_or_else(|| AppError::General(format!("Claude dialogue not found: {}", id)))?;
        let target_file = item.transcript_path.as_ref().ok_or_else(|| {
            AppError::General(format!("Claude dialogue has no transcript: {}", id))
        })?;
        let file = File::open(target_file)?;
        let reader = BufReader::new(file);

        let mut dialogue = CanonicalDialogue::new(&item.id, "Claude Dialogue", AgentKind::Claude);
        dialogue.created_at = None;
        dialogue.updated_at = None;
        let mut stored_updated_at = None;
        let mut first_user_found = false;

        for line in reader.lines().map_while(std::result::Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                if let Some(timestamp) = val.get("timestamp").and_then(Value::as_str) {
                    dialogue
                        .created_at
                        .get_or_insert_with(|| timestamp.to_string());
                    dialogue.updated_at = Some(timestamp.to_string());
                }
                if let Some(updated_at) = val.get("updated_at").and_then(Value::as_str) {
                    stored_updated_at = Some(updated_at.to_string());
                }
                let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let msg_obj = val.get("message");
                let role_str = msg_obj
                    .and_then(|m| m.get("role"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("");

                let (role, is_candidate) = if msg_type == "user" || role_str == "user" {
                    (CanonicalRole::User, true)
                } else if msg_type == "assistant" || role_str == "assistant" {
                    (CanonicalRole::Assistant, true)
                } else {
                    (CanonicalRole::System, false)
                };

                if !is_candidate {
                    continue;
                }

                let text = extract_claude_text(&val).unwrap_or_default();
                let tool_calls = extract_claude_tool_calls(&val);
                if text.is_empty() && tool_calls.is_empty() {
                    continue;
                }

                if role == CanonicalRole::User && !first_user_found {
                    let first_line = text.lines().next().unwrap_or("").trim();
                    if !first_line.starts_with("<local-command") && !first_line.is_empty() {
                        dialogue.title = first_line.chars().take(90).collect();
                        first_user_found = true;
                    }
                }

                let timestamp = val
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string());
                let msg_id = val
                    .get("uuid")
                    .and_then(|u| u.as_str())
                    .unwrap_or(&dialogue.id)
                    .to_string();

                dialogue.messages.push(CanonicalMessage {
                    id: msg_id,
                    role,
                    content: text,
                    timestamp,
                    tool_calls,
                    model: msg_obj
                        .and_then(|m| m.get("model"))
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string()),
                });
            }
        }

        dialogue.updated_at = stored_updated_at.or(dialogue.updated_at);

        Ok(dialogue)
    }

    fn save_canonical(&self, dialogue: &CanonicalDialogue) -> Result<String> {
        for message in &dialogue.messages {
            for call in &message.tool_calls {
                claude_tool_input(&call.args)?;
            }
        }
        let new_id = crate::canonical::uuid_v4_simple();
        let projects_dir = self.projects_dir();

        let target_dir = if let Ok(entries) = fs::read_dir(&projects_dir) {
            let mut found = None;
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    found = Some(entry.path());
                    break;
                }
            }
            found.unwrap_or_else(|| projects_dir.join("imported-sessions"))
        } else {
            projects_dir.join("imported-sessions")
        };

        fs::create_dir_all(&target_dir)?;
        let session_file = target_dir.join(format!("{}.jsonl", new_id));
        let mut file = File::create_new(&session_file)?;

        let init_line = json!({
            "type": "mode",
            "mode": "normal",
            "sessionId": new_id,
            "timestamp": dialogue.created_at,
            "updated_at": dialogue.updated_at,
        });
        writeln!(file, "{}", serde_json::to_string(&init_line)?)?;

        let mut prev_uuid: Option<String> = None;

        for msg in &dialogue.messages {
            let current_uuid = crate::canonical::uuid_v4_simple();
            let timestamp = msg
                .timestamp
                .clone()
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

            let line_obj = match msg.role {
                CanonicalRole::User => {
                    json!({
                        "parentUuid": prev_uuid,
                        "type": "user",
                        "message": {
                            "role": "user",
                            "content": msg.content,
                        },
                        "uuid": current_uuid,
                        "timestamp": timestamp,
                        "sessionId": new_id,
                    })
                }
                CanonicalRole::Assistant | CanonicalRole::ToolCall => {
                    let mut content_arr = Vec::new();
                    if !msg.content.is_empty() {
                        content_arr.push(json!({"type": "text", "text": msg.content}));
                    }

                    for tc in &msg.tool_calls {
                        content_arr.push(json!({
                            "type": "tool_use",
                            "id": format!("toolu_{}", crate::canonical::uuid_v4_simple()),
                            "name": tc.name,
                            "input": claude_tool_input(&tc.args)?,
                        }));
                    }

                    json!({
                        "parentUuid": prev_uuid,
                        "type": "assistant",
                        "message": {
                            "role": "assistant",
                            "content": content_arr,
                        },
                        "uuid": current_uuid,
                        "timestamp": timestamp,
                        "sessionId": new_id,
                    })
                }
                _ => continue,
            };

            writeln!(file, "{}", serde_json::to_string(&line_obj)?)?;
            prev_uuid = Some(current_uuid);
        }

        Ok(new_id)
    }
}

fn claude_tool_input(args: &str) -> Result<Value> {
    let input = if args.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(args)?
    };
    if !input.is_object() {
        return Err(AppError::General(
            "Claude tool arguments must be a JSON object".to_string(),
        ));
    }
    Ok(input)
}

fn extract_claude_text(val: &Value) -> Option<String> {
    let msg_obj = val.get("message").unwrap_or(val);
    if let Some(c) = msg_obj.get("content") {
        if let Some(s) = c.as_str() {
            return Some(s.to_string());
        }
        if let Some(arr) = c.as_array() {
            let mut parts = Vec::new();
            for item in arr {
                if item.get("type").and_then(|t| t.as_str()) == Some("text")
                    && let Some(t) = item.get("text").and_then(|t| t.as_str())
                {
                    parts.push(t);
                }
            }
            if !parts.is_empty() {
                return Some(parts.join("\n"));
            }
        }
    }
    None
}

fn extract_claude_tool_calls(val: &Value) -> Vec<CanonicalToolCall> {
    let mut calls = Vec::new();
    let msg_obj = val.get("message").unwrap_or(val);
    if let Some(arr) = msg_obj.get("content").and_then(|c| c.as_array()) {
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                let name = item
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let args = item
                    .get("input")
                    .map(|i| serde_json::to_string(i).unwrap_or_default())
                    .unwrap_or_default();
                calls.push(CanonicalToolCall {
                    name,
                    args,
                    result: None,
                });
            }
        }
    }
    calls
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ai-dialogs-provider-test-{}",
                crate::canonical::uuid_v4_simple()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tool_only_turns_and_source_dates_survive_loading() {
        let dir = TestDir::new();
        let provider = ClaudeProvider::with_base_dir(dir.0.clone());
        fs::create_dir_all(provider.projects_dir()).unwrap();
        let record = json!({
            "type": "assistant",
            "uuid": "tool-message",
            "timestamp": "2021-01-02T03:04:05+02:00",
            "message": {
                "role": "assistant",
                "content": [{"type": "tool_use", "id": "tool-1", "name": "read_file", "input": {"path": "file.rs"}}]
            }
        });
        fs::write(
            provider.projects_dir().join("tool-only.jsonl"),
            format!("{}\n", record),
        )
        .unwrap();
        let loaded = provider.load_canonical("tool-only").unwrap();
        assert_eq!(
            loaded.created_at.as_deref(),
            Some("2021-01-02T03:04:05+02:00")
        );
        assert_eq!(loaded.updated_at, loaded.created_at);
        assert_eq!(loaded.messages.len(), 1);
        assert!(loaded.messages[0].content.is_empty());
        assert_eq!(loaded.messages[0].tool_calls[0].name, "read_file");
        assert_eq!(
            serde_json::from_str::<Value>(&loaded.messages[0].tool_calls[0].args).unwrap(),
            json!({"path": "file.rs"})
        );
    }

    #[test]
    fn tool_inputs_remain_objects_across_repeated_roundtrips() {
        let dir = TestDir::new();
        let provider = ClaudeProvider::with_base_dir(dir.0.clone());
        let mut dialogue = CanonicalDialogue::new("source", "Tools", AgentKind::Claude);
        dialogue.created_at = Some("2020-01-02T03:04:05Z".to_string());
        dialogue.updated_at = Some("2020-01-02T04:04:05Z".to_string());
        let mut message = CanonicalMessage::new(CanonicalRole::Assistant, "");
        message.timestamp = dialogue.updated_at.clone();
        message.tool_calls.push(CanonicalToolCall {
            name: "run".to_string(),
            args: r#"{"command":"echo \"hello\"","options":{"quiet":true}}"#.to_string(),
            result: None,
        });
        dialogue.messages.push(message);
        for _ in 0..2 {
            let id = provider.save_canonical(&dialogue).unwrap();
            let path = provider
                .list_dialogues()
                .unwrap()
                .into_iter()
                .find(|item| item.id == id)
                .unwrap()
                .transcript_path
                .unwrap();
            let records: Vec<Value> = fs::read_to_string(path)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert!(records[1]["message"]["content"][0]["input"].is_object());
            assert!(records[1]["message"]["content"][0]["id"].is_string());
            let loaded = provider.load_canonical(&id).unwrap();
            assert_eq!(loaded.created_at, dialogue.created_at);
            assert_eq!(loaded.updated_at, dialogue.updated_at);
            assert_eq!(
                loaded.messages[0].tool_calls[0].args,
                dialogue.messages[0].tool_calls[0].args
            );
            dialogue = loaded;
        }
    }

    #[test]
    fn invalid_tool_inputs_do_not_create_partial_sessions() {
        let dir = TestDir::new();
        let provider = ClaudeProvider::with_base_dir(dir.0.clone());
        let mut dialogue = CanonicalDialogue::new("source", "Tools", AgentKind::Claude);
        let mut message = CanonicalMessage::new(CanonicalRole::Assistant, "");
        message.tool_calls.push(CanonicalToolCall {
            name: "run".to_string(),
            args: "not-json".to_string(),
            result: None,
        });
        dialogue.messages.push(message);
        assert!(provider.save_canonical(&dialogue).is_err());
        assert!(provider.collect_jsonl_files().is_empty());
    }

    #[test]
    fn listing_retains_later_prompts_and_full_first_prompt() {
        let dir = TestDir::new();
        let provider = ClaudeProvider::with_base_dir(dir.0.clone());
        let mut dialogue = CanonicalDialogue::new("source", "Search", AgentKind::Claude);
        dialogue.messages = vec![
            CanonicalMessage::new(
                CanonicalRole::User,
                format!("{} hidden keyword", "a".repeat(100)),
            ),
            CanonicalMessage::new(CanonicalRole::User, "later prompt"),
        ];
        provider.save_canonical(&dialogue).unwrap();
        let items = provider.list_dialogues().unwrap();
        assert!(items[0].user_messages[0].contains("hidden keyword"));
        assert_eq!(items[0].user_messages[1], "later prompt");
    }
}
