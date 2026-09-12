use crate::canonical::AgentKind;
use crate::cleaner::{clean_user_content, format_bytes, is_system_noise};
use crate::error::{AppError, Result};
use crate::models::{DialogueItem, DialogueStep, ToolCall};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub struct DialogueStore {
    pub base_dir: PathBuf,
    pub brain_dir: PathBuf,
    pub conv_dir: PathBuf,
    pub annot_dir: PathBuf,
    pub presence_dir: PathBuf,
    pub summaries_db: PathBuf,
    pub trash_dir: PathBuf,

    pub items: Vec<DialogueItem>,
    pub trash_items: Vec<DialogueItem>,
}

impl Default for DialogueStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DialogueStore {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let base_dir = home.join(".gemini").join("antigravity-cli");
        Self::with_base_dir(base_dir)
    }

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        let brain_dir = base_dir.join("brain");
        let conv_dir = base_dir.join("conversations");
        let annot_dir = base_dir.join("annotations");
        let presence_dir = base_dir.join("presence");
        let summaries_db = base_dir.join("conversation_summaries.db");
        let trash_dir = base_dir.join("trash");

        let mut store = Self {
            base_dir,
            brain_dir,
            conv_dir,
            annot_dir,
            presence_dir,
            summaries_db,
            trash_dir,
            items: Vec::new(),
            trash_items: Vec::new(),
        };
        store.refresh();
        store
    }

    pub fn items(&self, in_trash: bool) -> &[DialogueItem] {
        if in_trash {
            &self.trash_items
        } else {
            &self.items
        }
    }

    pub fn items_mut(&mut self, in_trash: bool) -> &mut Vec<DialogueItem> {
        if in_trash {
            &mut self.trash_items
        } else {
            &mut self.items
        }
    }

    pub fn refresh(&mut self) {
        self.items.clear();
        self.trash_items.clear();

        let mut conv_ids: HashSet<String> = HashSet::new();

        if self.brain_dir.is_dir()
            && let Ok(entries) = fs::read_dir(&self.brain_dir)
        {
            for entry in entries.flatten() {
                if entry.path().is_dir()
                    && let Ok(name) = entry.file_name().into_string()
                {
                    conv_ids.insert(name);
                }
            }
        }

        if self.conv_dir.is_dir()
            && let Ok(entries) = fs::read_dir(&self.conv_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension()
                    && ext == "db"
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    conv_ids.insert(stem.to_string());
                }
            }
        }

        for cid in conv_ids {
            let item = self.load_dialogue_item(&cid, false, None);
            self.items.push(item);
        }

        self.items.sort_by(|a, b| {
            let a_dt = a.created_at.as_deref().unwrap_or("0");
            let b_dt = b.created_at.as_deref().unwrap_or("0");
            b_dt.cmp(a_dt)
        });

        if self.trash_dir.is_dir()
            && let Ok(entries) = fs::read_dir(&self.trash_dir)
        {
            for entry in entries.flatten() {
                let folder = entry.path();
                let manifest_path = folder.join("manifest.json");
                if manifest_path.is_file() {
                    if let Ok(file) = File::open(&manifest_path)
                        && let Ok(manifest) = serde_json::from_reader::<_, TrashManifest>(file)
                    {
                        let transcript =
                            archived_transcript_path(&folder, &manifest.original_transcript);
                        let mut item = manifest.item;
                        item.is_in_trash = true;
                        item.is_marked = false;
                        item.trash_folder = Some(folder.clone());
                        item.transcript_path = Some(transcript);
                        item.size_bytes = get_dir_size(&folder);
                        self.trash_items.push(item);
                    }
                    continue;
                }
                if folder.is_dir()
                    && let Some(folder_name) = folder.file_name().and_then(|s| s.to_str())
                {
                    let cid = folder_name
                        .rsplit_once('_')
                        .map_or(folder_name, |(id, _)| id)
                        .to_string();
                    if !cid.is_empty() {
                        let item = self.load_dialogue_item(&cid, true, Some(folder));
                        self.trash_items.push(item);
                    }
                }
            }
        }

        self.trash_items.sort_by(|a, b| {
            let a_dt = a.deleted_at.as_deref().unwrap_or("0");
            let b_dt = b.deleted_at.as_deref().unwrap_or("0");
            b_dt.cmp(a_dt)
        });
    }

    fn load_dialogue_item(
        &self,
        conv_id: &str,
        is_trash: bool,
        trash_folder: Option<PathBuf>,
    ) -> DialogueItem {
        let (brain_path, db_path, annot_path, presence_path) =
            self.resolve_item_paths(conv_id, is_trash, trash_folder.as_ref());

        let size_bytes = if is_trash {
            trash_folder
                .as_ref()
                .map(|tf| get_dir_size(tf))
                .unwrap_or(0)
        } else {
            self.calculate_active_size(&brain_path, &db_path, &annot_path, &presence_path)
        };

        let deleted_at = if is_trash {
            trash_folder
                .as_ref()
                .and_then(|tf| tf.file_name().and_then(|s| s.to_str()))
                .and_then(parse_deleted_timestamp)
        } else {
            None
        };

        let transcript_path = find_transcript_path(&brain_path);

        let mut meta = DialogueMeta::default();
        if let Some(ref t_path) = transcript_path {
            let _ = extract_transcript_meta(t_path, &mut meta);
        }

        if meta.user_messages.is_empty() && db_path.exists() {
            let _ = extract_sqlite_meta(&db_path, &mut meta);
        }

        let first_user_msg = meta.user_messages.first().cloned().unwrap_or_default();
        let (topic, is_subagent, is_empty) = derive_topic(&first_user_msg, meta.model_msgs_count);

        DialogueItem {
            id: conv_id.to_string(),
            agent: crate::canonical::AgentKind::Antigravity,
            is_in_trash: is_trash,
            trash_folder,
            brain_path,
            db_path,
            annot_path,
            presence_path,
            transcript_path,
            created_at: meta.created_at,
            deleted_at,
            user_messages: meta.user_messages,
            user_msgs_count: meta.user_msgs_count,
            model_msgs_count: meta.model_msgs_count,
            total_steps: meta.total_steps,
            first_user_msg,
            topic,
            is_subagent,
            is_empty,
            size_bytes,
            is_marked: false,
        }
    }

    fn resolve_item_paths(
        &self,
        conv_id: &str,
        is_trash: bool,
        trash_folder: Option<&PathBuf>,
    ) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        if !is_trash {
            (
                self.brain_dir.join(conv_id),
                self.conv_dir.join(format!("{}.db", conv_id)),
                self.annot_dir.join(format!("{}.pbtxt", conv_id)),
                self.presence_dir.join(format!("{}.lock", conv_id)),
            )
        } else {
            let tf = trash_folder
                .cloned()
                .unwrap_or_else(|| self.trash_dir.join(conv_id));
            (
                tf.join("brain"),
                tf.join(format!("{}.db", conv_id)),
                tf.join(format!("{}.pbtxt", conv_id)),
                tf.join(format!("{}.lock", conv_id)),
            )
        }
    }

    fn calculate_active_size(
        &self,
        brain_path: &Path,
        db_path: &Path,
        annot_path: &Path,
        presence_path: &Path,
    ) -> u64 {
        let mut size: u64 = 0;
        if brain_path.is_dir() {
            size += get_dir_size(brain_path);
        }
        let companion_paths = [
            db_path.to_path_buf(),
            PathBuf::from(format!("{}-wal", db_path.display())),
            PathBuf::from(format!("{}-shm", db_path.display())),
            annot_path.to_path_buf(),
            presence_path.to_path_buf(),
        ];
        for path in &companion_paths {
            if let Ok(meta) = fs::metadata(path) {
                size += meta.len();
            }
        }
        size
    }

    pub fn find(&self, query: &str, in_trash: bool) -> Option<&DialogueItem> {
        let q = query.trim().to_lowercase();
        self.items(in_trash)
            .iter()
            .find(|item| item.id.to_lowercase().starts_with(&q))
    }

    pub fn delete(&mut self, item: &DialogueItem, use_trash: bool, refresh: bool) -> Result<()> {
        if item.is_in_trash {
            return Err(AppError::General(
                "Dialogue is already in trash".to_string(),
            ));
        }
        if item.agent != AgentKind::Antigravity {
            return self.delete_external(item, use_trash, refresh);
        }
        validate_session_id(&item.id)?;
        let paths = self.active_paths(&item.id);
        let sources: Vec<PathBuf> = paths.into_iter().filter(|p| path_exists(p)).collect();
        if sources.is_empty() {
            return Err(AppError::General("Dialogue files not found".to_string()));
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AppError::General(e.to_string()))?
            .as_secs();

        if use_trash {
            fs::create_dir_all(&self.trash_dir)?;
            let trash_subfolder = self.trash_dir.join(format!("{}_{}", item.id, timestamp));
            fs::create_dir(&trash_subfolder)?;
            let moves: Vec<_> = sources
                .iter()
                .map(|source| {
                    let name = if source == &self.brain_dir.join(&item.id) {
                        std::ffi::OsStr::new("brain")
                    } else {
                        source.file_name().expect("session path has a filename")
                    };
                    (source.clone(), trash_subfolder.join(name))
                })
                .collect();
            move_all_or_rollback(&moves)?;
        } else {
            for source in sources {
                remove_path(&source)?;
            }
        }

        if self.summaries_db.exists()
            && let Ok(conn) = Connection::open(&self.summaries_db)
        {
            let _ = conn.execute(
                "DELETE FROM conversation_summaries WHERE conversation_id = ?;",
                [&item.id],
            );
        }

        if refresh {
            self.refresh();
        }

        Ok(())
    }

    fn active_paths(&self, id: &str) -> Vec<PathBuf> {
        vec![
            self.brain_dir.join(id),
            self.conv_dir.join(format!("{}.db", id)),
            self.conv_dir.join(format!("{}.db-wal", id)),
            self.conv_dir.join(format!("{}.db-shm", id)),
            self.annot_dir.join(format!("{}.pbtxt", id)),
            self.presence_dir.join(format!("{}.lock", id)),
        ]
    }

    fn delete_external(
        &mut self,
        item: &DialogueItem,
        use_trash: bool,
        refresh: bool,
    ) -> Result<()> {
        let source = item
            .transcript_path
            .as_ref()
            .filter(|p| p.is_file())
            .ok_or_else(|| AppError::General("Dialogue transcript not found".to_string()))?;
        if !use_trash {
            fs::remove_file(source)?;
        } else {
            let folder = self
                .trash_dir
                .join(format!("external-{}", crate::canonical::uuid_v4_simple()));
            fs::create_dir_all(&self.trash_dir)?;
            fs::create_dir(&folder)?;
            let mut trashed_item = item.clone();
            trashed_item.deleted_at = Some(chrono::Utc::now().to_rfc3339());
            let manifest = TrashManifest {
                item: trashed_item,
                original_transcript: std::path::absolute(source)?,
            };
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(folder.join("manifest.json"))?;
            serde_json::to_writer_pretty(&mut file, &manifest)?;
            file.sync_all()?;
            if let Err(error) =
                move_without_overwrite(source, &archived_transcript_path(&folder, source))
            {
                let _ = fs::remove_file(folder.join("manifest.json"));
                let _ = fs::remove_dir(&folder);
                return Err(error);
            }
        }
        if refresh {
            self.refresh();
        }
        Ok(())
    }

    pub fn delete_batch(&mut self, items: &[DialogueItem], use_trash: bool) -> usize {
        let mut count = 0;
        for it in items {
            if self.delete(it, use_trash, false).is_ok() {
                count += 1;
            }
        }
        self.refresh();
        count
    }

    pub fn restore(&mut self, item: &DialogueItem) -> Result<()> {
        if !item.is_in_trash {
            return Err(AppError::General("Dialogue is not in trash".to_string()));
        }
        let folder = match &item.trash_folder {
            Some(f) if f.is_dir() => f,
            _ => return Err(AppError::General("Trash folder not found".to_string())),
        };

        self.validate_trash_folder(folder)?;
        if item.agent != AgentKind::Antigravity {
            let manifest: TrashManifest =
                serde_json::from_reader(File::open(folder.join("manifest.json"))?)?;
            if manifest.item.id != item.id || manifest.item.agent != item.agent {
                return Err(AppError::General(
                    "Trash manifest does not match the selected dialogue".to_string(),
                ));
            }
            let source = archived_transcript_path(folder, &manifest.original_transcript);
            move_all_or_rollback(&[(source, manifest.original_transcript)])?;
            fs::remove_file(folder.join("manifest.json"))?;
        } else {
            validate_session_id(&item.id)?;
            let destinations = self.active_paths(&item.id);
            for destination in &destinations {
                ensure_absent(destination)?;
            }
            let moves: Vec<_> = destinations
                .into_iter()
                .filter_map(|destination| {
                    let source = if destination == self.brain_dir.join(&item.id) {
                        folder.join("brain")
                    } else {
                        folder.join(
                            destination
                                .file_name()
                                .expect("session path has a filename"),
                        )
                    };
                    path_exists(&source).then_some((source, destination))
                })
                .collect();
            if moves.is_empty() {
                return Err(AppError::General(
                    "No dialogue files found in trash".to_string(),
                ));
            }
            move_all_or_rollback(&moves)?;
        }
        fs::remove_dir(folder)?;
        self.refresh();
        Ok(())
    }

    fn validate_trash_folder(&self, folder: &Path) -> Result<()> {
        let root = fs::canonicalize(&self.trash_dir)?;
        let resolved = fs::canonicalize(folder)?;
        if resolved.parent() != Some(root.as_path()) {
            return Err(AppError::General("Invalid trash folder".to_string()));
        }
        Ok(())
    }

    pub fn delete_permanently(&mut self, item: &DialogueItem, refresh: bool) -> Result<()> {
        if !item.is_in_trash {
            return Err(AppError::General("Dialogue is not in trash".to_string()));
        }
        let folder = match &item.trash_folder {
            Some(f) if f.exists() => f,
            _ => return Err(AppError::General("Trash folder not found".to_string())),
        };

        self.validate_trash_folder(folder)?;

        if folder.is_dir() {
            fs::remove_dir_all(folder)?;
        } else {
            fs::remove_file(folder)?;
        }

        if refresh {
            self.refresh();
        }
        Ok(())
    }

    pub fn delete_permanently_batch(&mut self, items: &[DialogueItem]) -> usize {
        let mut count = 0;
        for it in items {
            if self.delete_permanently(it, false).is_ok() {
                count += 1;
            }
        }
        self.refresh();
        count
    }

    pub fn empty_trash(&mut self) -> Result<usize> {
        let mut count = 0;
        if self.trash_dir.is_dir() {
            for entry in fs::read_dir(&self.trash_dir)? {
                let path = entry?.path();
                remove_path(&path)?;
                count += 1;
            }
        }
        self.refresh();
        Ok(count)
    }

    pub fn clean_empty(&mut self, use_trash: bool) -> usize {
        let empty_items: Vec<DialogueItem> = self
            .items
            .iter()
            .filter(|it| it.is_empty)
            .cloned()
            .collect();
        self.delete_batch(&empty_items, use_trash)
    }

    pub fn load_conversation_steps(&self, item: &DialogueItem) -> Vec<DialogueStep> {
        if let Some(ref t_path) = item.transcript_path
            && let Ok(steps) = load_steps_from_file(t_path)
            && !steps.is_empty()
        {
            return steps;
        }

        if item.db_path.exists()
            && let Ok(steps) = load_steps_from_db(&item.db_path)
        {
            return steps;
        }

        Vec::new()
    }

    pub fn export_to_markdown(
        &self,
        item: &DialogueItem,
        target: Option<&Path>,
    ) -> Result<PathBuf> {
        let steps = self.load_conversation_steps(item);
        let out_path = match target {
            Some(t) => t.to_path_buf(),
            None => {
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
                let out_dir = home.join("Desktop").join("antigravity_exports");
                fs::create_dir_all(&out_dir)?;

                let safe_topic: String = item
                    .topic
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '_' || c == '-' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .take(40)
                    .collect();
                let short_id: String = item.id.chars().take(8).collect();
                out_dir.join(format!("dialogue_{}_{}.md", short_id, safe_topic))
            }
        };

        let mut file = File::create(&out_path)?;

        writeln!(
            file,
            "# [{}] Dialogue: {}\n",
            item.agent.short_tag(),
            item.topic
        )?;
        writeln!(file, "- **Agent:** {}", item.agent.display_name())?;
        writeln!(file, "- **ID:** `{}`", item.id)?;
        writeln!(file, "- **Date:** {}", item.date_str())?;
        writeln!(file, "- **Disk Size:** {}", format_bytes(item.size_bytes))?;
        writeln!(file, "- **User Messages:** {}", item.user_msgs_count)?;
        writeln!(
            file,
            "- **Assistant Messages:** {}\n",
            item.model_msgs_count
        )?;
        writeln!(file, "---\n")?;

        for step in steps {
            let time_str = if !step.time.is_empty() {
                format!(" *({})*", step.time)
            } else {
                String::new()
            };

            match step.role.as_str() {
                "user" => {
                    writeln!(file, "### 👤 User{}\n\n{}\n\n---\n", time_str, step.content)?;
                }
                "assistant" => {
                    writeln!(
                        file,
                        "### 🤖 {}{}\n\n{}\n\n---\n",
                        item.agent.display_name(),
                        time_str,
                        step.content
                    )?;
                }
                "tool_call" => writeln!(file, "### 🛠️ Tool{}\n\n{}\n", time_str, step.content)?,
                "system" => writeln!(file, "### System{}\n\n{}\n", time_str, step.content)?,
                _ => {}
            }
            for tc in &step.tool_calls {
                writeln!(
                    file,
                    "> 🛠️ **Tool Call `{}`**{}\n\n{}\n",
                    tc.name, time_str, tc.args
                )?;
                if let Some(result) = &tc.result {
                    writeln!(file, "{}\n", result)?;
                }
            }
        }

        Ok(out_path)
    }
}

#[derive(Serialize, Deserialize)]
struct TrashManifest {
    item: DialogueItem,
    original_transcript: PathBuf,
}

fn archived_transcript_path(folder: &Path, original: &Path) -> PathBuf {
    folder.join(
        if original.extension().and_then(|ext| ext.to_str()) == Some("json") {
            "transcript.json"
        } else {
            "transcript.jsonl"
        },
    )
}

fn validate_session_id(id: &str) -> Result<()> {
    let mut components = Path::new(id).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
        || id.contains('\\')
    {
        return Err(AppError::General("Invalid dialogue ID".to_string()));
    }
    Ok(())
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn ensure_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(AppError::General(format!(
            "Destination already exists: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn remove_path(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path)?.file_type().is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn move_without_overwrite(source: &Path, destination: &Path) -> Result<()> {
    ensure_absent(destination)?;
    if fs::symlink_metadata(source)?.file_type().is_dir() {
        fs::rename(source, destination)?;
        return Ok(());
    }
    if fs::hard_link(source, destination).is_err() {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        if let Err(error) = std::io::copy(&mut input, &mut output).and_then(|_| output.sync_all()) {
            let _ = fs::remove_file(destination);
            return Err(error.into());
        }
    }
    if let Err(error) = fs::remove_file(source) {
        let _ = fs::remove_file(destination);
        return Err(error.into());
    }
    Ok(())
}

fn move_all_or_rollback(moves: &[(PathBuf, PathBuf)]) -> Result<()> {
    for (source, destination) in moves {
        fs::symlink_metadata(source)?;
        ensure_absent(destination)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    for (index, (source, destination)) in moves.iter().enumerate() {
        if let Err(error) = move_without_overwrite(source, destination) {
            let mut rollback_errors = Vec::new();
            for (original, moved) in moves[..index].iter().rev() {
                if let Err(rollback) = move_without_overwrite(moved, original) {
                    rollback_errors.push(rollback.to_string());
                }
            }
            return Err(AppError::General(if rollback_errors.is_empty() {
                error.to_string()
            } else {
                format!(
                    "{}; some files could not be rolled back: {}",
                    error,
                    rollback_errors.join("; ")
                )
            }));
        }
    }
    Ok(())
}

#[derive(Default)]
struct DialogueMeta {
    created_at: Option<String>,
    user_messages: Vec<String>,
    user_msgs_count: usize,
    model_msgs_count: usize,
    total_steps: usize,
}

impl DialogueMeta {
    fn record(&mut self, parsed: &ParsedRawStep) {
        self.total_steps += 1;
        if self.created_at.is_none() && parsed.created_at.is_some() {
            self.created_at = parsed.created_at.clone();
        }

        if parsed.is_user() {
            let cleaned = clean_user_content(&parsed.content);
            if !cleaned.is_empty() {
                self.user_messages.push(cleaned);
                self.user_msgs_count += 1;
            }
        } else if (parsed.is_model() || parsed.is_tool())
            && ((!parsed.content.is_empty() && !is_system_noise(&parsed.content))
                || !parsed.tool_calls.is_empty())
        {
            self.model_msgs_count += 1;
        }
    }
}

struct ParsedRawStep {
    stype: String,
    ssrc: String,
    created_at: Option<String>,
    time: String,
    content: String,
    tool_calls: Vec<ToolCall>,
}

impl ParsedRawStep {
    fn from_value(val: &Value) -> Self {
        let mut stype = val
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut ssrc = val
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let dt = val
            .get("created_at")
            .or_else(|| val.get("timestamp"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let created_at = if !dt.is_empty() {
            Some(dt.to_string())
        } else {
            None
        };
        let time = dt.get(11..19).unwrap_or_default().to_string();

        let mut content = val
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if content.is_empty()
            && let Some(msg_obj) = val.get("message").or_else(|| val.get("payload"))
            && let Some(c) = msg_obj.get("content")
        {
            if let Some(s) = c.as_str() {
                content = s.to_string();
            } else if let Some(arr) = c.as_array() {
                let mut parts = Vec::new();
                for item in arr {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        parts.push(t);
                    }
                }
                content = parts.join("\n");
            }
        }

        if ssrc.is_empty() && (stype.is_empty() || stype == "response_item") {
            let role = val
                .get("message")
                .and_then(|m| m.get("role"))
                .or_else(|| val.get("payload").and_then(|p| p.get("role")))
                .or_else(|| val.get("role"))
                .and_then(|r| r.as_str())
                .unwrap_or("");

            if role == "user" {
                ssrc = "USER_EXPLICIT".to_string();
                stype = "USER_INPUT".to_string();
            } else if role == "assistant" || role == "model" {
                ssrc = "MODEL".to_string();
                stype = "PLANNER_RESPONSE".to_string();
            } else if matches!(role, "tool_call" | "toolcall" | "tool") {
                ssrc = "TOOL".to_string();
                stype = "TOOL_CALL".to_string();
            } else if role == "system" || role == "developer" {
                ssrc = "SYSTEM".to_string();
                stype = "SYSTEM_EVENT".to_string();
            }
        } else if (stype == "user" || stype == "assistant") && ssrc.is_empty() {
            if stype == "user" {
                ssrc = "USER_EXPLICIT".to_string();
                stype = "USER_INPUT".to_string();
            } else {
                ssrc = "MODEL".to_string();
                stype = "PLANNER_RESPONSE".to_string();
            }
        }

        let mut tool_calls = Vec::new();
        if let Some(tcs) = val
            .get("tool_calls")
            .or_else(|| val.get("message").and_then(|m| m.get("tool_calls")))
            .or_else(|| val.get("payload").and_then(|m| m.get("tool_calls")))
            .and_then(|v| v.as_array())
        {
            for tc in tcs {
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .or_else(|| tc.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let args = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .or_else(|| tc.get("args"))
                    .map(|args| {
                        args.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| args.to_string())
                    })
                    .unwrap_or_default();
                let result = tc.get("result").and_then(Value::as_str).map(str::to_string);
                tool_calls.push(ToolCall { name, args, result });
            }
        }
        let message = val
            .get("message")
            .or_else(|| val.get("payload"))
            .unwrap_or(val);
        if let Some(blocks) = message.get("content").and_then(Value::as_array) {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    tool_calls.push(ToolCall {
                        name: block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool")
                            .to_string(),
                        args: block.get("input").map(Value::to_string).unwrap_or_default(),
                        result: None,
                    });
                }
            }
        }
        if val.get("type").and_then(Value::as_str) == Some("response_item")
            && message.get("type").and_then(Value::as_str) == Some("function_call")
        {
            ssrc = "TOOL".to_string();
            stype = "TOOL_CALL".to_string();
            tool_calls.push(ToolCall {
                name: message
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string(),
                args: message
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                result: None,
            });
        }
        if val.get("type").and_then(Value::as_str) == Some("response_item")
            && message.get("type").and_then(Value::as_str) == Some("function_call_output")
        {
            ssrc = "TOOL".to_string();
            stype = "TOOL_CALL".to_string();
            content = message
                .get("output")
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string())
                })
                .unwrap_or_default();
        }

        Self {
            stype,
            ssrc,
            created_at,
            time,
            content,
            tool_calls,
        }
    }

    fn is_user(&self) -> bool {
        self.stype == "USER_INPUT" || self.ssrc == "USER_EXPLICIT"
    }

    fn is_model(&self) -> bool {
        self.stype == "PLANNER_RESPONSE" || self.ssrc == "MODEL"
    }

    fn is_tool(&self) -> bool {
        self.stype == "TOOL_CALL" || self.ssrc == "TOOL"
    }

    fn into_dialogue_step(self) -> Option<DialogueStep> {
        if self.is_user() {
            let cleaned = clean_user_content(&self.content);
            if !cleaned.is_empty() {
                return Some(DialogueStep {
                    role: "user".to_string(),
                    time: self.time,
                    timestamp: self.created_at,
                    content: cleaned,
                    tool_calls: Vec::new(),
                });
            }
        } else if self.is_model() {
            if (!self.content.is_empty() && !is_system_noise(&self.content))
                || !self.tool_calls.is_empty()
            {
                return Some(DialogueStep {
                    role: "assistant".to_string(),
                    time: self.time,
                    timestamp: self.created_at,
                    content: self.content.trim().to_string(),
                    tool_calls: self.tool_calls,
                });
            }
        } else if self.is_tool() || self.ssrc == "SYSTEM" || self.stype == "SYSTEM_EVENT" {
            return Some(DialogueStep {
                role: if self.is_tool() {
                    "tool_call"
                } else {
                    "system"
                }
                .to_string(),
                time: self.time,
                timestamp: self.created_at,
                content: self.content,
                tool_calls: self.tool_calls,
            });
        }
        None
    }
}

fn find_transcript_path(brain_path: &Path) -> Option<PathBuf> {
    let t1 = brain_path
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl");
    if t1.exists() {
        return Some(t1);
    }
    let t2 = brain_path.join("transcript.jsonl");
    if t2.exists() {
        return Some(t2);
    }
    None
}

fn extract_transcript_meta(path: &Path, meta: &mut DialogueMeta) -> Result<()> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for line in reader.lines().map_while(std::result::Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<Value>(line) {
            let parsed = ParsedRawStep::from_value(&val);
            meta.record(&parsed);
        }
    }
    Ok(())
}

fn extract_sqlite_meta(db_path: &Path, meta: &mut DialogueMeta) -> Result<()> {
    let conn = Connection::open(db_path)?;
    if meta.created_at.is_none()
        && let Ok(mut stmt) = conn.prepare("SELECT created_at FROM trajectory_meta LIMIT 1;")
        && let Ok(mut rows) = stmt.query([])
        && let Ok(Some(row)) = rows.next()
        && let Ok(ca) = row.get::<_, String>(0)
    {
        meta.created_at = Some(ca);
    }

    if let Ok(mut stmt) = conn.prepare("SELECT step_data FROM steps ORDER BY step_index ASC;")
        && let Ok(mut rows) = stmt.query([])
    {
        while let Ok(Some(row)) = rows.next() {
            let raw_json: std::result::Result<String, _> = row.get(0);
            if let Ok(text) = raw_json
                && let Ok(val) = serde_json::from_str::<Value>(&text)
            {
                let parsed = ParsedRawStep::from_value(&val);
                meta.record(&parsed);
            }
        }
    }
    Ok(())
}

fn load_steps_from_file(path: &Path) -> Result<Vec<DialogueStep>> {
    if path.extension().and_then(|e| e.to_str()) == Some("json")
        && let Ok(file) = File::open(path)
        && let Ok(val) = serde_json::from_reader::<_, Value>(file)
        && let Some(messages) = val.get("messages").and_then(|m| m.as_array())
    {
        let mut steps = Vec::new();
        for msg in messages {
            let parsed = ParsedRawStep::from_value(msg);
            if let Some(step) = parsed.into_dialogue_step() {
                steps.push(step);
            }
        }
        return Ok(steps);
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut steps = Vec::new();
    for line in reader.lines().map_while(std::result::Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<Value>(line) {
            let parsed = ParsedRawStep::from_value(&val);
            if let Some(step) = parsed.into_dialogue_step() {
                steps.push(step);
            }
        }
    }
    Ok(steps)
}

fn load_steps_from_db(db_path: &Path) -> Result<Vec<DialogueStep>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT step_data FROM steps ORDER BY step_index ASC;")?;
    let mut rows = stmt.query([])?;
    let mut steps = Vec::new();
    while let Some(row) = rows.next()? {
        let text: String = row.get(0)?;
        if let Ok(val) = serde_json::from_str::<Value>(&text) {
            let parsed = ParsedRawStep::from_value(&val);
            if let Some(step) = parsed.into_dialogue_step() {
                steps.push(step);
            }
        }
    }
    Ok(steps)
}

fn derive_topic(first_user_msg: &str, model_msgs_count: usize) -> (String, bool, bool) {
    if !first_user_msg.is_empty() {
        let subagent_prefixes = [
            "Read the conversation transcript",
            "Read the saved",
            "Read the following conversation",
            "Check if the file",
            "Extract all information",
        ];
        if subagent_prefixes
            .iter()
            .any(|p| first_user_msg.starts_with(p))
        {
            let first_line = first_user_msg
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(80)
                .collect::<String>();
            (format!("[Subagent] {}", first_line), true, false)
        } else {
            let mut first_line = first_user_msg
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if first_line.starts_with("/goal ") {
                first_line = first_line[6..].trim().to_string();
            }
            let truncated: String = first_line.chars().take(90).collect();
            let topic = if truncated.is_empty() {
                "Dialogue".to_string()
            } else {
                truncated
            };
            (topic, false, false)
        }
    } else if model_msgs_count > 0 {
        (
            "[System session without user messages]".to_string(),
            false,
            false,
        )
    } else {
        ("[Empty session (0 messages)]".to_string(), false, true)
    }
}

fn parse_deleted_timestamp(folder_name: &str) -> Option<String> {
    let parts: Vec<&str> = folder_name.split('_').collect();
    if parts.len() >= 2
        && let Ok(ts) = parts.last().unwrap().parse::<i64>()
        && let Some(dt) = chrono::DateTime::from_timestamp(ts, 0)
    {
        Some(dt.format("%Y-%m-%d %H:%M").to_string())
    } else {
        None
    }
}

fn get_dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    for entry in WalkDir::new(path).into_iter().flatten() {
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            total += meta.len();
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ai-dialogs-storage-{}",
                crate::canonical::uuid_v4_simple()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn store(&self) -> DialogueStore {
            DialogueStore::with_base_dir(self.0.join("antigravity"))
        }

        fn active(&self, id: &str) -> (DialogueStore, DialogueItem) {
            let mut store = self.store();
            let brain = store.brain_dir.join(id);
            fs::create_dir_all(&brain).unwrap();
            fs::create_dir_all(&store.conv_dir).unwrap();
            fs::write(
                brain.join("transcript.jsonl"),
                "{\"source\":\"USER_EXPLICIT\",\"content\":\"old conversation\"}\n",
            )
            .unwrap();
            fs::write(store.conv_dir.join(format!("{}.db", id)), "old database").unwrap();
            store.refresh();
            let item = store.items[0].clone();
            (store, item)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn restore_refuses_active_brain_without_touching_either_copy() {
        let fixture = Fixture::new();
        let (mut store, item) = fixture.active("session_with_underscores");
        store.delete(&item, true, true).unwrap();
        let trashed = store.trash_items[0].clone();
        assert_eq!(trashed.id, item.id);
        fs::create_dir_all(&item.brain_path).unwrap();
        fs::write(item.brain_path.join("new-data"), "new conversation").unwrap();
        assert!(store.restore(&trashed).is_err());
        assert_eq!(
            fs::read_to_string(item.brain_path.join("new-data")).unwrap(),
            "new conversation"
        );
        assert_eq!(
            fs::read_to_string(&trashed.db_path).unwrap(),
            "old database"
        );
        assert!(trashed.transcript_path.unwrap().is_file());
    }

    #[test]
    fn restore_refuses_companion_directory_and_keeps_entire_backup() {
        let fixture = Fixture::new();
        let (mut store, item) = fixture.active("session");
        store.delete(&item, true, true).unwrap();
        let trashed = store.trash_items[0].clone();
        fs::create_dir(&item.db_path).unwrap();
        assert!(store.restore(&trashed).is_err());
        assert!(item.db_path.is_dir());
        assert!(!item.brain_path.exists());
        assert_eq!(
            fs::read_to_string(&trashed.db_path).unwrap(),
            "old database"
        );
        assert!(trashed.brain_path.is_dir());
    }

    #[test]
    fn restore_failure_to_create_parent_retains_backup() {
        let fixture = Fixture::new();
        let (mut store, item) = fixture.active("session");
        store.delete(&item, true, true).unwrap();
        let trashed = store.trash_items[0].clone();
        fs::remove_dir(&store.conv_dir).unwrap();
        fs::write(&store.conv_dir, "not a directory").unwrap();
        assert!(store.restore(&trashed).is_err());
        assert!(trashed.brain_path.is_dir());
        assert_eq!(
            fs::read_to_string(&trashed.db_path).unwrap(),
            "old database"
        );
        assert!(!item.brain_path.exists());
    }

    #[test]
    fn failed_later_move_rolls_back_earlier_moves() {
        let fixture = Fixture::new();
        let source = fixture.0.join("source");
        let moved = fixture.0.join("moved");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("child"), "recoverable").unwrap();
        let moves = vec![
            (source.clone(), moved.clone()),
            (source.join("child"), fixture.0.join("child")),
        ];
        assert!(move_all_or_rollback(&moves).is_err());
        assert_eq!(
            fs::read_to_string(source.join("child")).unwrap(),
            "recoverable"
        );
        assert!(!moved.exists());
    }

    #[test]
    fn external_delete_restore_only_moves_the_selected_provider() {
        let fixture = Fixture::new();
        let (mut store, agy) = fixture.active("same-id");
        let source = fixture.0.join("universal.json");
        fs::write(
            &source,
            "{\"messages\":[{\"role\":\"user\",\"content\":\"external prompt\"}]}",
        )
        .unwrap();
        let external = DialogueItem::new_external(
            "same-id".to_string(),
            AgentKind::Universal,
            "external".to_string(),
            None,
            1,
            0,
            0,
            Some(source.clone()),
        )
        .with_user_messages(vec!["external prompt".to_string()]);
        store.delete(&external, true, true).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read_to_string(&agy.db_path).unwrap(), "old database");
        assert!(agy.brain_path.is_dir());
        let trashed = store
            .trash_items
            .iter()
            .find(|it| it.agent == AgentKind::Universal)
            .unwrap()
            .clone();
        assert_eq!(
            store.load_conversation_steps(&trashed)[0].content,
            "external prompt"
        );
        fs::write(&source, "new external conversation").unwrap();
        assert!(store.restore(&trashed).is_err());
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            "new external conversation"
        );
        assert!(trashed.transcript_path.as_ref().unwrap().is_file());
        fs::remove_file(&source).unwrap();
        store.restore(&trashed).unwrap();
        assert!(source.is_file());
        assert!(store.trash_items.is_empty());
        assert_eq!(fs::read_to_string(&agy.db_path).unwrap(), "old database");
    }

    #[test]
    fn codex_wrapped_messages_are_visible_and_exported() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let source = fixture.0.join("rollout.jsonl");
        let records = [
            json!({"type":"session_meta", "payload":{"id":"session"}}),
            json!({"type":"response_item", "timestamp":"2025-03-04T05:06:07+02:00", "payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"question"}]}}),
            json!({"type":"response_item", "timestamp":"2025-03-04T05:06:08+02:00", "payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"answer"}]}}),
        ];
        fs::write(
            &source,
            records
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let item = DialogueItem::new_external(
            "session".to_string(),
            AgentKind::Codex,
            "title".to_string(),
            None,
            1,
            1,
            0,
            Some(source),
        );
        let steps = store.load_conversation_steps(&item);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].role, "user");
        assert_eq!(
            steps[0].timestamp.as_deref(),
            Some("2025-03-04T05:06:07+02:00")
        );
        assert_eq!(steps[1].role, "assistant");
        let output = fixture.0.join("export.md");
        store.export_to_markdown(&item, Some(&output)).unwrap();
        let markdown = fs::read_to_string(output).unwrap();
        assert!(markdown.contains("question"));
        assert!(markdown.contains("answer"));
        assert!(markdown.contains("OpenAI Codex"));
    }
}
