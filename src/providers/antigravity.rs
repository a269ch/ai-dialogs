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

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
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
        let store = DialogueStore::with_base_dir(self.base_dir.clone());
        Ok(store.items)
    }

    fn load_canonical(&self, id: &str) -> Result<CanonicalDialogue> {
        let store = DialogueStore::with_base_dir(self.base_dir.clone());
        let item =
            crate::providers::resolve_dialogue(&store.items, id, Some(AgentKind::Antigravity))?
                .ok_or_else(|| {
                    AppError::General(format!("Antigravity dialogue not found: {}", id))
                })?;

        let steps = store.load_conversation_steps(item);
        let mut canonical = CanonicalDialogue::new(&item.id, &item.topic, AgentKind::Antigravity);
        canonical.created_at = item.created_at.clone();
        canonical.updated_at = steps
            .iter()
            .rev()
            .find_map(|step| step.timestamp.clone())
            .or_else(|| canonical.created_at.clone());

        for (idx, step) in steps.into_iter().enumerate() {
            let role = match step.role.as_str() {
                "user" => CanonicalRole::User,
                "assistant" => CanonicalRole::Assistant,
                "tool_call" => CanonicalRole::ToolCall,
                "system" => CanonicalRole::System,
                _ => CanonicalRole::User,
            };

            let tool_calls = step
                .tool_calls
                .into_iter()
                .map(|tc| CanonicalToolCall {
                    name: tc.name,
                    args: tc.args,
                    result: tc.result,
                })
                .collect();

            let msg = CanonicalMessage {
                id: format!("{}-{}", item.id, idx + 1),
                role,
                content: step.content,
                timestamp: step.timestamp,
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
        let mut file = File::create_new(&transcript_path)?;

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
                        "function": {
                            "name": tc.name,
                            "arguments": tc.args,
                        },
                        "result": tc.result,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_records_and_full_timestamps_survive_save_load_and_markdown_export() {
        let fixture = crate::test_support::TestDir::new("antigravity-tools");
        let provider = AntigravityProvider::with_base_dir(fixture.path().join("agy"));
        let mut dialogue = CanonicalDialogue::new("source", "tools", AgentKind::Universal);
        for (role, content) in [
            (CanonicalRole::User, "Prompt"),
            (CanonicalRole::Assistant, "Using the tool"),
            (CanonicalRole::ToolCall, "Tool context"),
            (CanonicalRole::Assistant, ""),
            (CanonicalRole::System, "System context"),
        ] {
            let mut message = CanonicalMessage::new(role, content);
            message.timestamp = Some("2021-02-03T04:05:06+03:00".into());
            if matches!(role, CanonicalRole::Assistant | CanonicalRole::ToolCall) {
                message.tool_calls.push(CanonicalToolCall {
                    name: "read_file".into(),
                    args: "{\"path\":\"main.rs\"}".into(),
                    result: Some("file contents".into()),
                });
            }
            dialogue.messages.push(message);
        }
        let id = provider.save_canonical(&dialogue).unwrap();
        let loaded = provider.load_canonical(&id).unwrap();
        assert_eq!(loaded.messages.len(), dialogue.messages.len());
        for (actual, expected) in loaded.messages.iter().zip(&dialogue.messages) {
            assert_eq!(actual.role, expected.role);
            assert_eq!(actual.content, expected.content);
            assert_eq!(actual.timestamp, expected.timestamp);
            assert_eq!(
                serde_json::to_value(&actual.tool_calls).unwrap(),
                serde_json::to_value(&expected.tool_calls).unwrap()
            );
        }
        let item = provider.list_dialogues().unwrap().pop().unwrap();
        let store = DialogueStore::with_base_dir(fixture.path().join("agy"));
        let output = fixture.path().join("export.md");
        store.export_to_markdown(&item, Some(&output)).unwrap();
        let markdown = fs::read_to_string(output).unwrap();
        assert_eq!(markdown.matches("Tool Call `read_file`").count(), 3);
        assert!(markdown.contains("file contents") && markdown.contains("System context"));
    }
}
