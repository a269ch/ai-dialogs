use crate::cleaner::{clean_user_content, format_bytes, is_system_noise};
use crate::error::{AppError, Result};
use crate::models::{DialogueItem, DialogueStep, ToolCall};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File};
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
                if folder.is_dir()
                    && let Some(folder_name) = folder.file_name().and_then(|s| s.to_str())
                {
                    let cid = folder_name.split('_').next().unwrap_or("").to_string();
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
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AppError::General(e.to_string()))?
            .as_secs();

        let trash_subfolder = self.trash_dir.join(format!("{}_{}", item.id, timestamp));

        if use_trash {
            fs::create_dir_all(&trash_subfolder)?;
        }

        if item.brain_path.is_dir() {
            if use_trash {
                fs::rename(&item.brain_path, trash_subfolder.join("brain"))?;
            } else {
                let _ = fs::remove_dir_all(&item.brain_path);
            }
        }

        let prefix = format!("{}.db", item.id);
        if self.conv_dir.is_dir()
            && let Ok(entries) = fs::read_dir(&self.conv_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(fname) = path.file_name().and_then(|s| s.to_str())
                    && fname.starts_with(&prefix)
                {
                    if use_trash {
                        let _ = fs::rename(&path, trash_subfolder.join(fname));
                    } else {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }

        for path in [&item.annot_path, &item.presence_path] {
            if path.exists() {
                if use_trash {
                    let fname = path.file_name().unwrap_or_default();
                    let _ = fs::rename(path, trash_subfolder.join(fname));
                } else {
                    let _ = fs::remove_file(path);
                }
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

        fs::create_dir_all(&self.brain_dir)?;
        fs::create_dir_all(&self.conv_dir)?;
        fs::create_dir_all(&self.annot_dir)?;
        fs::create_dir_all(&self.presence_dir)?;

        let trash_brain = folder.join("brain");
        if trash_brain.is_dir() {
            let target_brain = self.brain_dir.join(&item.id);
            if target_brain.exists() {
                let _ = fs::remove_dir_all(&target_brain);
            }
            fs::rename(&trash_brain, target_brain)?;
        }

        if let Ok(entries) = fs::read_dir(folder) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                    let target_dir = if fname.ends_with(".db")
                        || fname.ends_with(".db-wal")
                        || fname.ends_with(".db-shm")
                    {
                        Some(&self.conv_dir)
                    } else if fname.ends_with(".pbtxt") {
                        Some(&self.annot_dir)
                    } else if fname.ends_with(".lock") {
                        Some(&self.presence_dir)
                    } else {
                        None
                    };

                    if let Some(dir) = target_dir {
                        let dst = dir.join(fname);
                        if dst.exists() {
                            let _ = fs::remove_file(&dst);
                        }
                        let _ = fs::rename(&p, dst);
                    }
                }
            }
        }

        let _ = fs::remove_dir_all(folder);
        self.refresh();
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
            for entry in fs::read_dir(&self.trash_dir)?.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                } else {
                    let _ = fs::remove_file(&path);
                }
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
                let short_id = if item.id.len() >= 8 {
                    &item.id[..8]
                } else {
                    &item.id
                };
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
                        "### 🤖 Antigravity{}\n\n{}\n\n---\n",
                        time_str, step.content
                    )?;
                }
                "tool_call" => {
                    for tc in step.tool_calls {
                        writeln!(file, "> 🛠️ **Tool Call `{}`**{}\n\n", tc.name, time_str)?;
                    }
                    writeln!(file, "---\n")?;
                }
                _ => {}
            }
        }

        Ok(out_path)
    }
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
        } else if parsed.is_model()
            && !parsed.content.is_empty()
            && !is_system_noise(&parsed.content)
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
        let time = if dt.len() >= 19 {
            dt[11..19].to_string()
        } else {
            String::new()
        };

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

        if stype.is_empty() && ssrc.is_empty() {
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
        if let Some(tcs) = val.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tcs {
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let args = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|a| a.as_str())
                    .unwrap_or("")
                    .to_string();
                tool_calls.push(ToolCall { name, args });
            }
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

    fn into_dialogue_step(self) -> Option<DialogueStep> {
        if self.is_user() {
            let cleaned = clean_user_content(&self.content);
            if !cleaned.is_empty() {
                return Some(DialogueStep {
                    role: "user".to_string(),
                    time: self.time,
                    content: cleaned,
                    tool_calls: Vec::new(),
                });
            }
        } else if self.is_model() {
            if !self.content.is_empty() && !is_system_noise(&self.content) {
                return Some(DialogueStep {
                    role: "assistant".to_string(),
                    time: self.time,
                    content: self.content.trim().to_string(),
                    tool_calls: self.tool_calls,
                });
            } else if !self.tool_calls.is_empty() {
                return Some(DialogueStep {
                    role: "tool_call".to_string(),
                    time: self.time,
                    content: String::new(),
                    tool_calls: self.tool_calls,
                });
            }
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
