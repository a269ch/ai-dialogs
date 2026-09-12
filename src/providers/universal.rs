use crate::canonical::{AgentKind, CanonicalDialogue, CanonicalRole};
use crate::error::{AppError, Result};
use crate::models::DialogueItem;
use crate::providers::DialogueProvider;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

pub struct UniversalProvider {
    base_dir: PathBuf,
}

impl Default for UniversalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl UniversalProvider {
    pub fn new() -> Self {
        let base_dir = AgentKind::Universal
            .default_storage_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("sessions");
        Self { base_dir }
    }

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn export_to_file(dialogue: &CanonicalDialogue, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create_new(path)?;
        serde_json::to_writer_pretty(file, dialogue)?;
        Ok(())
    }

    pub fn import_from_file(path: &Path) -> Result<CanonicalDialogue> {
        let file = File::open(path)?;
        let dialogue: CanonicalDialogue = serde_json::from_reader(file)?;
        Ok(dialogue)
    }
}

impl DialogueProvider for UniversalProvider {
    fn kind(&self) -> AgentKind {
        AgentKind::Universal
    }

    fn is_available(&self) -> bool {
        true
    }

    fn base_dir(&self) -> Option<PathBuf> {
        Some(self.base_dir.clone())
    }

    fn list_dialogues(&self) -> Result<Vec<DialogueItem>> {
        let mut items = Vec::new();
        if !self.base_dir.is_dir() {
            return Ok(items);
        }

        if let Ok(entries) = fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                    let stem = match path.file_stem().and_then(|s| s.to_str()) {
                        Some(s) => s.to_string(),
                        None => continue,
                    };

                    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    if let Ok(file) = File::open(&path)
                        && let Ok(dialogue) = serde_json::from_reader::<_, CanonicalDialogue>(file)
                    {
                        let u_count = dialogue.user_messages_count();
                        let a_count = dialogue.assistant_messages_count();
                        let user_messages = dialogue
                            .messages
                            .iter()
                            .filter(|message| message.role == CanonicalRole::User)
                            .map(|message| message.content.clone())
                            .collect();
                        let mut item = DialogueItem::new_external(
                            stem,
                            AgentKind::Universal,
                            dialogue.title,
                            dialogue.created_at,
                            u_count,
                            a_count,
                            size,
                            Some(path),
                        )
                        .with_user_messages(user_messages);
                        item.total_steps = dialogue.messages.len();
                        item.is_empty = dialogue.messages.is_empty();
                        items.push(item);
                    }
                }
            }
        }

        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    fn load_canonical(&self, id: &str) -> Result<CanonicalDialogue> {
        let items = self.list_dialogues()?;
        let item = crate::providers::resolve_dialogue(&items, id, Some(AgentKind::Universal))?
            .ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let path = item
            .transcript_path
            .as_ref()
            .ok_or_else(|| AppError::NotFound(id.to_string()))?;
        Self::import_from_file(path)
    }

    fn save_canonical(&self, dialogue: &CanonicalDialogue) -> Result<String> {
        fs::create_dir_all(&self.base_dir)?;
        let new_id = crate::canonical::uuid_v4_simple();
        let file_path = self.base_dir.join(format!("{}.json", new_id));
        let mut imported = dialogue.clone();
        imported.id = new_id.clone();
        if !dialogue.id.is_empty() {
            imported
                .metadata
                .original_id
                .get_or_insert_with(|| dialogue.id.clone());
        }
        let bytes = serde_json::to_vec_pretty(&imported)?;
        let mut file = File::create_new(file_path)?;
        std::io::Write::write_all(&mut file, &bytes)?;
        Ok(new_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::CanonicalMessage;
    use crate::test_support::TestDir;

    #[test]
    fn imports_never_use_source_ids_as_paths_or_replace_sessions() {
        let fixture = TestDir::new("universal-import");
        let provider = UniversalProvider::with_base_dir(fixture.path().join("sessions"));
        fs::create_dir_all(&provider.base_dir).unwrap();
        let outside = fixture.path().join("outside.json");
        let existing = provider.base_dir.join("existing.json");
        fs::write(&outside, "outside data").unwrap();
        fs::write(&existing, "existing session").unwrap();
        for source_id in [
            "../outside",
            "existing",
            outside.with_extension("").to_str().unwrap(),
            "",
        ] {
            let mut dialogue = CanonicalDialogue::new(source_id, "Import", AgentKind::Claude);
            dialogue
                .messages
                .push(CanonicalMessage::new(CanonicalRole::User, "First prompt"));
            dialogue.messages.push(CanonicalMessage::new(
                CanonicalRole::User,
                "Later searchable keyword",
            ));
            let first = provider.save_canonical(&dialogue).unwrap();
            let first_bytes = fs::read(provider.base_dir.join(format!("{first}.json"))).unwrap();
            let second = provider.save_canonical(&dialogue).unwrap();
            assert_ne!(first, second);
            assert!(uuid::Uuid::parse_str(&first).is_ok());
            assert_eq!(
                fs::read(provider.base_dir.join(format!("{first}.json"))).unwrap(),
                first_bytes
            );
            let loaded = provider.load_canonical(&first).unwrap();
            assert_eq!(loaded.id, first);
            if !source_id.is_empty() {
                assert_eq!(loaded.metadata.original_id.as_deref(), Some(source_id));
            }
            let item = provider
                .list_dialogues()
                .unwrap()
                .into_iter()
                .find(|item| item.id == first)
                .unwrap();
            assert_eq!(
                item.user_messages,
                ["First prompt", "Later searchable keyword"]
            );
        }
        assert_eq!(fs::read_to_string(outside).unwrap(), "outside data");
        assert_eq!(fs::read_to_string(existing).unwrap(), "existing session");
        assert!(provider.load_canonical("../outside").is_err());
    }

    #[test]
    fn export_refuses_to_truncate_existing_output() {
        let fixture = TestDir::new("universal-export");
        let output = fixture.path().join("export.json");
        fs::write(&output, "previous export").unwrap();
        let dialogue = CanonicalDialogue::new("id", "Title", AgentKind::Universal);
        assert!(UniversalProvider::export_to_file(&dialogue, &output).is_err());
        assert_eq!(fs::read_to_string(output).unwrap(), "previous export");
    }
}
