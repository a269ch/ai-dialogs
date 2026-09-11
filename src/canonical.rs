use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Antigravity,
    Claude,
    Codex,
    Grok,
    Universal,
}

impl AgentKind {
    pub const ALL: [AgentKind; 5] = [
        AgentKind::Antigravity,
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Grok,
        AgentKind::Universal,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Antigravity => "agy",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Grok => "grok",
            Self::Universal => "universal",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Antigravity => "Google Antigravity",
            Self::Claude => "Claude Code",
            Self::Codex => "OpenAI Codex",
            Self::Grok => "xAI Grok",
            Self::Universal => "Universal JSON",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "agy" | "antigravity" | "gemini" => Some(Self::Antigravity),
            "claude" | "claude-code" | "anthropic" => Some(Self::Claude),
            "codex" | "openai" | "chatgpt" => Some(Self::Codex),
            "grok" | "xai" => Some(Self::Grok),
            "universal" | "json" => Some(Self::Universal),
            _ => None,
        }
    }

    pub fn default_storage_dir(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        match self {
            Self::Antigravity => Some(home.join(".gemini").join("antigravity-cli")),
            Self::Claude => Some(home.join(".claude")),
            Self::Codex => Some(home.join(".codex")),
            Self::Grok => Some(home.join(".grok")),
            Self::Universal => dirs::data_dir()
                .or(Some(home.join(".local").join("share")))
                .map(|p| p.join("ai-dialogs")),
        }
    }

    pub fn short_tag(&self) -> &'static str {
        match self {
            Self::Antigravity => "AGY",
            Self::Claude => "CLAUDE",
            Self::Codex => "CODEX",
            Self::Grok => "GROK",
            Self::Universal => "UNIV",
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanonicalRole {
    System,
    User,
    Assistant,
    ToolCall,
}

impl CanonicalRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::ToolCall => "tool_call",
        }
    }

    pub fn parse_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "system" => Self::System,
            "user" | "human" => Self::User,
            "assistant" | "model" | "agent" => Self::Assistant,
            "tool_call" | "tool" | "call" => Self::ToolCall,
            _ => Self::User,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CanonicalToolCall {
    pub name: String,
    pub args: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMessage {
    pub id: String,
    pub role: CanonicalRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<CanonicalToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl CanonicalMessage {
    pub fn new(role: CanonicalRole, content: impl Into<String>) -> Self {
        Self {
            id: uuid_v4_simple(),
            role,
            content: content.into(),
            timestamp: Some(Utc::now().to_rfc3339()),
            tool_calls: Vec::new(),
            model: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CanonicalMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalDialogue {
    pub schema_version: String,
    pub id: String,
    pub title: String,
    pub source_agent: AgentKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub messages: Vec<CanonicalMessage>,
    pub metadata: CanonicalMetadata,
}

impl CanonicalDialogue {
    pub fn new(id: impl Into<String>, title: impl Into<String>, source: AgentKind) -> Self {
        Self {
            schema_version: "1.0".to_string(),
            id: id.into(),
            title: title.into(),
            source_agent: source,
            created_at: Some(Utc::now().to_rfc3339()),
            updated_at: Some(Utc::now().to_rfc3339()),
            messages: Vec::new(),
            metadata: CanonicalMetadata::default(),
        }
    }

    pub fn user_messages_count(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| m.role == CanonicalRole::User)
            .count()
    }

    pub fn assistant_messages_count(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| m.role == CanonicalRole::Assistant)
            .count()
    }

    pub fn total_steps(&self) -> usize {
        self.messages.len()
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json_str(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

pub fn uuid_v4_simple() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = duration.as_nanos();
    let rand_val = (nanos ^ (nanos >> 32)) as u64;
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        (nanos & 0xFFFF_FFFF) as u32,
        ((nanos >> 32) & 0xFFFF) as u16,
        ((rand_val >> 16) & 0x0FFF) as u16,
        ((rand_val >> 32) & 0x0FFF) as u16,
        rand_val & 0xFFFF_FFFF_FFFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_dialogue_roundtrip() {
        let mut dialogue = CanonicalDialogue::new(
            "test-123",
            "Write an algorithm in Rust",
            AgentKind::Antigravity,
        );
        dialogue.messages.push(CanonicalMessage::new(
            CanonicalRole::User,
            "Hello, can you write binary search?",
        ));
        dialogue.messages.push(CanonicalMessage::new(
            CanonicalRole::Assistant,
            "Sure! Here is binary search in Rust.",
        ));

        assert_eq!(dialogue.user_messages_count(), 1);
        assert_eq!(dialogue.assistant_messages_count(), 1);
        assert_eq!(dialogue.total_steps(), 2);

        let json = dialogue.to_json_pretty().unwrap();
        let deserialized = CanonicalDialogue::from_json_str(&json).unwrap();
        assert_eq!(deserialized.id, "test-123");
        assert_eq!(deserialized.messages.len(), 2);
        assert_eq!(deserialized.source_agent, AgentKind::Antigravity);
    }

    #[test]
    fn test_agent_kind_parsing() {
        assert_eq!(AgentKind::parse_str("agy"), Some(AgentKind::Antigravity));
        assert_eq!(AgentKind::parse_str("claude"), Some(AgentKind::Claude));
        assert_eq!(AgentKind::parse_str("codex"), Some(AgentKind::Codex));
        assert_eq!(AgentKind::parse_str("grok"), Some(AgentKind::Grok));
        assert_eq!(
            AgentKind::parse_str("universal"),
            Some(AgentKind::Universal)
        );
        assert_eq!(AgentKind::parse_str("unknown"), None);
    }
}
