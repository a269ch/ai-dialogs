pub mod action;
pub mod app;
pub mod ui;

use std::io::{self, stdout};
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::tui::action::{
    Action, map_confirm_key, map_main_key, map_search_input_key, map_transfer_key, map_viewer_key,
    map_viewer_search_key,
};
use crate::tui::app::App;
use crate::tui::ui::draw_app;

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, Show);
    }
}

pub fn run_tui() -> Result<(), io::Error> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new();

    while app.running {
        terminal.draw(|f| draw_app(f, &mut app))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            handle_key_event(&mut app, key, terminal.size()?.width as usize);
        }
    }

    Ok(())
}

fn handle_key_event(app: &mut App, key: KeyEvent, term_width: usize) {
    if app.confirm.is_some() {
        if let Some(action) = map_confirm_key(key) {
            match action {
                Action::ConfirmYes => app.execute_confirm(),
                Action::ConfirmNo => {
                    app.confirm = None;
                    app.status_msg = "Action cancelled.".to_string();
                }
                _ => {}
            }
        }
        return;
    }

    if app.transfer_dialog.is_some() {
        if let Some(action) = map_transfer_key(key) {
            match action {
                Action::TransferPrevTarget => {
                    if let Some(ref mut d) = app.transfer_dialog
                        && d.selected_idx > 0
                    {
                        d.selected_idx -= 1;
                    }
                }
                Action::TransferNextTarget => {
                    if let Some(ref mut d) = app.transfer_dialog
                        && !d.target_options.is_empty()
                        && d.selected_idx + 1 < d.target_options.len()
                    {
                        d.selected_idx += 1;
                    }
                }
                Action::TransferConfirm => app.execute_transfer(),
                Action::TransferCancel => {
                    app.transfer_dialog = None;
                    app.status_msg = "Transfer cancelled.".to_string();
                }
                _ => {}
            }
        }
        return;
    }

    if app.show_help {
        app.show_help = false;
        return;
    }

    if let Some(ref mut viewer) = app.viewer {
        if viewer.is_searching {
            if let Some(action) = map_viewer_search_key(key) {
                match action {
                    Action::ViewerSearchCancel => {
                        viewer.is_searching = false;
                        viewer.search_input.clear();
                    }
                    Action::ViewerSearchSubmit => {
                        viewer.search_kw = viewer.search_input.clone();
                        viewer.is_searching = false;
                        viewer.find_next(24);
                    }
                    Action::ViewerSearchBackspace => {
                        viewer.search_input.pop();
                    }
                    Action::ViewerSearchChar(c) => {
                        viewer.search_input.push(c);
                    }
                    _ => {}
                }
            }
            return;
        }

        let is_trash = viewer.item.is_in_trash;
        if let Some(action) = map_viewer_key(key, is_trash) {
            match action {
                Action::ViewerScrollUp(n) => viewer.scroll_up(n),
                Action::ViewerScrollDown(n) => viewer.scroll_down(n, 24),
                Action::ViewerScrollLeft(n) => viewer.scroll_left(n),
                Action::ViewerScrollRight(n) => viewer.scroll_right(n),
                Action::ViewerHome => {
                    viewer.scroll_y = 0;
                    viewer.scroll_x = 0;
                }
                Action::ViewerEnd => viewer.scroll_y = viewer.max_scroll_y(24),
                Action::Restore => {
                    let item_clone = viewer.item.clone();
                    if app.store.restore(&item_clone).is_ok() {
                        app.status_msg = format!("✅ Dialogue {} restored!", item_clone.id);
                        app.close_viewer();
                    }
                }
                Action::ViewerJumpUser => viewer.jump_next_user(24),
                Action::ViewerJumpAssistant => viewer.jump_next_assistant(24),
                Action::ViewerSearchStart => {
                    viewer.is_searching = true;
                    viewer.search_input.clear();
                }
                Action::ViewerFindNext => viewer.find_next(24),
                Action::ViewerFindPrev => viewer.find_prev(24),
                Action::Export => {
                    let item = viewer.item.clone();
                    match app.store.export_to_markdown(&item, None) {
                        Ok(p) => {
                            app.status_msg = format!("Exported to: {}", p.display());
                            app.close_viewer();
                        }
                        Err(e) => {
                            app.status_msg = format!("Export error: {}", e);
                        }
                    }
                }
                Action::ViewerDelete => {
                    let item = viewer.item.clone();
                    let short_id: String = item.id.chars().take(8).collect();
                    app.close_viewer();
                    if item.is_in_trash {
                        app.confirm = Some(crate::tui::app::ConfirmDialog {
                            message: format!(
                                "⛔ PERMANENTLY DELETE dialogue {} from trash? [y/N]",
                                short_id
                            ),
                            is_destructive: true,
                            action: crate::tui::app::ConfirmAction::DeletePermanentSingle(item),
                        });
                    } else {
                        app.confirm = Some(crate::tui::app::ConfirmDialog {
                            message: format!("⚠️ Move dialogue {} to trash? [y/N]", short_id),
                            is_destructive: false,
                            action: crate::tui::app::ConfirmAction::DeleteSingle(item),
                        });
                    }
                }
                Action::CloseViewer => app.close_viewer(),
                _ => {}
            }
        }
        return;
    }

    if app.is_searching {
        if let Some(action) = map_search_input_key(key) {
            match action {
                Action::SearchCancel => {
                    app.is_searching = false;
                    app.search_input.clear();
                }
                Action::SearchSubmit => {
                    app.search_query = app.search_input.clone();
                    app.is_searching = false;
                    app.selected_idx = 0;
                    app.status_msg = if app.search_query.is_empty() {
                        "Search cleared.".to_string()
                    } else {
                        format!("Search: \"{}\"", app.search_query)
                    };
                }
                Action::SearchBackspace => {
                    app.search_input.pop();
                }
                Action::SearchChar(c) => {
                    app.search_input.push(c);
                }
                _ => {}
            }
        }
        return;
    }

    let items_len = app.filtered_items().len();

    if let Some(action) = map_main_key(key) {
        match action {
            Action::MoveUp if app.selected_idx > 0 => {
                app.selected_idx -= 1;
            }
            Action::MoveDown if items_len > 0 && app.selected_idx + 1 < items_len => {
                app.selected_idx += 1;
            }
            Action::PageUp => {
                app.selected_idx = app.selected_idx.saturating_sub(15);
            }
            Action::PageDown if items_len > 0 => {
                app.selected_idx = (app.selected_idx + 15).min(items_len - 1);
            }
            Action::Home => {
                app.selected_idx = 0;
            }
            Action::End if items_len > 0 => {
                app.selected_idx = items_len - 1;
            }
            Action::ToggleMark => {
                app.toggle_mark_selected();
            }
            Action::ToggleMarkAll => {
                app.toggle_mark_all();
            }
            Action::OpenViewer => {
                let items = app.filtered_items();
                if !items.is_empty() && app.selected_idx < items.len() {
                    let item = items[app.selected_idx].clone();
                    app.open_viewer(item, term_width);
                }
            }
            Action::Restore => {
                app.restore_selected();
            }
            Action::Delete => {
                app.prompt_delete_selected();
            }
            Action::BatchDelete => {
                app.prompt_batch_delete();
            }
            Action::CleanOrEmpty => {
                app.prompt_clean_or_empty();
            }
            Action::SearchStart => {
                app.is_searching = true;
                app.search_input = app.search_query.clone();
            }
            Action::ClearSearch if !app.search_query.is_empty() => {
                app.search_query.clear();
                app.selected_idx = 0;
                app.status_msg = "Search filter reset.".to_string();
            }
            Action::ClearSearch => {
                app.running = false;
            }
            Action::ToggleTrash => {
                app.filter_mode = if app.is_trash_view() {
                    crate::tui::app::FilterMode::All
                } else {
                    crate::tui::app::FilterMode::Trash
                };
                app.selected_idx = 0;
                app.status_msg = format!("Mode: [{}]", app.filter_mode.as_str());
            }
            Action::CycleFilter => {
                app.filter_mode = app.filter_mode.next();
                app.selected_idx = 0;
                app.status_msg = format!("Mode: [{}]", app.filter_mode.as_str());
            }
            Action::CycleSort => {
                app.sort_mode = app.sort_mode.next();
                app.status_msg = format!("Sort: [{}]", app.sort_mode.as_str());
            }
            Action::CycleProvider => {
                app.cycle_provider();
            }
            Action::TransferStart => {
                app.prompt_transfer();
            }
            Action::Export => {
                app.export_selected();
            }
            Action::Refresh => {
                app.refresh_all();
                app.status_msg = "All agent dialogues refreshed from disk.".to_string();
            }
            Action::Help => {
                app.show_help = true;
            }
            Action::Quit => {
                app.running = false;
            }
            _ => {}
        }
    }
}
