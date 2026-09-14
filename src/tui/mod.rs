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
    let _guard = TerminalGuard;
    execute!(stdout(), EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new();

    while app.running {
        if let Some(item) = app.resume_target.take() {
            let _ = disable_raw_mode();
            let _ = execute!(stdout(), LeaveAlternateScreen, Show);

            match crate::launcher::run_interactive(&item) {
                Ok(status) => {
                    app.status_msg = format!("Session finished ({})", status);
                }
                Err(e) => {
                    app.status_msg = format!("Resume failed: {}", e);
                }
            }

            let _ = enable_raw_mode();
            let _ = execute!(stdout(), EnterAlternateScreen);
            terminal.clear()?;
            app.refresh_all();
        }

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
        let content_height = viewer.content_height;
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
                        viewer.find_next(content_height);
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
                Action::ViewerScrollDown(n) => viewer.scroll_down(n, content_height),
                Action::ViewerPageUp => viewer.scroll_up(content_height.max(1)),
                Action::ViewerPageDown => viewer.scroll_down(content_height.max(1), content_height),
                Action::ViewerScrollLeft(n) => viewer.scroll_left(n),
                Action::ViewerScrollRight(n) => viewer.scroll_right(n),
                Action::ViewerHome => {
                    viewer.scroll_y = 0;
                    viewer.scroll_x = 0;
                }
                Action::ViewerEnd => viewer.scroll_y = viewer.max_scroll_y(content_height),
                Action::Restore => {
                    let item_clone = viewer.item.clone();
                    match app.store.restore(&item_clone) {
                        Ok(()) => {
                            app.status_msg = format!("✅ Dialogue {} restored!", item_clone.id);
                            app.close_viewer();
                            app.refresh_all();
                        }
                        Err(e) => {
                            app.status_msg = format!("Error restoring {}: {}", item_clone.id, e);
                            app.close_viewer();
                        }
                    }
                }
                Action::ViewerJumpUser => viewer.jump_next_user(content_height),
                Action::ViewerJumpAssistant => viewer.jump_next_assistant(content_height),
                Action::ViewerSearchStart => {
                    viewer.is_searching = true;
                    viewer.search_input.clear();
                }
                Action::ViewerFindNext => viewer.find_next(content_height),
                Action::ViewerFindPrev => viewer.find_prev(content_height),
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
                Action::ResumeSession => {
                    let item = viewer.item.clone();
                    if item.is_in_trash {
                        app.status_msg =
                            "Cannot resume dialogue in trash. Restore it first.".to_string();
                    } else {
                        app.resume_target = Some(item);
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
            Action::ResumeSession => {
                let items = app.filtered_items();
                if !items.is_empty() && app.selected_idx < items.len() {
                    let item = items[app.selected_idx].clone();
                    if item.is_in_trash {
                        app.status_msg =
                            "Cannot resume dialogue in trash. Restore it first.".to_string();
                    } else {
                        app.resume_target = Some(item);
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::AgentKind;
    use crate::models::DialogueItem;
    use crate::tui::app::{ViewerState, test_app};
    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::text::Line;

    fn viewer_app() -> App {
        let mut app = test_app(std::path::Path::new("unused-test-path"));
        let item = DialogueItem::new_external(
            "test".into(),
            AgentKind::Universal,
            "test".into(),
            None,
            1,
            1,
            0,
            None,
        );
        let mut viewer = ViewerState::new(item, Vec::new(), 80);
        viewer.rendered_lines = (0..59)
            .map(|idx| Line::from(format!("line {idx}")))
            .collect();
        viewer.rendered_lines.push(Line::from("last line"));
        viewer.user_jump_lines = vec![59];
        viewer.assistant_jump_lines = vec![59];
        app.viewer = Some(viewer);
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        handle_key_event(app, KeyEvent::new(code, KeyModifiers::NONE), 80);
    }

    #[test]
    fn viewer_keys_use_rendered_viewport_and_reach_last_line() {
        for height in [24, 40] {
            let mut app = viewer_app();
            let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
            terminal.draw(|frame| draw_app(frame, &mut app)).unwrap();
            let content_height = usize::from(height) - 2;
            let max_scroll = 60 - content_height;
            assert_eq!(app.viewer.as_ref().unwrap().content_height, content_height);

            press(&mut app, KeyCode::End);
            assert_eq!(app.viewer.as_ref().unwrap().scroll_y, max_scroll);
            terminal.draw(|frame| draw_app(frame, &mut app)).unwrap();
            assert_eq!(terminal.backend().buffer()[(0, height - 2)].symbol(), "l");

            press(&mut app, KeyCode::Home);
            for _ in 0..60 {
                press(&mut app, KeyCode::Down);
            }
            assert_eq!(app.viewer.as_ref().unwrap().scroll_y, max_scroll);

            press(&mut app, KeyCode::Home);
            press(&mut app, KeyCode::PageDown);
            assert_eq!(
                app.viewer.as_ref().unwrap().scroll_y,
                content_height.min(max_scroll)
            );
            press(&mut app, KeyCode::PageUp);
            assert_eq!(app.viewer.as_ref().unwrap().scroll_y, 0);

            for code in [KeyCode::Char('u'), KeyCode::Char('m')] {
                press(&mut app, KeyCode::Home);
                press(&mut app, code);
                assert_eq!(app.viewer.as_ref().unwrap().scroll_y, max_scroll);
            }
            press(&mut app, KeyCode::Home);
            press(&mut app, KeyCode::Char('/'));
            app.viewer.as_mut().unwrap().search_input = "last".into();
            press(&mut app, KeyCode::Enter);
            assert_eq!(app.viewer.as_ref().unwrap().scroll_y, max_scroll);
            for code in [KeyCode::Char('n'), KeyCode::Char('N')] {
                press(&mut app, KeyCode::Home);
                press(&mut app, code);
                assert_eq!(app.viewer.as_ref().unwrap().scroll_y, max_scroll);
            }
        }
    }

    #[test]
    fn growing_viewport_clamps_existing_scroll_position() {
        let mut app = viewer_app();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw_app(frame, &mut app)).unwrap();
        press(&mut app, KeyCode::End);
        assert_eq!(app.viewer.as_ref().unwrap().scroll_y, 38);

        let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
        terminal.draw(|frame| draw_app(frame, &mut app)).unwrap();
        assert_eq!(app.viewer.as_ref().unwrap().scroll_y, 22);
        assert_eq!(terminal.backend().buffer()[(0, 38)].symbol(), "l");
    }
}
