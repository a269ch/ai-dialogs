use crate::canonical::{AgentKind, CanonicalDialogue, CanonicalMessage, CanonicalRole};
use crate::error::{AppError, Result};
use crate::models::DialogueItem;
use crate::providers::DialogueProvider;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct GrokProvider {
    base_dir: PathBuf,
}

impl Default for GrokProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GrokProvider {
    pub fn new() -> Self {
        let base_dir = AgentKind::Grok
            .default_storage_dir()
            .unwrap_or_else(|| PathBuf::from("."));
        Self::with_base_dir(base_dir)
    }

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.base_dir.join("sessions")
    }

    fn collect_session_files(&self) -> Vec<PathBuf> {
        WalkDir::new(self.sessions_dir())
            .into_iter()
            .flatten()
            .filter(|entry| {
                entry.file_type().is_file()
                    && matches!(
                        entry.path().extension().and_then(|ext| ext.to_str()),
                        Some("json" | "jsonl")
                    )
            })
            .map(|entry| entry.into_path())
            .collect()
    }
}

impl DialogueProvider for GrokProvider {
    fn kind(&self) -> AgentKind {
        AgentKind::Grok
    }

    fn is_available(&self) -> bool {
        self.base_dir.is_dir()
    }

    fn base_dir(&self) -> Option<PathBuf> {
        Some(self.base_dir.clone())
    }

    fn list_dialogues(&self) -> Result<Vec<DialogueItem>> {
        let mut items = Vec::new();
        for path in self.collect_session_files() {
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let size = fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            if size == 0 {
                continue;
            }
            let Ok(dialogue) = load_grok_file(&path, id) else {
                continue;
            };
            let mut item = DialogueItem::new_external(
                id.to_string(),
                AgentKind::Grok,
                dialogue.title.clone(),
                dialogue.created_at.clone(),
                dialogue.user_messages_count(),
                dialogue.assistant_messages_count(),
                size,
                Some(path),
            );
            item.user_messages = dialogue
                .messages
                .iter()
                .filter(|message| message.role == CanonicalRole::User)
                .map(|message| message.content.clone())
                .collect();
            item.total_steps = dialogue.messages.len();
            item.is_empty = dialogue.messages.is_empty();
            items.push(item);
        }
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    fn load_canonical(&self, id: &str) -> Result<CanonicalDialogue> {
        let items = self.list_dialogues()?;
        let item = crate::providers::resolve_dialogue(&items, id, Some(AgentKind::Grok))?
            .ok_or_else(|| AppError::General(format!("Grok dialogue not found: {}", id)))?;
        let path = item
            .transcript_path
            .as_ref()
            .ok_or_else(|| AppError::General(format!("Grok dialogue has no transcript: {}", id)))?;
        load_grok_file(path, &item.id)
    }

    fn save_canonical(&self, dialogue: &CanonicalDialogue) -> Result<String> {
        let new_id = crate::canonical::uuid_v4_simple();
        let sessions = self.sessions_dir();
        fs::create_dir_all(&sessions)?;
        let session_file = sessions.join(format!("{}.json", new_id));
        let mut file = File::create_new(&session_file)?;
        let obj = json!({
            "id": new_id,
            "title": dialogue.title,
            "created_at": dialogue.created_at,
            "updated_at": dialogue.updated_at,
            "model": dialogue.metadata.model,
            "messages": dialogue.messages,
        });
        writeln!(file, "{}", serde_json::to_string_pretty(&obj)?)?;
        Ok(new_id)
    }
}

fn load_grok_file(path: &Path, id: &str) -> Result<CanonicalDialogue> {
    let file = File::open(path)?;
    let mut dialogue = CanonicalDialogue::new(id, "Grok Dialogue", AgentKind::Grok);
    dialogue.created_at = None;
    dialogue.updated_at = None;
    let mut has_title = false;
    let mut stored_updated_at = None;
    let records = if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
        let value: Value = serde_json::from_reader(file)?;
        if let Some(title) = value.get("title").and_then(Value::as_str) {
            dialogue.title = title.to_string();
            has_title = true;
        }
        dialogue.created_at = value
            .get("created_at")
            .and_then(Value::as_str)
            .map(str::to_string);
        stored_updated_at = value
            .get("updated_at")
            .and_then(Value::as_str)
            .map(str::to_string);
        dialogue.metadata.model = value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string);
        value
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else {
        let mut records = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if !line.trim().is_empty() {
                records.push(serde_json::from_str(&line)?);
            }
        }
        records
    };
    for (index, record) in records.iter().enumerate() {
        let Some(role) = record.get("role").and_then(Value::as_str) else {
            continue;
        };
        let timestamp = record
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(timestamp) = timestamp.as_ref() {
            dialogue.created_at.get_or_insert_with(|| timestamp.clone());
            dialogue.updated_at = Some(timestamp.clone());
        }
        let content = record
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let role = CanonicalRole::parse_str(role);
        if !has_title && role == CanonicalRole::User && !content.trim().is_empty() {
            dialogue.title = content
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .chars()
                .take(90)
                .collect();
            has_title = true;
        }
        dialogue.messages.push(CanonicalMessage {
            id: record
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{}-{}", id, index + 1)),
            role,
            content: content.to_string(),
            timestamp,
            tool_calls: record
                .get("tool_calls")
                .map(|calls| serde_json::from_value(calls.clone()))
                .transpose()?
                .unwrap_or_default(),
            model: record
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| dialogue.metadata.model.clone()),
        });
    }
    dialogue.updated_at = stored_updated_at
        .or(dialogue.updated_at)
        .or_else(|| dialogue.created_at.clone());
    Ok(dialogue)
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
    fn json_roundtrip_preserves_title_dates_messages_and_search_text() {
        let dir = TestDir::new();
        let provider = GrokProvider::with_base_dir(dir.0.clone());
        let mut dialogue = CanonicalDialogue::new("source", "Original title", AgentKind::Grok);
        dialogue.created_at = Some("2020-02-03T04:05:06Z".to_string());
        dialogue.updated_at = Some("2020-02-04T04:05:06Z".to_string());
        dialogue.metadata.model = Some("source-model".to_string());
        dialogue.messages = vec![
            CanonicalMessage::new(CanonicalRole::User, "First prompt"),
            CanonicalMessage::new(CanonicalRole::User, "Later searchable keyword"),
        ];
        for message in &mut dialogue.messages {
            message.timestamp = dialogue.updated_at.clone();
        }
        let id = provider.save_canonical(&dialogue).unwrap();
        let loaded = provider.load_canonical(&id).unwrap();
        assert_eq!(loaded.title, dialogue.title);
        assert_eq!(loaded.created_at, dialogue.created_at);
        assert_eq!(loaded.updated_at, dialogue.updated_at);
        assert_eq!(loaded.metadata.model, dialogue.metadata.model);
        assert_eq!(loaded.messages[0].timestamp, dialogue.messages[0].timestamp);
        let items = provider.list_dialogues().unwrap();
        assert_eq!(items[0].topic, "Original title");
        assert_eq!(
            items[0].user_messages,
            ["First prompt", "Later searchable keyword"]
        );
    }

    #[test]
    fn jsonl_uses_original_message_timestamps() {
        let dir = TestDir::new();
        let provider = GrokProvider::with_base_dir(dir.0.clone());
        fs::create_dir_all(provider.sessions_dir()).unwrap();
        let records = [
            json!({"role": "user", "content": "Historical question", "timestamp": "2019-01-02T03:04:05Z"}),
            json!({"role": "assistant", "content": "Historical answer", "timestamp": "2019-01-02T04:04:05Z"}),
        ];
        fs::write(
            provider.sessions_dir().join("history.jsonl"),
            records
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let loaded = provider.load_canonical("history").unwrap();
        assert_eq!(loaded.title, "Historical question");
        assert_eq!(loaded.created_at.as_deref(), Some("2019-01-02T03:04:05Z"));
        assert_eq!(loaded.updated_at.as_deref(), Some("2019-01-02T04:04:05Z"));
        assert_eq!(loaded.messages[0].timestamp, loaded.created_at);
    }

    #[test]
    fn missing_dates_remain_unknown_instead_of_becoming_now() {
        let dir = TestDir::new();
        let provider = GrokProvider::with_base_dir(dir.0.clone());
        fs::create_dir_all(provider.sessions_dir()).unwrap();
        fs::write(
            provider.sessions_dir().join("unknown.json"),
            json!({"messages": [{"role": "user", "content": "Undated"}]}).to_string(),
        )
        .unwrap();
        let loaded = provider.load_canonical("unknown").unwrap();
        assert_eq!(loaded.created_at, None);
        assert_eq!(loaded.updated_at, None);
    }
}
