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

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
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

            let mut id = stem
                .get(stem.len().saturating_sub(36)..)
                .filter(|suffix| uuid::Uuid::parse_str(suffix).is_ok())
                .unwrap_or_else(|| stem.strip_prefix("rollout-").unwrap_or(&stem))
                .to_string();

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
            let mut stored_title = None;

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
                    if val["type"] == "session_meta" {
                        if let Some(session_id) = payload["id"].as_str() {
                            id = session_id.to_string();
                        }
                        if let Some(timestamp) = payload["timestamp"].as_str() {
                            created_at = Some(timestamp.to_string());
                        }
                        stored_title = payload["title"].as_str().map(str::to_string);
                    }
                    let role = payload
                        .get("role")
                        .and_then(|r| r.as_str())
                        .unwrap_or_default();

                    if role == "user" {
                        user_count += 1;
                        if let Some(text) = extract_codex_text(payload) {
                            user_messages.push(text);
                        }
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

            let topic = if let Some(title) = stored_title {
                title
            } else if !first_user_msg.is_empty() {
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
            )
            .with_user_messages(user_messages);
            items.push(item);
        }

        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    fn load_canonical(&self, id: &str) -> Result<CanonicalDialogue> {
        let items = self.list_dialogues()?;
        let item = crate::providers::resolve_dialogue(&items, id, Some(AgentKind::Codex))?
            .ok_or_else(|| AppError::General(format!("Codex dialogue not found: {}", id)))?;
        let target_file = item
            .transcript_path
            .as_ref()
            .ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let file = File::open(target_file)?;
        let reader = BufReader::new(file);

        let mut dialogue = CanonicalDialogue::new(&item.id, "Codex Dialogue", AgentKind::Codex);
        dialogue.created_at = None;
        dialogue.updated_at = None;
        let mut stored_updated_at = None;
        let mut first_user_found = false;
        let mut ordinal = 0;

        for line in reader.lines().map_while(std::result::Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                let payload = val.get("payload").unwrap_or(&val);
                if let Some(timestamp) = val["timestamp"].as_str() {
                    dialogue
                        .created_at
                        .get_or_insert_with(|| timestamp.to_string());
                    dialogue.updated_at = Some(timestamp.to_string());
                }
                if val["type"] == "session_meta" {
                    if let Some(timestamp) = payload["timestamp"].as_str() {
                        dialogue.created_at = Some(timestamp.to_string());
                    }
                    if let Some(title) = payload["title"].as_str() {
                        dialogue.title = title.to_string();
                        first_user_found = true;
                    }
                    stored_updated_at = payload["updated_at"].as_str().map(str::to_string);
                    dialogue.metadata.project_path = payload["cwd"].as_str().map(str::to_string);
                    continue;
                }
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
                    id: format!("{}-{}", item.id, ordinal),
                    role,
                    content: text,
                    timestamp,
                    tool_calls: Vec::new(),
                    model: None,
                });
            }
        }

        dialogue.updated_at = stored_updated_at
            .or(dialogue.updated_at)
            .or_else(|| dialogue.created_at.clone());
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
        let mut file = File::create_new(&session_file)?;
        let timestamp = dialogue
            .created_at
            .clone()
            .unwrap_or_else(|| now.to_rfc3339());
        let cwd = dialogue
            .metadata
            .project_path
            .clone()
            .unwrap_or(std::env::current_dir()?.to_string_lossy().into_owned());
        let header = json!({
            "timestamp": timestamp,
            "type": "session_meta",
            "payload": {
                "id": new_id,
                "timestamp": timestamp,
                "cwd": cwd,
                "originator": "ai-dialogs",
                "cli_version": env!("CARGO_PKG_VERSION"),
                "source": "cli",
                "model_provider": "openai",
                "title": dialogue.title,
                "updated_at": dialogue.updated_at,
            }
        });
        writeln!(file, "{header}")?;

        for (idx, msg) in dialogue.messages.iter().enumerate() {
            let role = match msg.role {
                CanonicalRole::User => "user",
                CanonicalRole::Assistant => "assistant",
                CanonicalRole::System => "developer",
                CanonicalRole::ToolCall => "developer",
            };

            let content_type = if role == "assistant" {
                "output_text"
            } else {
                "input_text"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn saved_rollout_starts_with_metadata_and_preserves_search_text_and_dates() {
        let fixture = TestDir::new("codex-metadata");
        let provider = CodexProvider::with_base_dir(fixture.path().to_path_buf());
        let mut dialogue = CanonicalDialogue::new("source", "Historical title", AgentKind::Claude);
        dialogue.created_at = Some("2020-01-02T03:04:05Z".into());
        dialogue.updated_at = Some("2020-01-02T04:04:05Z".into());
        dialogue.metadata.project_path = Some(fixture.path().to_string_lossy().into_owned());
        dialogue.messages = vec![
            CanonicalMessage::new(CanonicalRole::User, "First prompt\nMore context"),
            CanonicalMessage::new(CanonicalRole::User, "Later searchable keyword"),
            CanonicalMessage::new(CanonicalRole::Assistant, "Answer"),
        ];
        let id = provider.save_canonical(&dialogue).unwrap();
        let items = provider.list_dialogues().unwrap();
        assert_eq!(items[0].id, id);
        assert_eq!(
            items[0].user_messages,
            ["First prompt\nMore context", "Later searchable keyword"]
        );
        let records = fs::read_to_string(items[0].transcript_path.as_ref().unwrap()).unwrap();
        let header: Value = serde_json::from_str(records.lines().next().unwrap()).unwrap();
        assert_eq!(header["type"], "session_meta");
        assert_eq!(header["payload"]["id"], id);
        assert_eq!(
            header["payload"]["timestamp"],
            dialogue.created_at.as_ref().unwrap().as_str()
        );
        assert_eq!(
            header["payload"]["cwd"],
            dialogue.metadata.project_path.as_ref().unwrap().as_str()
        );
        let loaded = provider.load_canonical(&id[..8]).unwrap();
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.title, dialogue.title);
        assert_eq!(loaded.created_at, dialogue.created_at);
        assert_eq!(loaded.updated_at, dialogue.updated_at);
        assert_eq!(loaded.messages.len(), 3);
    }

    #[test]
    fn metadata_id_wins_over_filename_and_legacy_rollouts_use_uuid_suffix() {
        let fixture = TestDir::new("codex-id");
        let provider = CodexProvider::with_base_dir(fixture.path().to_path_buf());
        fs::create_dir_all(provider.sessions_dir()).unwrap();
        let uuid = "12345678-1234-4234-8234-123456789abc";
        let path = provider
            .sessions_dir()
            .join(format!("rollout-2020-01-02T03-04-05-{uuid}.jsonl"));
        let message = json!({"type":"response_item", "payload":{"type":"message", "role":"user", "content":"Undated question"}});
        fs::write(&path, message.to_string()).unwrap();
        assert_eq!(provider.list_dialogues().unwrap()[0].id, uuid);
        let loaded = provider.load_canonical(uuid).unwrap();
        assert_eq!(loaded.created_at, None);
        assert_eq!(loaded.updated_at, None);
        let header = json!({"type":"session_meta", "payload":{"id":"metadata-id", "timestamp":"2020-01-02T03:04:05Z"}});
        fs::write(path, format!("{header}\n{message}\n")).unwrap();
        assert_eq!(provider.list_dialogues().unwrap()[0].id, "metadata-id");
        let loaded = provider.load_canonical("metadata-id").unwrap();
        assert_eq!(loaded.created_at.as_deref(), Some("2020-01-02T03:04:05Z"));
    }
}
