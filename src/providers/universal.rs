use crate::canonical::{AgentKind, CanonicalDialogue};
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

    pub fn export_to_file(dialogue: &CanonicalDialogue, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(path)?;
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
                        let item = DialogueItem::new_external(
                            stem,
                            AgentKind::Universal,
                            dialogue.title,
                            dialogue.created_at,
                            u_count,
                            a_count,
                            size,
                            Some(path),
                        );
                        items.push(item);
                    }
                }
            }
        }

        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    fn load_canonical(&self, id: &str) -> Result<CanonicalDialogue> {
        let target_path = self.base_dir.join(format!("{}.json", id));
        if target_path.exists() {
            Self::import_from_file(&target_path)
        } else {
            let matches = self.list_dialogues()?;
            let item = matches
                .into_iter()
                .find(|it| it.id == id || it.id.starts_with(id))
                .ok_or_else(|| {
                    AppError::General(format!("Universal dialogue not found: {}", id))
                })?;
            if let Some(ref path) = item.transcript_path {
                Self::import_from_file(path)
            } else {
                Err(AppError::General(format!(
                    "Path not found for dialogue: {}",
                    id
                )))
            }
        }
    }

    fn save_canonical(&self, dialogue: &CanonicalDialogue) -> Result<String> {
        fs::create_dir_all(&self.base_dir)?;
        let new_id = if dialogue.id.is_empty() {
            crate::canonical::uuid_v4_simple()
        } else {
            dialogue.id.clone()
        };
        let file_path = self.base_dir.join(format!("{}.json", new_id));
        Self::export_to_file(dialogue, &file_path)?;
        Ok(new_id)
    }
}
