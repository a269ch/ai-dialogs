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
    pub content_height: usize,
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
            content_height: 1,
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
            } else if role == "tool_call" || role == "system" {
                lines.push(Line::from(format!(
                    "── {}{}",
                    if role == "system" { "SYSTEM" } else { "TOOL" },
                    time_str
                )));
                lines.extend(render_markdown_tui(&step.content, content_w));
                lines.push(Line::from(""));
            }
            for tc in &step.tool_calls {
                lines.push(Line::from(Span::styled(
                    format!("  [🛠️ Tool Call: {}]{}", tc.name, time_str),
                    Style::default().fg(Color::Yellow),
                )));
                lines.extend(render_markdown_tui(&tc.args, content_w));
                if let Some(result) = &tc.result {
                    lines.extend(render_markdown_tui(result, content_w));
                }
                lines.push(Line::from(""));
            }
        }

        self.rendered_lines = lines;
    }

    pub fn max_scroll_y(&self, content_height: usize) -> usize {
        self.rendered_lines
            .len()
            .saturating_sub(content_height.max(1))
    }

    pub fn set_viewport(&mut self, width: usize, height: usize) {
        if self.rendered_width != width.max(30) {
            self.rebuild_rendered_lines(width);
        }
        self.content_height = height;
        self.scroll_y = self.scroll_y.min(self.max_scroll_y(height));
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_y = self.scroll_y.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: usize, content_height: usize) {
        let max_y = self.max_scroll_y(content_height);
        self.scroll_y = self.scroll_y.saturating_add(n).min(max_y);
    }

    pub fn scroll_left(&mut self, n: usize) {
        self.scroll_x = self.scroll_x.saturating_sub(n);
    }

    pub fn scroll_right(&mut self, n: usize) {
        self.scroll_x = self.scroll_x.saturating_add(n);
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
        if self.search_kw.is_empty() || self.rendered_lines.is_empty() {
            return;
        }
        let max_y = self.max_scroll_y(content_height);

        for idx in (self.scroll_y + 1)..self.rendered_lines.len() {
            let full_text = line_to_string(&self.rendered_lines[idx]);
            if !case_insensitive_match_ranges(&full_text, &self.search_kw).is_empty() {
                self.scroll_y = idx.min(max_y);
                return;
            }
        }
        for idx in 0..=self
            .scroll_y
            .min(self.rendered_lines.len().saturating_sub(1))
        {
            let full_text = line_to_string(&self.rendered_lines[idx]);
            if !case_insensitive_match_ranges(&full_text, &self.search_kw).is_empty() {
                self.scroll_y = idx.min(max_y);
                return;
            }
        }
    }

    pub fn find_prev(&mut self, content_height: usize) {
        if self.search_kw.is_empty() || self.rendered_lines.is_empty() {
            return;
        }
        let max_y = self.max_scroll_y(content_height);

        if self.scroll_y > 0 {
            for idx in (0..self.scroll_y).rev() {
                let full_text = line_to_string(&self.rendered_lines[idx]);
                if !case_insensitive_match_ranges(&full_text, &self.search_kw).is_empty() {
                    self.scroll_y = idx.min(max_y);
                    return;
                }
            }
        }
        for idx in (self.scroll_y..self.rendered_lines.len()).rev() {
            let full_text = line_to_string(&self.rendered_lines[idx]);
            if !case_insensitive_match_ranges(&full_text, &self.search_kw).is_empty() {
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
    pub resume_target: Option<DialogueItem>,
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
            status_msg: "Ready".to_string(),
            running: true,
            show_help: false,
            confirm: None,
            viewer: None,
            resume_target: None,
        };
        app.refresh_all();
        app
    }

    pub fn refresh_all(&mut self) {
        let marked: Vec<_> = self
            .store
            .items
            .iter()
            .chain(&self.store.trash_items)
            .chain(&self.external_items)
            .filter(|it| it.is_marked)
            .cloned()
            .collect();
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
        for item in self
            .store
            .items
            .iter_mut()
            .chain(&mut self.store.trash_items)
            .chain(&mut self.external_items)
        {
            item.is_marked = marked.iter().any(|previous| same_item(item, previous));
        }
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

        if let Some(kind) = self.provider_filter {
            items.retain(|it| it.agent == kind);
        }

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
        let target = &items[self.selected_idx];
        let is_trash = self.is_trash_view();
        let mut marked_state = None;

        if let Some(it) = self
            .store
            .items_mut(is_trash)
            .iter_mut()
            .find(|x| same_item(x, target))
        {
            it.is_marked = !it.is_marked;
            marked_state = Some(it.is_marked);
        } else if !is_trash
            && let Some(it) = self
                .external_items
                .iter_mut()
                .find(|x| same_item(x, target))
        {
            it.is_marked = !it.is_marked;
            marked_state = Some(it.is_marked);
        }

        if let Some(marked) = marked_state {
            let short_id: String = target.id.chars().take(8).collect();
            self.status_msg = format!(
                "Dialogue {} {}.",
                short_id,
                if marked { "marked" } else { "unmarked" }
            );
        }

        let new_len = self.filtered_items().len();
        if self.filter_mode != FilterMode::Marked && self.selected_idx + 1 < new_len {
            self.selected_idx += 1;
        } else {
            self.selected_idx = self.selected_idx.min(new_len.saturating_sub(1));
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
            if let Some(it) = self
                .store
                .items_mut(is_trash)
                .iter_mut()
                .find(|x| same_item(x, item))
            {
                it.is_marked = new_state;
            }
            if !is_trash
                && let Some(it) = self.external_items.iter_mut().find(|x| same_item(x, item))
            {
                it.is_marked = new_state;
            }
        }

        self.status_msg = if new_state {
            format!("Marked all ({} items).", items.len())
        } else {
            "Unmarked all items.".to_string()
        };
        self.selected_idx = self
            .selected_idx
            .min(self.filtered_items().len().saturating_sub(1));
    }

    pub fn prompt_transfer(&mut self) {
        if self.is_trash_view() {
            self.status_msg = "Restore the dialogue before transferring it.".to_string();
            return;
        }
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
        let marked = self.marked_items();

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
        let marked = self.marked_items();

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

    fn marked_items(&self) -> Vec<DialogueItem> {
        let is_trash = self.is_trash_view();
        self.store
            .items(is_trash)
            .iter()
            .chain(self.external_items.iter().filter(|_| !is_trash))
            .filter(|it| it.is_marked)
            .cloned()
            .collect()
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
                .filtered_items()
                .into_iter()
                .filter(|it| it.is_empty)
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
                self.refresh_all();
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
                match self.store.delete(&item, true, false) {
                    Ok(()) => {
                        self.status_msg =
                            format!("Dialogue {} moved to trash (press [T] to view).", short_id);
                    }
                    Err(e) => self.status_msg = format!("Error deleting {}: {}", short_id, e),
                }
            }
            ConfirmAction::DeletePermanentSingle(item) => {
                let short_id: String = item.id.chars().take(8).collect();
                match self.store.delete_permanently(&item, false) {
                    Ok(()) => {
                        self.status_msg = format!("Dialogue {} permanently deleted.", short_id);
                    }
                    Err(e) => {
                        self.status_msg = format!("Error permanently deleting {}: {}", short_id, e)
                    }
                }
            }
            ConfirmAction::DeleteBatch(marked) => {
                self.delete_batch_with_status(&marked, false);
            }
            ConfirmAction::DeletePermanentBatch(marked) => {
                self.delete_batch_with_status(&marked, true);
            }
            ConfirmAction::CleanEmpty(empty) => {
                self.delete_batch_with_status(&empty, false);
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

    fn delete_batch_with_status(&mut self, items: &[DialogueItem], permanent: bool) {
        let mut count = 0;
        let mut failures = Vec::new();
        for item in items {
            let result = if permanent {
                self.store.delete_permanently(item, false)
            } else {
                self.store.delete(item, true, false)
            };
            match result {
                Ok(()) => count += 1,
                Err(error) => {
                    failures.push(format!("{} {}: {}", item.agent.short_tag(), item.id, error))
                }
            }
        }
        self.status_msg = if permanent {
            format!("Permanently deleted {} dialogues from trash.", count)
        } else {
            format!("Moved {} dialogues to trash.", count)
        };
        if let Some(first_error) = failures.first() {
            self.status_msg
                .push_str(&format!(" Failed: {}. {}", failures.len(), first_error));
        }
    }
}

pub fn line_to_string(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn same_item(left: &DialogueItem, right: &DialogueItem) -> bool {
    left.agent == right.agent
        && left.id == right.id
        && left.is_in_trash == right.is_in_trash
        && left.trash_folder == right.trash_folder
}

pub fn case_insensitive_match_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let query: String = query.chars().flat_map(char::to_lowercase).collect();
    let mut folded = String::new();
    let mut source_ranges = Vec::new();
    for (start, ch) in text.char_indices() {
        let end = start + ch.len_utf8();
        for folded_ch in ch.to_lowercase() {
            folded.push(folded_ch);
            source_ranges.extend(std::iter::repeat_n(start..end, folded_ch.len_utf8()));
        }
    }
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for (start, matched) in folded.match_indices(&query) {
        let range = source_ranges[start].start..source_ranges[start + matched.len() - 1].end;
        if let Some(previous) = ranges.last_mut()
            && range.start < previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            ranges.push(range);
        }
    }
    ranges
}

pub fn slice_spans(spans: &[Span], scroll_x: usize, max_cols: usize) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut current_col = 0;
    let end_col = scroll_x.saturating_add(max_cols);

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
pub(super) fn test_app(base: &std::path::Path) -> App {
    App {
        store: DialogueStore {
            base_dir: base.to_path_buf(),
            brain_dir: base.join("brain"),
            conv_dir: base.join("conversations"),
            annot_dir: base.join("annotations"),
            presence_dir: base.join("presence"),
            summaries_db: base.join("conversation_summaries.db"),
            trash_dir: base.join("trash"),
            items: Vec::new(),
            trash_items: Vec::new(),
        },
        registry: ProviderRegistry::empty(),
        provider_filter: None,
        external_items: Vec::new(),
        transfer_dialog: None,
        selected_idx: 0,
        filter_mode: FilterMode::All,
        sort_mode: SortMode::Newest,
        search_query: String::new(),
        is_searching: false,
        search_input: String::new(),
        status_msg: String::new(),
        running: true,
        show_help: false,
        confirm: None,
        viewer: None,
        resume_target: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_displays_tool_only_assistant_turns_and_system_context() {
        let item = DialogueItem::new_external(
            "id".into(),
            AgentKind::Claude,
            "tools".into(),
            None,
            0,
            1,
            0,
            None,
        );
        let steps = vec![
            DialogueStep {
                role: "assistant".into(),
                time: String::new(),
                timestamp: None,
                content: String::new(),
                tool_calls: vec![crate::models::ToolCall {
                    name: "read_file".into(),
                    args: "main.rs".into(),
                    result: Some("file contents".into()),
                }],
            },
            DialogueStep {
                role: "system".into(),
                time: String::new(),
                timestamp: None,
                content: "System context".into(),
                tool_calls: vec![],
            },
        ];
        let viewer = ViewerState::new(item, steps, 80);
        let text = viewer
            .rendered_lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        for needle in ["read_file", "main.rs", "file contents", "System context"] {
            assert!(text.contains(needle), "missing {needle}");
        }
    }
    use crate::canonical::CanonicalDialogue;
    use crate::providers::universal::UniversalProvider;
    use crate::test_support::TestDir;
    use std::fs;

    fn item(agent: AgentKind) -> DialogueItem {
        DialogueItem::new_external(
            "shared-id".into(),
            agent,
            "test".into(),
            None,
            1,
            1,
            0,
            None,
        )
    }

    #[test]
    fn marks_use_provider_and_id_for_single_and_select_all() {
        let mut app = test_app(std::path::Path::new("unused-test-path"));
        app.store.items.push(item(AgentKind::Antigravity));
        app.external_items = vec![item(AgentKind::Claude), item(AgentKind::Universal)];
        app.provider_filter = Some(AgentKind::Universal);

        app.toggle_mark_selected();
        assert!(!app.store.items[0].is_marked);
        assert!(!app.external_items[0].is_marked);
        assert!(app.external_items[1].is_marked);

        app.toggle_mark_all();
        assert!(!app.external_items[1].is_marked);
        app.toggle_mark_all();
        assert!(!app.store.items[0].is_marked);
        assert!(!app.external_items[0].is_marked);
        assert!(app.external_items[1].is_marked);
    }

    #[test]
    fn marked_batches_include_external_providers() {
        let mut app = test_app(std::path::Path::new("unused-test-path"));
        app.store.items.push(item(AgentKind::Antigravity));
        app.external_items = vec![item(AgentKind::Claude), item(AgentKind::Universal)];
        app.toggle_mark_all();
        app.prompt_batch_delete();
        let ConfirmAction::DeleteBatch(items) = app.confirm.take().unwrap().action else {
            panic!("expected batch deletion");
        };
        assert_eq!(items.len(), 3);
        assert!(items.iter().any(|it| it.agent == AgentKind::Universal));

        app.store.items[0].is_marked = false;
        app.prompt_delete_selected();
        let ConfirmAction::DeleteBatch(items) = app.confirm.take().unwrap().action else {
            panic!("expected marked external batch deletion");
        };
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|it| it.agent != AgentKind::Antigravity));
    }

    #[test]
    fn external_delete_and_restore_preserve_same_id_antigravity_source() {
        for marked in [false, true] {
            let dir = TestDir::new("tui-delete");
            let mut app = test_app(&dir.path().join("antigravity"));
            fs::create_dir_all(app.store.brain_dir.join("shared-id")).unwrap();
            fs::create_dir_all(&app.store.conv_dir).unwrap();
            let source_database = app.store.conv_dir.join("shared-id.db");
            fs::write(&source_database, "precious source database").unwrap();
            let sessions = dir.path().join("universal");
            fs::create_dir(&sessions).unwrap();
            let transcript = sessions.join("shared-id.json");
            let dialogue = CanonicalDialogue::new("shared-id", "test", AgentKind::Universal);
            let bytes = serde_json::to_vec(&dialogue).unwrap();
            fs::write(&transcript, &bytes).unwrap();
            app.registry
                .register(Box::new(UniversalProvider::with_base_dir(sessions)));
            app.refresh_all();
            app.provider_filter = Some(AgentKind::Universal);
            assert_eq!(app.filtered_items().len(), 1);

            if marked {
                app.toggle_mark_selected();
                app.prompt_batch_delete();
            } else {
                app.prompt_delete_selected();
            }
            app.execute_confirm();
            assert!(app.status_msg.contains("trash"), "{}", app.status_msg);
            assert!(!transcript.exists());
            assert_eq!(
                fs::read_to_string(&source_database).unwrap(),
                "precious source database"
            );
            assert!(app.store.brain_dir.join("shared-id").is_dir());
            assert!(app.external_items.is_empty());

            app.filter_mode = FilterMode::Trash;
            let trash = app.filtered_items();
            assert_eq!(trash.len(), 1);
            assert_eq!(trash[0].agent, AgentKind::Universal);
            app.restore_selected();
            assert!(
                app.status_msg.contains("successfully restored"),
                "{}",
                app.status_msg
            );
            assert_eq!(fs::read(&transcript).unwrap(), bytes);
            assert!(app.store.trash_items.is_empty());
            assert_eq!(app.external_items.len(), 1);
        }
    }

    #[test]
    fn batch_delete_reports_failures() {
        let dir = TestDir::new("tui-delete-failure");
        let mut app = test_app(dir.path());
        let mut missing = item(AgentKind::Universal);
        missing.transcript_path = Some(dir.path().join("missing.json"));
        app.delete_batch_with_status(&[missing], false);
        assert!(app.status_msg.contains("Moved 0 dialogues"));
        assert!(app.status_msg.contains("Failed: 1"));
        assert!(app.status_msg.contains("UNIV shared-id"));
    }

    #[test]
    fn trash_transfer_requires_restoring_the_selected_backup() {
        let mut app = test_app(std::path::Path::new("unused-test-path"));
        app.filter_mode = FilterMode::Trash;
        app.store.trash_items.push(item(AgentKind::Universal));
        app.prompt_transfer();
        assert!(app.transfer_dialog.is_none());
        assert!(app.status_msg.contains("Restore"));
    }

    #[test]
    fn trash_filter_and_marks_distinguish_repeated_backups() {
        let mut app = test_app(std::path::Path::new("unused-test-path"));
        let mut first = item(AgentKind::Universal);
        first.is_in_trash = true;
        first.trash_folder = Some("first-backup".into());
        let mut second = first.clone();
        second.trash_folder = Some("second-backup".into());
        app.store.trash_items = vec![item(AgentKind::Claude), first, second];
        app.filter_mode = FilterMode::Trash;
        app.provider_filter = Some(AgentKind::Universal);
        assert_eq!(app.filtered_items().len(), 2);
        app.selected_idx = 1;
        app.toggle_mark_selected();
        assert!(!app.store.trash_items[1].is_marked);
        assert!(app.store.trash_items[2].is_marked);
    }

    #[test]
    fn unmarking_in_marked_view_keeps_selection_valid() {
        let mut app = test_app(std::path::Path::new("unused-test-path"));
        let mut marked = item(AgentKind::Universal);
        marked.is_marked = true;
        app.external_items.push(marked);
        app.filter_mode = FilterMode::Marked;
        app.toggle_mark_selected();
        assert!(app.filtered_items().is_empty());
        assert_eq!(app.selected_idx, 0);
    }

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
        let mut app = test_app(std::path::Path::new("unused-test-path"));
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
