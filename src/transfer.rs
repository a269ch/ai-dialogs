use crate::canonical::AgentKind;
use crate::error::{AppError, Result};
use crate::providers::ProviderRegistry;
use crate::providers::universal::UniversalProvider;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResult {
    pub source_agent: AgentKind,
    pub target_agent: AgentKind,
    pub source_id: String,
    pub target_id: String,
    pub title: String,
    pub messages_count: usize,
    pub user_messages_count: usize,
    pub assistant_messages_count: usize,
}

pub struct TransferEngine;

impl TransferEngine {
    pub fn transfer(
        registry: &ProviderRegistry,
        source_agent: AgentKind,
        target_agent: AgentKind,
        dialogue_id: &str,
    ) -> Result<TransferResult> {
        let source_provider = registry.get(source_agent).ok_or_else(|| {
            AppError::General(format!("Source provider not found: {}", source_agent))
        })?;

        let target_provider = registry.get(target_agent).ok_or_else(|| {
            AppError::General(format!("Target provider not found: {}", target_agent))
        })?;

        let (_, source_item) = registry
            .find_dialogue_in(dialogue_id, Some(source_agent))?
            .ok_or_else(|| AppError::NotFound(dialogue_id.to_string()))?;
        let mut dialogue = source_provider.load_canonical(&source_item.id)?;
        dialogue
            .metadata
            .original_id
            .get_or_insert_with(|| source_item.id.clone());
        dialogue.source_agent = target_agent;

        let user_count = dialogue.user_messages_count();
        let assistant_count = dialogue.assistant_messages_count();
        let total_count = dialogue.messages.len();
        let title = dialogue.title.clone();

        let new_id = target_provider.save_canonical(&dialogue)?;

        Ok(TransferResult {
            source_agent,
            target_agent,
            source_id: source_item.id,
            target_id: new_id,
            title,
            messages_count: total_count,
            user_messages_count: user_count,
            assistant_messages_count: assistant_count,
        })
    }

    pub fn export_file(
        registry: &ProviderRegistry,
        source_agent: AgentKind,
        dialogue_id: &str,
        dest_path: &Path,
    ) -> Result<()> {
        let source_provider = registry.get(source_agent).ok_or_else(|| {
            AppError::General(format!("Source provider not found: {}", source_agent))
        })?;

        let dialogue = source_provider.load_canonical(dialogue_id)?;
        UniversalProvider::export_to_file(&dialogue, dest_path)?;
        Ok(())
    }

    pub fn import_file(
        registry: &ProviderRegistry,
        target_agent: AgentKind,
        src_path: &Path,
    ) -> Result<String> {
        let target_provider = registry.get(target_agent).ok_or_else(|| {
            AppError::General(format!("Target provider not found: {}", target_agent))
        })?;

        let dialogue = UniversalProvider::import_from_file(src_path)?;
        let new_id = target_provider.save_canonical(&dialogue)?;
        Ok(new_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::{CanonicalDialogue, CanonicalMessage, CanonicalRole};

    #[test]
    fn test_transfer_save_and_load_roundtrip() {
        let dir = crate::test_support::TestDir::new("transfer");
        let mut registry = ProviderRegistry::empty();
        registry.register(Box::new(UniversalProvider::with_base_dir(
            dir.path().join("sessions"),
        )));
        let mut dialogue = CanonicalDialogue::new(
            "transfer-test-1",
            "Solve LeetCode problem in Rust",
            AgentKind::Universal,
        );
        dialogue.messages.push(CanonicalMessage::new(
            CanonicalRole::User,
            "Given an array of integers, return indices of the two numbers.",
        ));
        dialogue.messages.push(CanonicalMessage::new(
            CanonicalRole::Assistant,
            "Use a HashMap to store complements in O(n) time.",
        ));

        let universal_prov = registry.get(AgentKind::Universal).unwrap();
        let saved_id = universal_prov.save_canonical(&dialogue).unwrap();
        assert!(!saved_id.is_empty());

        let loaded = universal_prov.load_canonical(&saved_id).unwrap();
        assert_eq!(loaded.title, "Solve LeetCode problem in Rust");
        assert_eq!(loaded.user_messages_count(), 1);
        assert_eq!(loaded.assistant_messages_count(), 1);
    }
}
