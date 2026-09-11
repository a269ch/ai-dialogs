use unicode_width::UnicodeWidthChar;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::canonical::AgentKind;
use crate::cleaner::format_bytes;
use crate::markdown::tui::render_markdown_tui;
use crate::models::{DialogueItem, DialogueStep};
use crate::providers::ProviderRegistry;
use crate::storage::DialogueStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    All,
    User,
    Subagent,
    Empty,
    Marked,
    Trash,
}

impl FilterMode {
    pub fn next(&self) -> Self {
        match self {
            Self::All => Self::User,
            Self::User => Self::Subagent,
            Self::Subagent => Self::Empty,
            Self::Empty => Self::Marked,
            Self::Marked => Self::Trash,
            Self::Trash => Self::All,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::User => "USER",
            Self::Subagent => "SUBAGENT",
            Self::Empty => "EMPTY",
            Self::Marked => "MARKED",
            Self::Trash => "TRASH",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Newest,
    Oldest,
    Size,
    Msgs,
}

impl SortMode {
    pub fn next(&self) -> Self {
        match self {
            Self::Newest => Self::Oldest,
            Self::Oldest => Self::Size,
            Self::Size => Self::Msgs,
            Self::Msgs => Self::Newest,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Newest => "NEWEST",
            Self::Oldest => "OLDEST",
            Self::Size => "SIZE",
            Self::Msgs => "MSGS",
        }
    }
}

pub enum ConfirmAction {
    DeleteSingle(DialogueItem),
    DeletePermanentSingle(DialogueItem),
    DeleteBatch(Vec<DialogueItem>),
    DeletePermanentBatch(Vec<DialogueItem>),
    CleanEmpty(Vec<DialogueItem>),
    EmptyTrash,
}

pub struct ConfirmDialog {
    pub message: String,
    pub is_destructive: bool,
    pub action: ConfirmAction,
}

pub struct TransferDialog {
    pub item: DialogueItem,
    pub target_options: Vec<AgentKind>,
    pub selected_idx: usize,
}

pub struct ViewerState {
    pub item: DialogueItem,
    pub steps: Vec<DialogueStep>,
    pub scroll_y: usize,
    pub scroll_x: usize,
    pub search_kw: String,
    pub is_searching: bool,
    pub search_input: String,
    pub user_jump_lines: Vec<usize>,
    pub assistant_jump_lines: Vec<usize>,
    pub rendered_lines: Vec<Line<'static>>,
    pub rendered_width: usize,
}

impl ViewerState {
    pub fn new(item: DialogueItem, steps: Vec<DialogueStep>, width: usize) -> Self {
        let mut s = Self {
            item,
            steps,
            scroll_y: 0,
            scroll_x: 0,
            search_kw: String::new(),
            is_searching: false,
            search_input: String::new(),
            user_jump_lines: Vec::new(),
            assistant_jump_lines: Vec::new(),
            rendered_lines: Vec::new(),
            rendered_width: 0,
        };
        s.rebuild_rendered_lines(width);
        s
    }

    pub fn rebuild_rendered_lines(&mut self, width: usize) {
        let content_w = width.max(30);
        self.rendered_width = content_w;
        self.user_jump_lines.clear();
        self.assistant_jump_lines.clear();

        let mut lines: Vec<Line<'static>> = Vec::new();

        let status_tag = if self.item.is_in_trash {
            " [IN TRASH]"
        } else {
            ""
        };
        lines.push(Line::from(Span::styled(
            format!(
                "=== [{}] DIALOGUE: {}{} ===",
                self.item.agent.short_tag(),
                self.item.topic,
                status_tag
            ),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "ID: {} | Agent: {} | Date: {} | Size: {}",
                self.item.id,
                self.item.agent.display_name(),
                self.item.date_str(),
                format_bytes(self.item.size_bytes)
            ),
            Style::default().fg(Color::DarkGray),
        )));
        if self.item.is_in_trash
            && let Some(ref tf) = self.item.trash_folder
        {
            lines.push(Line::from(Span::styled(
                format!(
                    "Deleted: {} | Path: {}",
                    self.item.deleted_date_str(),
                    tf.display()
                ),
                Style::default().fg(Color::Red),
            )));
        }
        lines.push(Line::from(Span::styled(
            format!(
                "Messages: {} user / {} assistant",
                self.item.user_msgs_count, self.item.model_msgs_count
            ),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));

        if self.steps.is_empty() {
            lines.push(Line::from(Span::styled(
                "Dialogue logs are empty or missing.",
                Style::default().fg(Color::DarkGray),
            )));
        }

        for step in &self.steps {
            let role = &step.role;
            let time_str = if !step.time.is_empty() {
                format!(" [{}]", step.time)
            } else {
                String::new()
            };

            if role == "user" {
                lines.push(Line::from(""));
                self.user_jump_lines.push(lines.len());
                let banner_len = content_w.saturating_sub(time_str.len() + 25);
                let banner = format!("── 👤 USER{} {}", time_str, "─".repeat(banner_len));
                lines.push(Line::from(Span::styled(
                    banner,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                let md_lines = render_markdown_tui(&step.content, content_w);
                lines.extend(md_lines);
                lines.push(Line::from(""));
            } else if role == "assistant" {
                lines.push(Line::from(""));
                self.assistant_jump_lines.push(lines.len());
                let agent_name = self.item.agent.short_tag();
                let banner_len = content_w.saturating_sub(time_str.len() + agent_name.len() + 10);
                let banner = format!(
                    "── 🤖 {}{} {}",
                    agent_name,
                    time_str,
                    "─".repeat(banner_len)
                );
                lines.push(Line::from(Span::styled(
                    banner,
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                )));
                let md_lines = render_markdown_tui(&step.content, content_w);
                lines.extend(md_lines);
                lines.push(Line::from(""));
            } else if role == "tool_call" {
                for tc in &step.tool_calls {
                    lines.push(Line::from(Span::styled(
                        format!("  [🛠️ Tool Call: {}]{}", tc.name, time_str),
                        Style::default().fg(Color::Yellow),
                    )));
                }
                lines.push(Line::from(""));
            }
        }

        self.rendered_lines = lines;
    }

    pub fn max_scroll_y(&self, content_height: usize) -> usize {
        self.rendered_lines.len().saturating_sub(content_height)
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_y = self.scroll_y.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: usize, content_height: usize) {
        let max_y = self.max_scroll_y(content_height);
        self.scroll_y = (self.scroll_y + n).min(max_y);
    }

    pub fn scroll_left(&mut self, n: usize) {
        self.scroll_x = self.scroll_x.saturating_sub(n);
    }

    pub fn scroll_right(&mut self, n: usize) {
        self.scroll_x += n;
    }

    pub fn jump_next_user(&mut self, content_height: usize) {
        let max_y = self.max_scroll_y(content_height);
        if let Some(&next_line) = self
            .user_jump_lines
            .iter()
            .find(|&&idx| idx > self.scroll_y)
        {
            self.scroll_y = next_line.min(max_y);
        } else if let Some(&first_line) = self.user_jump_lines.first() {
            self.scroll_y = first_line.min(max_y);
        }
    }

    pub fn jump_next_assistant(&mut self, content_height: usize) {
        let max_y = self.max_scroll_y(content_height);
        if let Some(&next_line) = self
            .assistant_jump_lines
            .iter()
            .find(|&&idx| idx > self.scroll_y)
        {
            self.scroll_y = next_line.min(max_y);
        } else if let Some(&first_line) = self.assistant_jump_lines.first() {
            self.scroll_y = first_line.min(max_y);
        }
    }

    pub fn find_next(&mut self, content_height: usize) {
        if self.search_kw.is_empty() {
            return;
        }
        let q = self.search_kw.to_lowercase();
        let max_y = self.max_scroll_y(content_height);

        for idx in (self.scroll_y + 1)..self.rendered_lines.len() {
            let full_text = line_to_string(&self.rendered_lines[idx]);
            if full_text.to_lowercase().contains(&q) {
                self.scroll_y = idx.min(max_y);
                return;
            }
        }
        for idx in 0..=self
            .scroll_y
            .min(self.rendered_lines.len().saturating_sub(1))
        {
            let full_text = line_to_string(&self.rendered_lines[idx]);
            if full_text.to_lowercase().contains(&q) {
                self.scroll_y = idx.min(max_y);
                return;
            }
        }
    }

    pub fn find_prev(&mut self, content_height: usize) {
        if self.search_kw.is_empty() {
            return;
        }
        let q = self.search_kw.to_lowercase();
        let max_y = self.max_scroll_y(content_height);

        if self.scroll_y > 0 {
            for idx in (0..self.scroll_y).rev() {
                let full_text = line_to_string(&self.rendered_lines[idx]);
                if full_text.to_lowercase().contains(&q) {
                    self.scroll_y = idx.min(max_y);
                    return;
                }
            }
        }
        for idx in (self.scroll_y..self.rendered_lines.len()).rev() {
            let full_text = line_to_string(&self.rendered_lines[idx]);
            if full_text.to_lowercase().contains(&q) {
                self.scroll_y = idx.min(max_y);
                return;
            }
        }
    }
}

pub struct App {
    pub store: DialogueStore,
    pub registry: ProviderRegistry,
    pub provider_filter: Option<AgentKind>,
    pub external_items: Vec<DialogueItem>,
    pub transfer_dialog: Option<TransferDialog>,
    pub selected_idx: usize,
    pub filter_mode: FilterMode,
    pub sort_mode: SortMode,
    pub search_query: String,
    pub is_searching: bool,
    pub search_input: String,
    pub status_msg: String,
    pub running: bool,
    pub show_help: bool,
    pub confirm: Option<ConfirmDialog>,
    pub viewer: Option<ViewerState>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let store = DialogueStore::new();
        let registry = ProviderRegistry::new();
        let mut app = Self {
            store,
            registry,
            provider_filter: None,
            external_items: Vec::new(),
            transfer_dialog: None,
            selected_idx: 0,
            filter_mode: FilterMode::All,
            sort_mode: SortMode::Newest,
            search_query: String::new(),
            is_searching: false,
            search_input: String::new(),
            status_msg: "Ready. Press [H] for help.".to_string(),
            running: true,
            show_help: false,
            confirm: None,
            viewer: None,
        };
        app.refresh_all();
        app
    }

    pub fn refresh_all(&mut self) {
        self.store.refresh();
        let mut ext = Vec::new();
        for kind in [
            AgentKind::Claude,
            AgentKind::Codex,
            AgentKind::Grok,
            AgentKind::Universal,
        ] {
            if let Some(p) = self.registry.get(kind)
                && p.is_available()
                && let Ok(items) = p.list_dialogues()
            {
                ext.extend(items);
            }
        }
        self.external_items = ext;
    }

    pub fn cycle_provider(&mut self) {
        self.provider_filter = match self.provider_filter {
            None => Some(AgentKind::Antigravity),
            Some(AgentKind::Antigravity) => Some(AgentKind::Claude),
            Some(AgentKind::Claude) => Some(AgentKind::Codex),
            Some(AgentKind::Codex) => Some(AgentKind::Grok),
            Some(AgentKind::Grok) => Some(AgentKind::Universal),
            Some(AgentKind::Universal) => None,
        };
        self.selected_idx = 0;
        self.status_msg = format!("Agent filter: [{}]", self.provider_name());
    }

    pub fn provider_name(&self) -> &'static str {
        match self.provider_filter {
            None => "ALL",
            Some(k) => k.short_tag(),
        }
    }

    pub fn is_trash_view(&self) -> bool {
        self.filter_mode == FilterMode::Trash
    }

    pub fn filtered_items(&self) -> Vec<DialogueItem> {
        let mut items = if self.is_trash_view() {
            self.store.trash_items.clone()
        } else {
            let mut list = match self.provider_filter {
                None => {
                    let mut combined = self.store.items.clone();
                    combined.extend(self.external_items.clone());
                    combined
                }
                Some(AgentKind::Antigravity) => self.store.items.clone(),
                Some(kind) => self
                    .external_items
                    .iter()
                    .filter(|it| it.agent == kind)
                    .cloned()
                    .collect(),
            };
            match self.filter_mode {
                FilterMode::User => {
                    list.retain(|it| !it.is_subagent && !it.is_empty);
                }
                FilterMode::Subagent => {
                    list.retain(|it| it.is_subagent);
                }
                FilterMode::Empty => {
                    list.retain(|it| it.is_empty);
                }
                FilterMode::Marked => {
                    list.retain(|it| it.is_marked);
                }
                _ => {}
            }
            list
        };

        if !self.search_query.is_empty() {
            let q = self.search_query.to_lowercase();
            items.retain(|it| {
                it.id.to_lowercase().contains(&q)
                    || it.topic.to_lowercase().contains(&q)
                    || it
                        .user_messages
                        .iter()
                        .any(|m| m.to_lowercase().contains(&q))
            });
        }

        match self.sort_mode {
            SortMode::Newest => {
                items.sort_by(|a, b| {
                    let a_dt = if self.is_trash_view() {
                        a.deleted_at.as_deref().unwrap_or("0")
                    } else {
                        a.created_at.as_deref().unwrap_or("0")
                    };
                    let b_dt = if self.is_trash_view() {
                        b.deleted_at.as_deref().unwrap_or("0")
                    } else {
                        b.created_at.as_deref().unwrap_or("0")
                    };
                    b_dt.cmp(a_dt)
                });
            }
            SortMode::Oldest => {
                items.sort_by(|a, b| {
                    let a_dt = if self.is_trash_view() {
                        a.deleted_at.as_deref().unwrap_or("0")
                    } else {
                        a.created_at.as_deref().unwrap_or("0")
                    };
                    let b_dt = if self.is_trash_view() {
                        b.deleted_at.as_deref().unwrap_or("0")
                    } else {
                        b.created_at.as_deref().unwrap_or("0")
                    };
                    a_dt.cmp(b_dt)
                });
            }
            SortMode::Size => {
                items.sort_by_key(|b| std::cmp::Reverse(b.size_bytes));
            }
            SortMode::Msgs => {
                items.sort_by_key(|b| std::cmp::Reverse(b.user_msgs_count));
            }
        }

        items
    }

    pub fn open_viewer(&mut self, item: DialogueItem, term_width: usize) {
        let steps = self.store.load_conversation_steps(&item);
        self.viewer = Some(ViewerState::new(item, steps, term_width));
    }

    pub fn close_viewer(&mut self) {
        self.viewer = None;
    }

    pub fn toggle_mark_selected(&mut self) {
        let items = self.filtered_items();
        if items.is_empty() || self.selected_idx >= items.len() {
            return;
        }
        let target_id = items[self.selected_idx].id.clone();
        let is_trash = self.is_trash_view();
        let mut marked_state = None;

        if let Some(it) = self
            .store
            .items_mut(is_trash)
            .iter_mut()
            .find(|x| x.id == target_id)
        {
            it.is_marked = !it.is_marked;
            marked_state = Some(it.is_marked);
        } else if let Some(it) = self.external_items.iter_mut().find(|x| x.id == target_id) {
            it.is_marked = !it.is_marked;
            marked_state = Some(it.is_marked);
        }

        if let Some(marked) = marked_state {
            let short_id: String = target_id.chars().take(8).collect();
            self.status_msg = format!(
                "Dialogue {} {}.",
                short_id,
                if marked { "marked" } else { "unmarked" }
            );
        }

        if self.selected_idx + 1 < items.len() {
            self.selected_idx += 1;
        }
    }

    pub fn toggle_mark_all(&mut self) {
        let items = self.filtered_items();
        if items.is_empty() {
            return;
        }
        let all_marked = items.iter().all(|it| it.is_marked);
        let new_state = !all_marked;
        let is_trash = self.is_trash_view();

        for item in &items {
            let id = &item.id;
            if let Some(it) = self
                .store
                .items_mut(is_trash)
                .iter_mut()
                .find(|x| &x.id == id)
            {
                it.is_marked = new_state;
            }
            if let Some(it) = self.external_items.iter_mut().find(|x| &x.id == id) {
                it.is_marked = new_state;
            }
        }

        self.status_msg = if new_state {
            format!("Marked all ({} items).", items.len())
        } else {
            "Unmarked all items.".to_string()
        };
    }

    pub fn prompt_transfer(&mut self) {
        let items = self.filtered_items();
        if items.is_empty() || self.selected_idx >= items.len() {
            self.status_msg = "No dialogue selected to transfer.".to_string();
            return;
        }
        let item = items[self.selected_idx].clone();
        let current_agent = item.agent;
        let target_options: Vec<AgentKind> = AgentKind::ALL
            .iter()
            .copied()
            .filter(|&k| k != current_agent)
            .collect();
        if target_options.is_empty() {
            self.status_msg = "No available target agents.".to_string();
            return;
        }
        self.transfer_dialog = Some(TransferDialog {
            item,
            target_options,
            selected_idx: 0,
        });
    }

    pub fn execute_transfer(&mut self) {
        let Some(dialog) = self.transfer_dialog.take() else {
            return;
        };
        if dialog.selected_idx >= dialog.target_options.len() {
            return;
        }
        let target_agent = dialog.target_options[dialog.selected_idx];
        let source_agent = dialog.item.agent;
        let short_id: String = dialog.item.id.chars().take(8).collect();

        match crate::transfer::TransferEngine::transfer(
            &self.registry,
            source_agent,
            target_agent,
            &dialog.item.id,
        ) {
            Ok(res) => {
                let new_short: String = res.target_id.chars().take(8).collect();
                self.status_msg = format!(
                    "Transferred {} from {} to {} (New ID: {}).",
                    short_id,
                    source_agent.display_name(),
                    target_agent.display_name(),
                    new_short
                );
                self.refresh_all();
            }
            Err(e) => {
                self.status_msg = format!("Transfer failed: {}", e);
            }
        }
    }

    pub fn prompt_delete_selected(&mut self) {
        let items = self.filtered_items();
        let is_trash = self.is_trash_view();
        let marked: Vec<DialogueItem> = self
            .store
            .items(is_trash)
            .iter()
            .filter(|it| it.is_marked)
            .cloned()
            .collect();

        if !marked.is_empty() {
            if is_trash {
                self.confirm = Some(ConfirmDialog {
                    message: format!(
                        "⛔ PERMANENTLY DELETE {} marked dialogues from trash? [y/N]",
                        marked.len()
                    ),
                    is_destructive: true,
                    action: ConfirmAction::DeletePermanentBatch(marked),
                });
            } else {
                self.confirm = Some(ConfirmDialog {
                    message: format!("⚠️ Move {} marked dialogues to trash? [y/N]", marked.len()),
                    is_destructive: false,
                    action: ConfirmAction::DeleteBatch(marked),
                });
            }
        } else if !items.is_empty() && self.selected_idx < items.len() {
            let target = items[self.selected_idx].clone();
            let short_id: String = target.id.chars().take(8).collect();
            if is_trash {
                self.confirm = Some(ConfirmDialog {
                    message: format!(
                        "⛔ PERMANENTLY DELETE dialogue {} from trash? [y/N]",
                        short_id
                    ),
                    is_destructive: true,
                    action: ConfirmAction::DeletePermanentSingle(target),
                });
            } else {
                self.confirm = Some(ConfirmDialog {
                    message: format!("⚠️ Move dialogue {} to trash? [y/N]", short_id),
                    is_destructive: false,
                    action: ConfirmAction::DeleteSingle(target),
                });
            }
        }
    }

    pub fn prompt_batch_delete(&mut self) {
        let is_trash = self.is_trash_view();
        let marked: Vec<DialogueItem> = self
            .store
            .items(is_trash)
            .iter()
            .filter(|it| it.is_marked)
            .cloned()
            .collect();

        if marked.is_empty() {
            self.status_msg = if is_trash {
                "No marked dialogues in trash! (Use Space to select).".to_string()
            } else {
                "No marked dialogues! (Use Space to select).".to_string()
            };
        } else if is_trash {
            self.confirm = Some(ConfirmDialog {
                message: format!(
                    "⛔ PERMANENTLY DELETE {} marked dialogues from trash? [y/N]",
                    marked.len()
                ),
                is_destructive: true,
                action: ConfirmAction::DeletePermanentBatch(marked),
            });
        } else {
            self.confirm = Some(ConfirmDialog {
                message: format!("⚠️ Move {} marked dialogues to trash? [y/N]", marked.len()),
                is_destructive: false,
                action: ConfirmAction::DeleteBatch(marked),
            });
        }
    }

    pub fn prompt_clean_or_empty(&mut self) {
        if self.is_trash_view() {
            let count = self.store.trash_items.len();
            if count == 0 {
                self.status_msg = "Trash is already empty.".to_string();
            } else {
                self.confirm = Some(ConfirmDialog {
                    message: format!("⛔ COMPLETELY EMPTY TRASH ({} dialogues)? [y/N]", count),
                    is_destructive: true,
                    action: ConfirmAction::EmptyTrash,
                });
            }
        } else {
            let empty_items: Vec<DialogueItem> = self
                .store
                .items
                .iter()
                .filter(|it| it.is_empty)
                .cloned()
                .collect();
            if empty_items.is_empty() {
                self.status_msg = "No empty sessions detected.".to_string();
            } else {
                self.confirm = Some(ConfirmDialog {
                    message: format!(
                        "⚠️ Move {} empty sessions to trash? [y/N]",
                        empty_items.len()
                    ),
                    is_destructive: false,
                    action: ConfirmAction::CleanEmpty(empty_items),
                });
            }
        }
    }

    pub fn restore_selected(&mut self) {
        if !self.is_trash_view() {
            self.status_msg =
                "Restore is only available in trash mode [TRASH] (press Tab or T).".to_string();
            return;
        }
        let items = self.filtered_items();
        if items.is_empty() || self.selected_idx >= items.len() {
            return;
        }
        let target = items[self.selected_idx].clone();
        let short_id: String = target.id.chars().take(8).collect();
        match self.store.restore(&target) {
            Ok(()) => {
                self.status_msg = format!("✅ Dialogue {} successfully restored!", short_id);
                let new_len = self.filtered_items().len();
                if self.selected_idx >= new_len {
                    self.selected_idx = new_len.saturating_sub(1);
                }
            }
            Err(e) => {
                self.status_msg = format!("Error restoring {}: {}", short_id, e);
            }
        }
    }

    pub fn export_selected(&mut self) {
        let items = self.filtered_items();
        if items.is_empty() || self.selected_idx >= items.len() {
            return;
        }
        let target = &items[self.selected_idx];
        match self.store.export_to_markdown(target, None) {
            Ok(p) => {
                self.status_msg = format!("Exported to: {}", p.display());
            }
            Err(e) => {
                self.status_msg = format!("Export error: {}", e);
            }
        }
    }

    pub fn execute_confirm(&mut self) {
        let Some(dialog) = self.confirm.take() else {
            return;
        };

        match dialog.action {
            ConfirmAction::DeleteSingle(item) => {
                let short_id: String = item.id.chars().take(8).collect();
                match self.store.delete(&item, true, true) {
                    Ok(()) => {
                        self.status_msg =
                            format!("Dialogue {} moved to trash (press [T] to view).", short_id);
                    }
                    Err(e) => self.status_msg = format!("Error deleting {}: {}", short_id, e),
                }
            }
            ConfirmAction::DeletePermanentSingle(item) => {
                let short_id: String = item.id.chars().take(8).collect();
                match self.store.delete_permanently(&item, true) {
                    Ok(()) => {
                        self.status_msg = format!("Dialogue {} permanently deleted.", short_id);
                    }
                    Err(e) => {
                        self.status_msg = format!("Error permanently deleting {}: {}", short_id, e)
                    }
                }
            }
            ConfirmAction::DeleteBatch(marked) => {
                let count = self.store.delete_batch(&marked, true);
                self.status_msg = format!("Moved {} dialogues to trash.", count);
            }
            ConfirmAction::DeletePermanentBatch(marked) => {
                let count = self.store.delete_permanently_batch(&marked);
                self.status_msg = format!("Permanently deleted {} dialogues from trash.", count);
            }
            ConfirmAction::CleanEmpty(empty) => {
                let count = self.store.delete_batch(&empty, true);
                self.status_msg = format!("Moved {} empty sessions to trash.", count);
            }
            ConfirmAction::EmptyTrash => match self.store.empty_trash() {
                Ok(count) => {
                    self.status_msg =
                        format!("Trash completely emptied (deleted {} folders).", count);
                    self.selected_idx = 0;
                }
                Err(e) => self.status_msg = format!("Error emptying trash: {}", e),
            },
        }

        self.refresh_all();
        let new_len = self.filtered_items().len();
        if self.selected_idx >= new_len {
            self.selected_idx = new_len.saturating_sub(1);
        }
    }
}

pub fn line_to_string(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn slice_spans(spans: &[Span], scroll_x: usize, max_cols: usize) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut current_col = 0;
    let end_col = scroll_x + max_cols;

    for span in spans {
        let span_str = span.content.as_ref();
        let span_style = span.style;
        let mut slice = String::new();

        for ch in span_str.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
            let next_col = current_col + cw;

            if next_col > scroll_x && current_col < end_col {
                slice.push(ch);
            }

            current_col = next_col;
            if current_col >= end_col {
                break;
            }
        }

        if !slice.is_empty() {
            result.push(Span::styled(slice, span_style));
        }

        if current_col >= end_col {
            break;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_mode_cycle() {
        let m = FilterMode::All;
        assert_eq!(m.next(), FilterMode::User);
        assert_eq!(m.next().next(), FilterMode::Subagent);
        assert_eq!(m.next().next().next(), FilterMode::Empty);
        assert_eq!(m.next().next().next().next(), FilterMode::Marked);
        assert_eq!(m.next().next().next().next().next(), FilterMode::Trash);
        assert_eq!(m.next().next().next().next().next().next(), FilterMode::All);
    }

    #[test]
    fn test_sort_mode_cycle() {
        let s = SortMode::Newest;
        assert_eq!(s.next(), SortMode::Oldest);
        assert_eq!(s.next().next(), SortMode::Size);
        assert_eq!(s.next().next().next(), SortMode::Msgs);
        assert_eq!(s.next().next().next().next(), SortMode::Newest);
    }

    #[test]
    fn test_cycle_provider() {
        let mut app = App::new();
        assert_eq!(app.provider_name(), "ALL");
        app.cycle_provider();
        assert_eq!(app.provider_name(), "AGY");
        app.cycle_provider();
        assert_eq!(app.provider_name(), "CLAUDE");
        app.cycle_provider();
        assert_eq!(app.provider_name(), "CODEX");
        app.cycle_provider();
        assert_eq!(app.provider_name(), "GROK");
        app.cycle_provider();
        assert_eq!(app.provider_name(), "UNIV");
        app.cycle_provider();
        assert_eq!(app.provider_name(), "ALL");
    }
}
