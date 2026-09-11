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
        Self { base_dir }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.base_dir.join("sessions")
    }

    fn collect_session_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let sessions = self.sessions_dir();
        if !sessions.is_dir() {
            return files;
        }

        for entry in WalkDir::new(&sessions).into_iter().flatten() {
            let p = entry.path();
            if p.is_file()
                && let Some(ext) = p.extension().and_then(|e| e.to_str())
                && (ext == "json" || ext == "jsonl")
            {
                files.push(p.to_path_buf());
            }
        }
        files
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

            let mut user_count = 0;
            let mut model_count = 0;
            let mut first_user_msg = String::new();
            let mut created_at = None;

            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(val) = serde_json::from_reader::<_, Value>(file) {
                    if let Some(arr) = val.get("messages").and_then(|m| m.as_array()) {
                        for msg in arr {
                            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
                            if role == "user" {
                                user_count += 1;
                                if first_user_msg.is_empty() {
                                    first_user_msg = msg
                                        .get("content")
                                        .and_then(|c| c.as_str())
                                        .unwrap_or("")
                                        .lines()
                                        .next()
                                        .unwrap_or("")
                                        .trim()
                                        .to_string();
                                }
                            } else if role == "assistant" {
                                model_count += 1;
                            }
                        }
                    }
                    created_at = val
                        .get("created_at")
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string());
                }
            } else {
                let reader = BufReader::new(file);
                for line in reader.lines().map_while(std::result::Result::ok) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                        let role = val.get("role").and_then(|r| r.as_str()).unwrap_or("");
                        if role == "user" {
                            user_count += 1;
                            if first_user_msg.is_empty() {
                                first_user_msg = val
                                    .get("content")
                                    .and_then(|c| c.as_str())
                                    .unwrap_or("")
                                    .lines()
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();
                            }
                        } else if role == "assistant" {
                            model_count += 1;
                        }
                    }
                }
            }

            let topic = if !first_user_msg.is_empty() {
                first_user_msg.chars().take(90).collect()
            } else if model_count > 0 {
                "[Grok session without user messages]".to_string()
            } else {
                "[Empty session]".to_string()
            };

            let item = DialogueItem::new_external(
                stem,
                AgentKind::Grok,
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
        let files = self.collect_session_files();
        let target_file = files
            .into_iter()
            .find(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|stem| stem == id || stem.starts_with(id))
                    .unwrap_or(false)
            })
            .ok_or_else(|| AppError::General(format!("Grok dialogue not found: {}", id)))?;

        let file = File::open(&target_file)?;
        let mut dialogue = CanonicalDialogue::new(id, "Grok Dialogue", AgentKind::Grok);

        if target_file.extension().and_then(|e| e.to_str()) == Some("json") {
            let val: Value = serde_json::from_reader(file)?;
            if let Some(arr) = val.get("messages").and_then(|m| m.as_array()) {
                for (idx, m) in arr.iter().enumerate() {
                    let role_str = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                    let content = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    let role = match role_str {
                        "user" => CanonicalRole::User,
                        "assistant" => CanonicalRole::Assistant,
                        _ => CanonicalRole::System,
                    };
                    dialogue.messages.push(CanonicalMessage {
                        id: format!("{}-{}", id, idx + 1),
                        role,
                        content: content.to_string(),
                        timestamp: None,
                        tool_calls: Vec::new(),
                        model: Some("grok-3".to_string()),
                    });
                }
            }
        } else {
            let reader = BufReader::new(file);
            let mut idx = 0;
            for line in reader.lines().map_while(std::result::Result::ok) {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    let role_str = val.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                    let content = val.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    let role = match role_str {
                        "user" => CanonicalRole::User,
                        "assistant" => CanonicalRole::Assistant,
                        _ => CanonicalRole::System,
                    };
                    idx += 1;
                    dialogue.messages.push(CanonicalMessage {
                        id: format!("{}-{}", id, idx),
                        role,
                        content: content.to_string(),
                        timestamp: None,
                        tool_calls: Vec::new(),
                        model: Some("grok-3".to_string()),
                    });
                }
            }
        }

        Ok(dialogue)
    }

    fn save_canonical(&self, dialogue: &CanonicalDialogue) -> Result<String> {
        let new_id = crate::canonical::uuid_v4_simple();
        let sessions = self.sessions_dir();
        fs::create_dir_all(&sessions)?;

        let session_file = sessions.join(format!("{}.json", new_id));
        let mut file = File::create(&session_file)?;

        let messages: Vec<Value> = dialogue
            .messages
            .iter()
            .map(|m| {
                json!({
                    "role": m.role.as_str(),
                    "content": m.content,
                })
            })
            .collect();

        let obj = json!({
            "id": new_id,
            "title": dialogue.title,
            "created_at": Utc::now().to_rfc3339(),
            "model": "grok-3",
            "messages": messages,
        });

        writeln!(file, "{}", serde_json::to_string_pretty(&obj)?)?;
        Ok(new_id)
    }
}
