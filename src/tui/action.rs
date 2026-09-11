use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ConfirmYes,
    ConfirmNo,
    CloseHelp,
    SearchChar(char),
    SearchBackspace,
    SearchSubmit,
    SearchCancel,
    ViewerSearchChar(char),
    ViewerSearchBackspace,
    ViewerSearchSubmit,
    ViewerSearchCancel,
    ViewerScrollUp(usize),
    ViewerScrollDown(usize),
    ViewerScrollLeft(usize),
    ViewerScrollRight(usize),
    ViewerHome,
    ViewerEnd,
    ViewerJumpUser,
    ViewerJumpAssistant,
    ViewerSearchStart,
    ViewerFindNext,
    ViewerFindPrev,
    ViewerDelete,
    CloseViewer,
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    Home,
    End,
    ToggleMark,
    ToggleMarkAll,
    OpenViewer,
    Restore,
    Delete,
    BatchDelete,
    CleanOrEmpty,
    SearchStart,
    ClearSearch,
    ToggleTrash,
    CycleFilter,
    CycleSort,
    CycleProvider,
    TransferStart,
    TransferNextTarget,
    TransferPrevTarget,
    TransferConfirm,
    TransferCancel,
    Export,
    Refresh,
    Help,
    Quit,
}

pub fn map_confirm_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => Some(Action::ConfirmYes),
        KeyCode::Char('n' | 'N' | 'q' | 'Q') | KeyCode::Esc => Some(Action::ConfirmNo),
        _ => None,
    }
}

pub fn map_transfer_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::TransferPrevTarget),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::TransferNextTarget),
        KeyCode::Enter | KeyCode::Char('y' | 'Y') => Some(Action::TransferConfirm),
        KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q' | 'Q') => Some(Action::TransferCancel),
        _ => None,
    }
}

pub fn map_viewer_search_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::ViewerSearchCancel),
        KeyCode::Enter => Some(Action::ViewerSearchSubmit),
        KeyCode::Backspace => Some(Action::ViewerSearchBackspace),
        KeyCode::Char(c) => Some(Action::ViewerSearchChar(c)),
        _ => None,
    }
}

pub fn map_viewer_key(key: KeyEvent, is_trash: bool) -> Option<Action> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::ViewerScrollUp(1)),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::ViewerScrollDown(1)),
        KeyCode::Left | KeyCode::Char('h') => Some(Action::ViewerScrollLeft(4)),
        KeyCode::Right | KeyCode::Char('l') => Some(Action::ViewerScrollRight(4)),
        KeyCode::PageUp | KeyCode::Char('b') => Some(Action::ViewerScrollUp(20)),
        KeyCode::PageDown | KeyCode::Char(' ') => Some(Action::ViewerScrollDown(20)),
        KeyCode::Home | KeyCode::Char('g') => Some(Action::ViewerHome),
        KeyCode::End | KeyCode::Char('G') => Some(Action::ViewerEnd),
        KeyCode::Char('0') => Some(Action::ViewerScrollLeft(usize::MAX)),
        KeyCode::Char('u' | 'U') => {
            if is_trash {
                Some(Action::Restore)
            } else {
                Some(Action::ViewerJumpUser)
            }
        }
        KeyCode::Char('m' | 'M' | 'a' | 'A') => Some(Action::ViewerJumpAssistant),
        KeyCode::Char('/') => Some(Action::ViewerSearchStart),
        KeyCode::Char('n') => Some(Action::ViewerFindNext),
        KeyCode::Char('N') => Some(Action::ViewerFindPrev),
        KeyCode::Char('e' | 'E') => Some(Action::Export),
        KeyCode::Char('d' | 'D' | 'x' | 'X') => Some(Action::ViewerDelete),
        KeyCode::Char('q' | 'Q') | KeyCode::Esc => Some(Action::CloseViewer),
        _ => None,
    }
}

pub fn map_search_input_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::SearchCancel),
        KeyCode::Enter => Some(Action::SearchSubmit),
        KeyCode::Backspace => Some(Action::SearchBackspace),
        KeyCode::Char(c) => Some(Action::SearchChar(c)),
        _ => None,
    }
}

pub fn map_main_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::MoveUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::MoveDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Home | KeyCode::Char('g') => Some(Action::Home),
        KeyCode::End | KeyCode::Char('G') => Some(Action::End),
        KeyCode::Char(' ') => Some(Action::ToggleMark),
        KeyCode::Char('a' | 'A') => Some(Action::ToggleMarkAll),
        KeyCode::Enter | KeyCode::Char('v' | 'V') => Some(Action::OpenViewer),
        KeyCode::Char('u' | 'U') => Some(Action::Restore),
        KeyCode::Char('d' | 'D' | 'x' | 'X') => Some(Action::Delete),
        KeyCode::Char('b' | 'B') => Some(Action::BatchDelete),
        KeyCode::Char('c' | 'C') => Some(Action::CleanOrEmpty),
        KeyCode::Char('/' | 's' | 'S') => Some(Action::SearchStart),
        KeyCode::Esc => Some(Action::ClearSearch),
        KeyCode::Char('t' | 'T') => Some(Action::ToggleTrash),
        KeyCode::Tab | KeyCode::Char('f' | 'F') => Some(Action::CycleFilter),
        KeyCode::Char('o' | 'O') => Some(Action::CycleSort),
        KeyCode::Char('p' | 'P') => Some(Action::CycleProvider),
        KeyCode::Char('m' | 'M') => Some(Action::TransferStart),
        KeyCode::Char('e' | 'E') => Some(Action::Export),
        KeyCode::Char('R') | KeyCode::F(5) => Some(Action::Refresh),
        KeyCode::Char('h' | 'H' | '?') => Some(Action::Help),
        KeyCode::Char('q' | 'Q') => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    #[test]
    fn test_navigation() {
        assert_eq!(
            map_main_key(press(KeyCode::Char('k'))),
            Some(Action::MoveUp)
        );
        assert_eq!(map_main_key(press(KeyCode::Up)), Some(Action::MoveUp));
        assert_eq!(
            map_main_key(press(KeyCode::Char('j'))),
            Some(Action::MoveDown)
        );
        assert_eq!(map_main_key(press(KeyCode::Down)), Some(Action::MoveDown));
    }

    #[test]
    fn test_confirm_keys() {
        assert_eq!(
            map_confirm_key(press(KeyCode::Char('y'))),
            Some(Action::ConfirmYes)
        );
        assert_eq!(
            map_confirm_key(press(KeyCode::Enter)),
            Some(Action::ConfirmYes)
        );
        assert_eq!(
            map_confirm_key(press(KeyCode::Char('n'))),
            Some(Action::ConfirmNo)
        );
        assert_eq!(
            map_confirm_key(press(KeyCode::Esc)),
            Some(Action::ConfirmNo)
        );
    }

    #[test]
    fn test_transfer_keys() {
        assert_eq!(
            map_transfer_key(press(KeyCode::Up)),
            Some(Action::TransferPrevTarget)
        );
        assert_eq!(
            map_transfer_key(press(KeyCode::Char('k'))),
            Some(Action::TransferPrevTarget)
        );
        assert_eq!(
            map_transfer_key(press(KeyCode::Down)),
            Some(Action::TransferNextTarget)
        );
        assert_eq!(
            map_transfer_key(press(KeyCode::Char('j'))),
            Some(Action::TransferNextTarget)
        );
        assert_eq!(
            map_transfer_key(press(KeyCode::Enter)),
            Some(Action::TransferConfirm)
        );
        assert_eq!(
            map_transfer_key(press(KeyCode::Esc)),
            Some(Action::TransferCancel)
        );
    }

    #[test]
    fn test_agent_actions() {
        assert_eq!(
            map_main_key(press(KeyCode::Char('p'))),
            Some(Action::CycleProvider)
        );
        assert_eq!(
            map_main_key(press(KeyCode::Char('P'))),
            Some(Action::CycleProvider)
        );
        assert_eq!(
            map_main_key(press(KeyCode::Char('m'))),
            Some(Action::TransferStart)
        );
        assert_eq!(
            map_main_key(press(KeyCode::Char('M'))),
            Some(Action::TransferStart)
        );
    }
}
