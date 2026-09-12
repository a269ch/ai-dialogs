use crate::canonical::AgentKind;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub args: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueStep {
    pub role: String,
    pub time: String,
    #[serde(default)]
    pub timestamp: Option<String>,
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueItem {
    pub id: String,
    pub agent: AgentKind,
    pub is_in_trash: bool,
    pub trash_folder: Option<PathBuf>,
    pub brain_path: PathBuf,
    pub db_path: PathBuf,
    pub annot_path: PathBuf,
    pub presence_path: PathBuf,
    pub transcript_path: Option<PathBuf>,

    pub created_at: Option<String>,
    pub deleted_at: Option<String>,
    pub user_messages: Vec<String>,
    pub user_msgs_count: usize,
    pub model_msgs_count: usize,
    pub total_steps: usize,
    pub first_user_msg: String,
    pub topic: String,
    pub is_subagent: bool,
    pub is_empty: bool,
    pub size_bytes: u64,
    pub is_marked: bool,
}

impl DialogueItem {
    pub fn with_user_messages(mut self, messages: Vec<String>) -> Self {
        self.first_user_msg = messages.first().cloned().unwrap_or_default();
        self.user_messages = messages;
        self
    }

    pub fn date_str(&self) -> String {
        crate::cleaner::format_date(self.created_at.as_deref())
    }

    pub fn deleted_date_str(&self) -> String {
        crate::cleaner::format_date(self.deleted_at.as_deref())
    }

    pub fn formatted_size(&self) -> String {
        crate::cleaner::format_bytes(self.size_bytes)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_external(
        id: String,
        agent: AgentKind,
        topic: String,
        created_at: Option<String>,
        user_msgs_count: usize,
        model_msgs_count: usize,
        size_bytes: u64,
        transcript_path: Option<PathBuf>,
    ) -> Self {
        let is_empty = user_msgs_count == 0 && model_msgs_count == 0;
        let is_subagent = topic.starts_with("[Subagent]");
        let empty_path = PathBuf::new();
        Self {
            id,
            agent,
            is_in_trash: false,
            trash_folder: None,
            brain_path: empty_path.clone(),
            db_path: empty_path.clone(),
            annot_path: empty_path.clone(),
            presence_path: empty_path,
            transcript_path,
            created_at,
            deleted_at: None,
            user_messages: Vec::new(),
            user_msgs_count,
            model_msgs_count,
            total_steps: user_msgs_count + model_msgs_count,
            first_user_msg: topic.clone(),
            topic,
            is_subagent,
            is_empty,
            size_bytes,
            is_marked: false,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ActiveItemJson<'a> {
    pub id: &'a str,
    pub agent: &'a str,
    pub created_at: Option<&'a str>,
    pub user_messages_count: usize,
    pub model_messages_count: usize,
    pub size_bytes: u64,
    pub is_subagent: bool,
    pub is_empty: bool,
    pub topic: &'a str,
}

#[derive(Debug, Serialize)]
pub struct TrashItemJson<'a> {
    pub id: &'a str,
    pub agent: &'a str,
    pub created_at: Option<&'a str>,
    pub deleted_at: Option<&'a str>,
    pub size_bytes: u64,
    pub topic: &'a str,
    pub trash_folder: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DialogueStoreJson<'a> {
    pub active: Vec<ActiveItemJson<'a>>,
    pub trash: Vec<TrashItemJson<'a>>,
}
