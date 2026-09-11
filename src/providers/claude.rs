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

            for line in reader.lines().map_while(std::result::Result::ok) {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    let role = val
                        .get("message")
                        .and_then(|m| m.get("role"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("");

                    if msg_type == "user" || role == "user" {
                        user_count += 1;
                        if created_at.is_none()
                            && let Some(ts) = val.get("timestamp").and_then(|t| t.as_str())
                        {
                            created_at = Some(ts.to_string());
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

            let item = DialogueItem::new_external(
                stem,
                AgentKind::Claude,
                topic,
                created_at,
                user_count,
                model_count,
                size,
                Some(path),
            );
            items.push(item);
        }

        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    fn load_canonical(&self, id: &str) -> Result<CanonicalDialogue> {
        let files = self.collect_jsonl_files();
        let target_file = files
            .into_iter()
            .find(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|stem| stem == id || stem.starts_with(id))
                    .unwrap_or(false)
            })
            .ok_or_else(|| AppError::General(format!("Claude dialogue not found: {}", id)))?;

        let file = File::open(&target_file)?;
        let reader = BufReader::new(file);

        let mut dialogue = CanonicalDialogue::new(id, "Claude Dialogue", AgentKind::Claude);
        let mut first_user_found = false;

        for line in reader.lines().map_while(std::result::Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
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
                if text.is_empty() && val.get("tool_use").is_none() {
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

                let tool_calls = extract_claude_tool_calls(&val);

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

        Ok(dialogue)
    }

    fn save_canonical(&self, dialogue: &CanonicalDialogue) -> Result<String> {
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
        let mut file = File::create(&session_file)?;

        let init_line = json!({
            "type": "mode",
            "mode": "normal",
            "sessionId": new_id,
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
                CanonicalRole::Assistant => {
                    let mut content_arr = vec![json!({
                        "type": "text",
                        "text": msg.content,
                    })];

                    for tc in &msg.tool_calls {
                        content_arr.push(json!({
                            "type": "tool_use",
                            "name": tc.name,
                            "input": tc.args,
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
