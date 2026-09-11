use ai_dialogs::cli::{Cli, handle_cli};
use ai_dialogs::storage::DialogueStore;
use ai_dialogs::tui;
use clap::Parser;

fn main() {
    let cli = Cli::parse();

    if cli.is_cli_mode() {
        let mut store = DialogueStore::new();
        handle_cli(&cli, &mut store);
    } else if let Err(e) = tui::run_tui() {
        eprintln!("TUI Error: {}", e);
        std::process::exit(1);
    }
}
