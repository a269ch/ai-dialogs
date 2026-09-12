pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod grok;
pub mod universal;

use crate::canonical::{AgentKind, CanonicalDialogue};
use crate::error::{AppError, Result};
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
    pub fn empty() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Box<dyn DialogueProvider>) {
        self.providers.insert(provider.kind(), provider);
    }

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

    pub fn list_dialogues(&self, filter: Option<AgentKind>) -> Result<Vec<DialogueItem>> {
        let mut items = Vec::new();
        for kind in AgentKind::ALL {
            if filter.is_some_and(|selected| selected != kind) {
                continue;
            }
            if let Some(provider) = self.get(kind)
                && provider.is_available()
            {
                items.extend(provider.list_dialogues()?);
            }
        }
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    pub fn find_dialogue_in(
        &self,
        id: &str,
        filter: Option<AgentKind>,
    ) -> Result<Option<(&dyn DialogueProvider, DialogueItem)>> {
        let items = self.list_dialogues(filter)?;
        Ok(resolve_dialogue(&items, id, filter)?.and_then(|item| {
            self.get(item.agent)
                .map(|provider| (provider, item.clone()))
        }))
    }

    pub fn find_dialogue(&self, id: &str) -> Option<(&dyn DialogueProvider, DialogueItem)> {
        self.find_dialogue_in(id, None).ok().flatten()
    }
}

pub fn resolve_dialogue<'a>(
    items: &'a [DialogueItem],
    id: &str,
    filter: Option<AgentKind>,
) -> Result<Option<&'a DialogueItem>> {
    if id.is_empty() {
        return Err(AppError::General("Dialogue ID cannot be empty".into()));
    }
    let candidates: Vec<_> = items
        .iter()
        .filter(|item| filter.is_none_or(|kind| item.agent == kind) && item.id.starts_with(id))
        .collect();
    let exact: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|item| item.id == id)
        .collect();
    let matches = if exact.is_empty() {
        &candidates
    } else {
        &exact
    };
    match matches.as_slice() {
        [] => Ok(None),
        [item] => Ok(Some(item)),
        _ => Err(AppError::General(format!(
            "Ambiguous dialogue ID '{}'; specify --provider and a unique ID (or trash backup)",
            id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, agent: AgentKind) -> DialogueItem {
        DialogueItem::new_external(id.into(), agent, "Title".into(), None, 1, 0, 0, None)
    }

    #[test]
    fn resolution_rejects_ambiguous_ids_and_prefers_exact_matches() {
        let items = [
            item("shared", AgentKind::Claude),
            item("shared", AgentKind::Universal),
            item("shared-long", AgentKind::Universal),
        ];
        assert!(resolve_dialogue(&items, "shared", None).is_err());
        assert!(resolve_dialogue(&items, "sh", Some(AgentKind::Universal)).is_err());
        let selected = resolve_dialogue(&items, "shared", Some(AgentKind::Universal))
            .unwrap()
            .unwrap();
        assert_eq!(selected.agent, AgentKind::Universal);
        assert_eq!(selected.id, "shared");
        assert!(resolve_dialogue(&items, "missing", None).unwrap().is_none());
        assert!(resolve_dialogue(&items, "", None).is_err());
    }
}
