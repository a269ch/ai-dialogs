use crate::canonical::{AgentKind, CanonicalDialogue, CanonicalMessage, CanonicalRole};
use crate::error::{AppError, Result};
use crate::models::DialogueItem;
use crate::providers::DialogueProvider;
use chrono::Utc;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use walkdir::WalkDir;

pub struct CodexProvider {
    base_dir: PathBuf,
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexProvider {
    pub fn new() -> Self {
        let base_dir = AgentKind::Codex
            .default_storage_dir()
            .unwrap_or_else(|| PathBuf::from("."));
        Self { base_dir }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.base_dir.join("sessions")
    }

    fn collect_jsonl_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let sessions = self.sessions_dir();
        if !sessions.is_dir() {
            return files;
        }

        for entry in WalkDir::new(&sessions).into_iter().flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                files.push(p.to_path_buf());
            }
        }
        files
    }
}

impl DialogueProvider for CodexProvider {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
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

            let id = if let Some(stripped) = stem.strip_prefix("rollout-") {
                stripped.to_string()
            } else {
                stem
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
                    if created_at.is_none()
                        && let Some(ts) = val.get("timestamp").and_then(|t| t.as_str())
                    {
                        created_at = Some(ts.to_string());
                    }

                    let payload = val.get("payload").unwrap_or(&val);
                    let role = payload
                        .get("role")
                        .and_then(|r| r.as_str())
                        .unwrap_or_default();

                    if role == "user" {
                        user_count += 1;
                        if first_user_msg.is_empty()
                            && let Some(text) = extract_codex_text(payload)
                        {
                            let cleaned = text.lines().next().unwrap_or("").trim().to_string();
                            if !cleaned.starts_with("<environment_context>") && !cleaned.is_empty()
                            {
                                first_user_msg = cleaned;
                            }
                        }
                    } else if role == "assistant" {
                        model_count += 1;
                    }
                }
            }

            let topic = if !first_user_msg.is_empty() {
                first_user_msg.chars().take(90).collect()
            } else if model_count > 0 {
                "[Codex session without user messages]".to_string()
            } else {
                "[Empty session]".to_string()
            };

            let item = DialogueItem::new_external(
                id,
                AgentKind::Codex,
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
                    .map(|stem| stem.contains(id))
                    .unwrap_or(false)
            })
            .ok_or_else(|| AppError::General(format!("Codex dialogue not found: {}", id)))?;

        let file = File::open(&target_file)?;
        let reader = BufReader::new(file);

        let mut dialogue = CanonicalDialogue::new(id, "Codex Dialogue", AgentKind::Codex);
        let mut first_user_found = false;
        let mut ordinal = 0;

        for line in reader.lines().map_while(std::result::Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                let payload = val.get("payload").unwrap_or(&val);
                let role_str = payload
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or_default();

                let role = match role_str {
                    "user" => CanonicalRole::User,
                    "assistant" => CanonicalRole::Assistant,
                    "system" | "developer" => CanonicalRole::System,
                    _ => continue,
                };

                let text = extract_codex_text(payload).unwrap_or_default();
                if text.is_empty() {
                    continue;
                }

                if role == CanonicalRole::User && !first_user_found {
                    let first_line = text.lines().next().unwrap_or("").trim();
                    if !first_line.starts_with("<environment_context>") && !first_line.is_empty() {
                        dialogue.title = first_line.chars().take(90).collect();
                        first_user_found = true;
                    }
                }

                ordinal += 1;
                let timestamp = val
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string());

                dialogue.messages.push(CanonicalMessage {
                    id: format!("{}-{}", id, ordinal),
                    role,
                    content: text,
                    timestamp,
                    tool_calls: Vec::new(),
                    model: None,
                });
            }
        }

        Ok(dialogue)
    }

    fn save_canonical(&self, dialogue: &CanonicalDialogue) -> Result<String> {
        let now = Utc::now();
        let new_id = crate::canonical::uuid_v4_simple();
        let year = now.format("%Y").to_string();
        let month = now.format("%m").to_string();
        let day = now.format("%d").to_string();

        let date_dir = self.sessions_dir().join(year).join(month).join(day);
        fs::create_dir_all(&date_dir)?;

        let filename = format!(
            "rollout-{}-{}.jsonl",
            now.format("%Y-%m-%dT%H-%M-%S"),
            new_id
        );
        let session_file = date_dir.join(filename);
        let mut file = File::create(&session_file)?;

        for (idx, msg) in dialogue.messages.iter().enumerate() {
            let role = match msg.role {
                CanonicalRole::User => "user",
                CanonicalRole::Assistant => "assistant",
                CanonicalRole::System => "developer",
                CanonicalRole::ToolCall => "developer",
            };

            let content_type = if role == "user" {
                "input_text"
            } else {
                "output_text"
            };

            let timestamp = msg.timestamp.clone().unwrap_or_else(|| now.to_rfc3339());

            let line_obj = json!({
                "timestamp": timestamp,
                "ordinal": idx + 1,
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "id": format!("msg_{}", crate::canonical::uuid_v4_simple()),
                    "role": role,
                    "content": [
                        {
                            "type": content_type,
                            "text": msg.content
                        }
                    ]
                }
            });

            writeln!(file, "{}", serde_json::to_string(&line_obj)?)?;
        }

        Ok(new_id)
    }
}

fn extract_codex_text(payload: &Value) -> Option<String> {
    if let Some(c) = payload.get("content") {
        if let Some(s) = c.as_str() {
            return Some(s.to_string());
        }
        if let Some(arr) = c.as_array() {
            let mut parts = Vec::new();
            for item in arr {
                if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
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
