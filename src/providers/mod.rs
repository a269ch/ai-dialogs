pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod grok;
pub mod universal;

use crate::canonical::{AgentKind, CanonicalDialogue};
use crate::error::Result;
use crate::models::DialogueItem;
use std::collections::HashMap;
use std::path::PathBuf;

pub trait DialogueProvider: Send + Sync {
    fn kind(&self) -> AgentKind;
    fn display_name(&self) -> &'static str {
        self.kind().display_name()
    }
    fn is_available(&self) -> bool;
    fn base_dir(&self) -> Option<PathBuf>;
    fn list_dialogues(&self) -> Result<Vec<DialogueItem>>;
    fn load_canonical(&self, id: &str) -> Result<CanonicalDialogue>;
    fn save_canonical(&self, dialogue: &CanonicalDialogue) -> Result<String>;
}

pub struct ProviderRegistry {
    providers: HashMap<AgentKind, Box<dyn DialogueProvider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        let mut providers: HashMap<AgentKind, Box<dyn DialogueProvider>> = HashMap::new();
        providers.insert(
            AgentKind::Antigravity,
            Box::new(antigravity::AntigravityProvider::new()),
        );
        providers.insert(AgentKind::Claude, Box::new(claude::ClaudeProvider::new()));
        providers.insert(AgentKind::Codex, Box::new(codex::CodexProvider::new()));
        providers.insert(AgentKind::Grok, Box::new(grok::GrokProvider::new()));
        providers.insert(
            AgentKind::Universal,
            Box::new(universal::UniversalProvider::new()),
        );

        Self { providers }
    }

    pub fn get(&self, kind: AgentKind) -> Option<&dyn DialogueProvider> {
        self.providers.get(&kind).map(|b| b.as_ref())
    }

    pub fn available_providers(&self) -> Vec<AgentKind> {
        AgentKind::ALL
            .iter()
            .copied()
            .filter(|k| {
                self.providers
                    .get(k)
                    .map(|p| p.is_available())
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn list_all_dialogues(&self) -> Vec<DialogueItem> {
        let mut all = Vec::new();
        for kind in AgentKind::ALL {
            if let Some(provider) = self.providers.get(&kind)
                && provider.is_available()
                && let Ok(items) = provider.list_dialogues()
            {
                all.extend(items);
            }
        }
        all
    }

    pub fn find_dialogue(&self, id: &str) -> Option<(&dyn DialogueProvider, DialogueItem)> {
        for kind in AgentKind::ALL {
            if let Some(provider) = self.providers.get(&kind)
                && provider.is_available()
                && let Ok(items) = provider.list_dialogues()
            {
                for it in items {
                    if it.id == id || it.id.starts_with(id) {
                        return Some((provider.as_ref(), it));
                    }
                }
            }
        }
        None
    }
}
