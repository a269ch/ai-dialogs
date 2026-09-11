use clap::Parser;
use std::io::{self, Write};
use std::process;

use crate::canonical::AgentKind;
use crate::cleaner::format_bytes;
use crate::markdown::ansi::render_ansi;
use crate::models::{ActiveItemJson, DialogueItem, DialogueStoreJson, TrashItemJson};
use crate::providers::ProviderRegistry;
use crate::storage::DialogueStore;
use crate::transfer::TransferEngine;

#[derive(Parser, Debug)]
#[command(
    name = "ai-dialogs",
    author,
    version,
    about = "Universal AI Dialogue Manager — inspect, transfer, and manage chat sessions across Antigravity, Claude, Codex, Grok, and Universal JSON."
)]
pub struct Cli {
    #[arg(
        short = 'l',
        long = "list",
        help = "List active dialogues in a formatted table"
    )]
    pub list: bool,

    #[arg(
        short = 't',
        long = "trash",
        help = "List deleted dialogues currently in trash"
    )]
    pub trash: bool,

    #[arg(
        short = 'v',
        long = "view",
        value_name = "ID",
        help = "Read and render dialogue Markdown by ID"
    )]
    pub view: Option<String>,

    #[arg(
        short = 'd',
        long = "delete",
        value_name = "ID",
        help = "Delete dialogue (move to trash)"
    )]
    pub delete: Option<String>,

    #[arg(
        short = 'r',
        long = "restore",
        value_name = "ID",
        help = "Restore dialogue from trash to active sessions"
    )]
    pub restore: Option<String>,

    #[arg(
        short = 'e',
        long = "export",
        value_name = "ID",
        help = "Export dialogue to Markdown file"
    )]
    pub export: Option<String>,

    #[arg(long = "clean-empty", help = "Clean all empty sessions (0 messages)")]
    pub clean_empty: bool,

    #[arg(
        long = "empty-trash",
        help = "Completely empty trash (permanently delete all files)"
    )]
    pub empty_trash: bool,

    #[arg(long = "force", help = "Skip interactive confirmation prompts")]
    pub force: bool,

    #[arg(long = "json", help = "Output dialogue data in JSON format")]
    pub json: bool,

    #[arg(
        short = 'p',
        long = "provider",
        value_name = "AGENT",
        help = "Filter by AI agent provider (agy, claude, codex, grok, universal, all)"
    )]
    pub provider: Option<String>,

    #[arg(
        long = "providers",
        help = "List all detected AI agent providers and status"
    )]
    pub providers: bool,

    #[arg(
        long = "transfer",
        value_name = "ID",
        help = "Transfer dialogue to another AI agent"
    )]
    pub transfer: Option<String>,

    #[arg(
        long = "from",
        value_name = "AGENT",
        help = "Source agent for transfer (default: agy)"
    )]
    pub from_agent: Option<String>,

    #[arg(
        long = "to",
        value_name = "AGENT",
        help = "Target agent for transfer (agy, claude, codex, grok, universal)"
    )]
    pub to_agent: Option<String>,

    #[arg(
        long = "export-json",
        value_name = "PATH",
        help = "Export dialogue to Universal JSON file"
    )]
    pub export_json: Option<String>,

    #[arg(
        long = "import-json",
        value_name = "PATH",
        help = "Import dialogue from Universal JSON file"
    )]
    pub import_json: Option<String>,
}

impl Cli {
    pub fn is_cli_mode(&self) -> bool {
        self.list
            || self.trash
            || self.view.is_some()
            || self.delete.is_some()
            || self.restore.is_some()
            || self.export.is_some()
            || self.clean_empty
            || self.empty_trash
            || self.json
            || self.providers
            || self.transfer.is_some()
            || self.export_json.is_some()
            || self.import_json.is_some()
    }
}

fn prompt_user(prompt: &str) -> bool {
    print!("{}", prompt);
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let trimmed = input.trim().to_lowercase();
        matches!(trimmed.as_str(), "y" | "yes")
    } else {
        false
    }
}

pub fn handle_cli(cli: &Cli, store: &mut DialogueStore) {
    let registry = ProviderRegistry::new();

    if cli.providers {
        println!();
        println!("AI Agent Providers:");
        println!("{}", "─".repeat(88));
        println!(
            "{:<18} {:<12} {:<10} {:<45}",
            "PROVIDER", "STATUS", "SESSIONS", "PATH"
        );
        println!("{}", "─".repeat(88));
        for kind in AgentKind::ALL {
            if let Some(p) = registry.get(kind) {
                let status = if p.is_available() {
                    "available"
                } else {
                    "not found"
                };
                let count = p.list_dialogues().map(|v| v.len()).unwrap_or(0);
                let path_str = p
                    .base_dir()
                    .map(|pb| pb.display().to_string())
                    .unwrap_or_else(|| "N/A".to_string());
                println!(
                    "{:<18} {:<12} {:<10} {:<45}",
                    p.display_name(),
                    status,
                    count,
                    path_str
                );
            }
        }
        println!("{}", "─".repeat(88));
        println!();
        return;
    }

    if let Some(ref tid) = cli.transfer {
        let from_str = cli.from_agent.as_deref().unwrap_or("agy");
        let to_str = match cli.to_agent.as_deref() {
            Some(t) => t,
            None => {
                eprintln!("Error: Target agent (--to <AGENT>) must be specified.");
                eprintln!("Available agents: agy, claude, codex, grok, universal");
                process::exit(1);
            }
        };

        let from_kind = match AgentKind::parse_str(from_str) {
            Some(k) => k,
            None => {
                eprintln!(
                    "Invalid source agent '{}'. Available: agy, claude, codex, grok, universal",
                    from_str
                );
                process::exit(1);
            }
        };

        let to_kind = match AgentKind::parse_str(to_str) {
            Some(k) => k,
            None => {
                eprintln!(
                    "Invalid target agent '{}'. Available: agy, claude, codex, grok, universal",
                    to_str
                );
                process::exit(1);
            }
        };

        match TransferEngine::transfer(&registry, from_kind, to_kind, tid) {
            Ok(result) => {
                println!();
                println!("✅ Dialogue successfully transferred!");
                println!(
                    "- Source: {} (ID: {})",
                    result.source_agent, result.source_id
                );
                println!(
                    "- Target: {} (New ID: {})",
                    result.target_agent, result.target_id
                );
                println!("- Title:  {}", result.title);
                println!(
                    "- Messages: {} total ({} user, {} assistant)",
                    result.messages_count,
                    result.user_messages_count,
                    result.assistant_messages_count
                );
                println!();
            }
            Err(e) => {
                eprintln!("Transfer failed: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    if let Some(ref path_str) = cli.import_json {
        let to_str = cli.to_agent.as_deref().unwrap_or("agy");
        let to_kind = match AgentKind::parse_str(to_str) {
            Some(k) => k,
            None => {
                eprintln!("Invalid target agent '{}'.", to_str);
                process::exit(1);
            }
        };
        match TransferEngine::import_file(&registry, to_kind, std::path::Path::new(path_str)) {
            Ok(new_id) => {
                println!("✅ Dialogue imported from {} to {}!", path_str, to_kind);
                println!("- New session ID: {}", new_id);
            }
            Err(e) => {
                eprintln!("Import failed: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    if let Some(ref path_str) = cli.export_json {
        let from_str = cli.from_agent.as_deref().unwrap_or("agy");
        let from_kind = match AgentKind::parse_str(from_str) {
            Some(k) => k,
            None => {
                eprintln!("Invalid source agent '{}'.", from_str);
                process::exit(1);
            }
        };
        let export_id = match cli.view.as_deref().or(cli.export.as_deref()) {
            Some(id) => id,
            None => {
                eprintln!("Error: Specify dialogue ID to export via -v <ID> or -e <ID>");
                process::exit(1);
            }
        };
        match TransferEngine::export_file(
            &registry,
            from_kind,
            export_id,
            std::path::Path::new(path_str),
        ) {
            Ok(()) => {
                println!(
                    "✅ Dialogue {} exported to Universal JSON: {}",
                    export_id, path_str
                );
            }
            Err(e) => {
                eprintln!("Export failed: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    if cli.json {
        let active_items: Vec<ActiveItemJson> = store
            .items
            .iter()
            .map(|it| ActiveItemJson {
                id: &it.id,
                agent: it.agent.as_str(),
                created_at: it.created_at.as_deref(),
                user_messages_count: it.user_msgs_count,
                model_messages_count: it.model_msgs_count,
                size_bytes: it.size_bytes,
                is_subagent: it.is_subagent,
                is_empty: it.is_empty,
                topic: &it.topic,
            })
            .collect();

        let trash_items: Vec<TrashItemJson> = store
            .trash_items
            .iter()
            .map(|it| TrashItemJson {
                id: &it.id,
                agent: it.agent.as_str(),
                created_at: it.created_at.as_deref(),
                deleted_at: it.deleted_at.as_deref(),
                size_bytes: it.size_bytes,
                topic: &it.topic,
                trash_folder: it
                    .trash_folder
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
            })
            .collect();

        let store_json = DialogueStoreJson {
            active: active_items,
            trash: trash_items,
        };

        match serde_json::to_string_pretty(&store_json) {
            Ok(json_str) => println!("{}", json_str),
            Err(e) => eprintln!("JSON serialization error: {}", e),
        }
        return;
    }

    if cli.trash {
        cli_list_trash(store);
        return;
    }

    if cli.list {
        if let Some(ref prov_str) = cli.provider {
            if prov_str == "all" {
                let items = registry.list_all_dialogues();
                cli_list_external(&items, "All Providers");
                return;
            } else if let Some(kind) = AgentKind::parse_str(prov_str)
                && kind != AgentKind::Antigravity
                && let Some(p) = registry.get(kind)
            {
                match p.list_dialogues() {
                    Ok(items) => {
                        cli_list_external(&items, p.display_name());
                        return;
                    }
                    Err(e) => {
                        eprintln!("Error listing {} dialogues: {}", p.display_name(), e);
                        process::exit(1);
                    }
                }
            }
        }
        cli_list(store, true, true);
        return;
    }

    if let Some(ref view_id) = cli.view {
        if let Some((_prov, item)) = registry.find_dialogue(view_id) {
            cli_view_item(store, &item, false);
            return;
        }
        cli_view(store, view_id);
        return;
    }

    if let Some(ref restore_id) = cli.restore {
        let item = store.find(restore_id, true).cloned();
        match item {
            Some(it) => match store.restore(&it) {
                Ok(()) => {
                    println!(
                        "✅ Dialogue {} successfully restored from trash to active sessions!",
                        it.id
                    );
                }
                Err(err) => {
                    eprintln!("Error restoring {}: {}", it.id, err);
                    process::exit(1);
                }
            },
            None => {
                eprintln!("Dialogue '{}' not found in trash.", restore_id);
                process::exit(1);
            }
        }
        return;
    }

    if cli.empty_trash {
        if store.trash_items.is_empty() {
            println!("Trash is already empty.");
            return;
        }
        if !cli.force {
            let msg = format!(
                "Are you sure you want to PERMANENTLY delete {} dialogues from trash? [y/N]: ",
                store.trash_items.len()
            );
            if !prompt_user(&msg) {
                println!("Trash cleanup cancelled.");
                return;
            }
        }
        match store.empty_trash() {
            Ok(cleared) => println!("Trash emptied ({} entries removed).", cleared),
            Err(e) => eprintln!("Error emptying trash: {}", e),
        }
        return;
    }

    if let Some(ref export_id) = cli.export {
        let item = store
            .find(export_id, false)
            .or_else(|| store.find(export_id, true))
            .cloned();

        match item {
            Some(it) => match store.export_to_markdown(&it, None) {
                Ok(path) => println!("Dialogue exported to: {}", path.display()),
                Err(e) => eprintln!("Export error: {}", e),
            },
            None => {
                if let Some((_prov, it)) = registry.find_dialogue(export_id) {
                    match store.export_to_markdown(&it, None) {
                        Ok(path) => println!("Dialogue exported to: {}", path.display()),
                        Err(e) => eprintln!("Export error: {}", e),
                    }
                    return;
                }
                eprintln!("Dialogue '{}' not found.", export_id);
                process::exit(1);
            }
        }
        return;
    }

    if let Some(ref delete_id) = cli.delete {
        let active_item = store.find(delete_id, false).cloned();
        if let Some(it) = active_item {
            if !cli.force {
                let topic_trunc: String = it.topic.chars().take(40).collect();
                let msg = format!(
                    "Move dialogue {} ({}) to trash? [y/N]: ",
                    it.id, topic_trunc
                );
                if !prompt_user(&msg) {
                    println!("Deletion cancelled.");
                    return;
                }
            }
            match store.delete(&it, true, true) {
                Ok(()) => {
                    let short_id: String = it.id.chars().take(8).collect();
                    println!(
                        "Dialogue {} moved to trash. (Restore: ai-dialogs --restore {})",
                        it.id, short_id
                    );
                }
                Err(e) => {
                    eprintln!("Error deleting {}: {}", it.id, e);
                    process::exit(1);
                }
            }
            return;
        }

        let trash_item = store.find(delete_id, true).cloned();
        if let Some(it) = trash_item {
            if !cli.force {
                let topic_trunc: String = it.topic.chars().take(40).collect();
                let msg = format!(
                    "⛔ PERMANENTLY delete dialogue {} ({})? This cannot be undone! [y/N]: ",
                    it.id, topic_trunc
                );
                if !prompt_user(&msg) {
                    println!("Permanent deletion cancelled.");
                    return;
                }
            }
            match store.delete_permanently(&it, true) {
                Ok(()) => {
                    println!("Dialogue {} permanently deleted from disk.", it.id);
                }
                Err(e) => {
                    eprintln!("Error permanently deleting {}: {}", it.id, e);
                    process::exit(1);
                }
            }
            return;
        }

        eprintln!("Dialogue '{}' not found.", delete_id);
        process::exit(1);
    }

    if cli.clean_empty {
        let empty_count = store.items.iter().filter(|i| i.is_empty).count();
        if empty_count == 0 {
            println!("No empty sessions found.");
            return;
        }
        if !cli.force {
            let msg = format!("Move {} empty sessions to trash? [y/N]: ", empty_count);
            if !prompt_user(&msg) {
                println!("Cancelled.");
                return;
            }
        }
        let count = store.clean_empty(true);
        println!("{} empty sessions moved to trash.", count);
    }
}

pub fn cli_list(store: &DialogueStore, _header: bool, _footer: bool) {
    println!(
        "\n{:<3} {:<10} {:<17} {:<10} {:<10} TOPIC",
        "#", "ID", "DATE / TIME", "MSGS", "SIZE"
    );
    println!("{}", "─".repeat(100));

    for (idx, item) in store.items.iter().enumerate() {
        let msgs = format!("{}u / {}m", item.user_msgs_count, item.model_msgs_count);
        let size = format_bytes(item.size_bytes);
        let short_id: String = item.id.chars().take(8).collect();
        let topic_short: String = item.topic.chars().take(50).collect();
        println!(
            "{:<3} {:<10} {:<17} {:<10} {:<10} {}",
            idx + 1,
            short_id,
            item.date_str(),
            msgs,
            size,
            topic_short
        );
    }
    println!("{}", "─".repeat(100));
    println!(
        "Total active dialogues: {} | In trash: {} (ai-dialogs --trash)\n",
        store.items.len(),
        store.trash_items.len()
    );
}

pub fn cli_list_external(items: &[DialogueItem], provider_name: &str) {
    println!(
        "\n{:<3} {:<10} {:<12} {:<17} {:<10} {:<10} TOPIC",
        "#", "ID", "AGENT", "DATE / TIME", "MSGS", "SIZE"
    );
    println!("{}", "─".repeat(105));

    for (i, item) in items.iter().enumerate() {
        let msgs = format!("{}u / {}m", item.user_msgs_count, item.model_msgs_count);
        let size = format_bytes(item.size_bytes);
        let short_id: String = item.id.chars().take(8).collect();
        let topic_short: String = item.topic.chars().take(45).collect();
        println!(
            "{:<3} {:<10} {:<12} {:<17} {:<10} {:<10} {}",
            i + 1,
            short_id,
            item.agent.as_str(),
            item.date_str(),
            msgs,
            size,
            topic_short
        );
    }
    println!("{}", "─".repeat(105));
    println!("Total dialogues for [{}]: {}\n", provider_name, items.len());
}

pub fn cli_list_trash(store: &DialogueStore) {
    println!(
        "\n{:<3} {:<10} {:<17} {:<10} {:<10} TOPIC IN TRASH",
        "#", "ID", "DELETED", "MSGS", "SIZE"
    );
    println!("{}", "─".repeat(100));

    if store.trash_items.is_empty() {
        println!("  Trash is empty.");
    } else {
        for (i, item) in store.trash_items.iter().enumerate() {
            let msgs = format!("{}u / {}m", item.user_msgs_count, item.model_msgs_count);
            let size = format_bytes(item.size_bytes);
            let del_d = item.deleted_date_str();
            let short_id: String = item.id.chars().take(8).collect();
            let topic_short: String = item.topic.chars().take(50).collect();
            println!(
                "{:<3} {:<10} {:<17} {:<10} {:<10} {}",
                i + 1,
                short_id,
                del_d,
                msgs,
                size,
                topic_short
            );
        }
    }
    println!("{}", "─".repeat(100));
    println!("Total in trash: {}", store.trash_items.len());
    println!("To restore use:   ai-dialogs --restore <ID>");
    println!("To empty trash:   ai-dialogs --empty-trash\n");
}

pub fn cli_view(store: &DialogueStore, conv_id: &str) {
    let mut in_trash = false;
    let mut item = store.find(conv_id, false);
    if item.is_none() {
        item = store.find(conv_id, true);
        if item.is_some() {
            in_trash = true;
        }
    }

    let Some(item) = item else {
        let loc = if in_trash {
            "in trash"
        } else {
            "among active dialogues"
        };
        eprintln!("Dialogue with ID '{}' not found {}.", conv_id, loc);
        process::exit(1);
    };

    cli_view_item(store, item, in_trash);
}

pub fn cli_view_item(store: &DialogueStore, item: &DialogueItem, in_trash: bool) {
    let steps = store.load_conversation_steps(item);
    let term_width = terminal_size_cols().unwrap_or(85);
    let render_w = term_width.min(100);

    let status_tag = if in_trash { " [IN TRASH]" } else { "" };
    println!("\n{}", "=".repeat(80));
    println!(
        " DIALOGUE [{}]: {}{}",
        item.agent.display_name(),
        item.topic,
        status_tag
    );
    println!(
        " ID: {} | Date: {} | Size: {}",
        item.id,
        item.date_str(),
        format_bytes(item.size_bytes)
    );
    if in_trash && let Some(ref tf) = item.trash_folder {
        println!(
            " Deleted: {} | Folder: {}",
            item.deleted_date_str(),
            tf.display()
        );
    }
    println!(
        " Messages: {} user / {} assistant",
        item.user_msgs_count, item.model_msgs_count
    );
    println!("{}\n", "=".repeat(80));

    let agent_name = item.agent.display_name().to_uppercase();

    for step in &steps {
        let role = &step.role;
        let time_str = if !step.time.is_empty() {
            format!(" [{}]", step.time)
        } else {
            String::new()
        };

        if role == "user" {
            let banner_len = render_w.saturating_sub(time_str.len() + 25);
            let banner = format!("── 👤 USER{} {}", time_str, "─".repeat(banner_len));
            println!("\x1b[1;38;5;75m{}\x1b[0m", banner);
            println!("{}", render_ansi(&step.content, render_w));
            println!();
        } else if role == "assistant" {
            let banner_len = render_w.saturating_sub(time_str.len() + agent_name.len() + 8);
            let banner = format!(
                "── 🤖 {}{} {}",
                agent_name,
                time_str,
                "─".repeat(banner_len)
            );
            println!("\x1b[1;38;5;177m{}\x1b[0m", banner);
            println!("{}", render_ansi(&step.content, render_w));
            println!();
        } else if role == "tool_call" {
            for tc in &step.tool_calls {
                println!("\x1b[0;33m  [🛠️ Tool Call: {}]{}\x1b[0m", tc.name, time_str);
            }
            println!();
        }
    }
}

fn terminal_size_cols() -> Option<usize> {
    crossterm::terminal::size().ok().map(|(w, _)| w as usize)
}
