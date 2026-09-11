use crate::canonical::{
    AgentKind, CanonicalDialogue, CanonicalMessage, CanonicalRole, CanonicalToolCall,
};
use crate::error::{AppError, Result};
use crate::models::DialogueItem;
use crate::providers::DialogueProvider;
use crate::storage::DialogueStore;
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

pub struct AntigravityProvider {
    base_dir: PathBuf,
}

impl Default for AntigravityProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AntigravityProvider {
    pub fn new() -> Self {
        let base_dir = AgentKind::Antigravity
            .default_storage_dir()
            .unwrap_or_else(|| PathBuf::from("."));
        Self { base_dir }
    }
}

impl DialogueProvider for AntigravityProvider {
    fn kind(&self) -> AgentKind {
        AgentKind::Antigravity
    }

    fn is_available(&self) -> bool {
        self.base_dir.is_dir()
    }

    fn base_dir(&self) -> Option<PathBuf> {
        Some(self.base_dir.clone())
    }

    fn list_dialogues(&self) -> Result<Vec<DialogueItem>> {
        let store = DialogueStore::new();
        Ok(store.items)
    }

    fn load_canonical(&self, id: &str) -> Result<CanonicalDialogue> {
        let store = DialogueStore::new();
        let item = store
            .items
            .iter()
            .chain(store.trash_items.iter())
            .find(|it| it.id == id || it.id.starts_with(id))
            .ok_or_else(|| AppError::General(format!("Antigravity dialogue not found: {}", id)))?;

        let steps = store.load_conversation_steps(item);
        let mut canonical = CanonicalDialogue::new(&item.id, &item.topic, AgentKind::Antigravity);
        canonical.created_at = item.created_at.clone();

        for (idx, step) in steps.into_iter().enumerate() {
            let role = match step.role.as_str() {
                "user" => CanonicalRole::User,
                "assistant" => CanonicalRole::Assistant,
                "tool_call" => CanonicalRole::ToolCall,
                _ => CanonicalRole::User,
            };

            let tool_calls = step
                .tool_calls
                .into_iter()
                .map(|tc| CanonicalToolCall {
                    name: tc.name,
                    args: tc.args,
                    result: None,
                })
                .collect();

            let msg = CanonicalMessage {
                id: format!("{}-{}", item.id, idx + 1),
                role,
                content: step.content,
                timestamp: Some(step.time),
                tool_calls,
                model: None,
            };
            canonical.messages.push(msg);
        }

        Ok(canonical)
    }

    fn save_canonical(&self, dialogue: &CanonicalDialogue) -> Result<String> {
        let new_id = crate::canonical::uuid_v4_simple();
        let brain_log_dir = self
            .base_dir
            .join("brain")
            .join(&new_id)
            .join(".system_generated")
            .join("logs");
        fs::create_dir_all(&brain_log_dir)?;

        let transcript_path = brain_log_dir.join("transcript.jsonl");
        let mut file = File::create(&transcript_path)?;

        for (idx, msg) in dialogue.messages.iter().enumerate() {
            let (source, step_type) = match msg.role {
                CanonicalRole::User => ("USER_EXPLICIT", "USER_INPUT"),
                CanonicalRole::Assistant => ("MODEL", "PLANNER_RESPONSE"),
                CanonicalRole::ToolCall => ("TOOL", "TOOL_CALL"),
                CanonicalRole::System => ("SYSTEM", "SYSTEM_EVENT"),
            };

            let tool_calls_json: Vec<serde_json::Value> = msg
                .tool_calls
                .iter()
                .map(|tc| {
                    json!({
                        "name": tc.name,
                        "args": tc.args,
                    })
                })
                .collect();

            let line_obj = json!({
                "step_index": idx + 1,
                "source": source,
                "type": step_type,
                "content": msg.content,
                "tool_calls": tool_calls_json,
                "created_at": msg.timestamp.clone().unwrap_or_default(),
            });

            writeln!(file, "{}", serde_json::to_string(&line_obj)?)?;
        }

        Ok(new_id)
    }
}
