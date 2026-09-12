# Universal AI Dialogue Manager (`ai-dialogs`)

[![CI](https://github.com/a269ch/ai-dialogs/actions/workflows/ci.yml/badge.svg)](https://github.com/a269ch/ai-dialogs/actions/workflows/ci.yml)
[![Rust Edition](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Universal AI Dialogue Manager (`ai-dialogs`)** is a unified command-line utility, interactive TUI, and cross-agent migration engine for AI coding sessions and dialogues. Written in **Rust** using `Ratatui` and `Crossterm`.

It natively connects, discovers, searches, renders, and **transfers dialogues across different AI agents**:
- **Google Antigravity (`agy`)** (`~/.gemini/antigravity-cli`)
- **Anthropic Claude Code (`claude`)** (`~/.claude/projects/`)
- **OpenAI Codex (`codex`)** (`~/.codex/sessions/`)
- **xAI Grok (`grok`)** (`~/.grok/sessions/`)
- **Universal JSON (`universal`)** (`~/.local/share/ai-dialogs/sessions/`)

---

## ✨ Features

- 🌐 **Universal Cross-Agent Management**:
  - Automatically detects and discovers active sessions across all installed AI agents on your system.
  - Switch agent views instantly in both CLI (`-p/--provider`) and TUI (`P`).
- 🔄 **Cross-Agent Dialogue Migration & Transfer Engine**:
  - Convert conversation text and supported tool records between agents (e.g. Claude Code → Antigravity, Antigravity → OpenAI Codex).
  - Standalone canonical JSON import and export for sharing and archiving dialogues.
- ⚡ **Instant Performance**: Built with Rust — launch time, log parsing, and scanning hundreds of sessions execute almost instantaneously.
- 🎨 **Rich Markdown Rendering**:
  - Code syntax highlighting (Rust, Python, JavaScript/TypeScript, Bash, JSON, HTML).
  - Clean table formatting with accurate Unicode character width calculations.
  - Support for GitHub-style alerts (`[!NOTE]`, `[!TIP]`, `[!WARNING]`, `[!IMPORTANT]`).
  - Automatic filtering of internal noise, tool call records, and checkpoint artifacts.
- 🗑️ **Safe Trash & Recovery**:
  - Two-way soft deletion into timestamped trash storage.
  - Provider-aware deletion, including external transcripts with their original restore paths.
  - Restoration refuses existing destinations and retains backups when a move fails.
  - Instant session restoration back to active dialogues (`--restore` or `U` in TUI).
  - Permanent deletion and trash emptying (`--empty-trash`).
- 🖥️ **Interactive TUI (Ratatui)**:
  - Agent switcher (`P`): filter by `ALL`, `AGY`, `CLAUDE`, `CODEX`, `GROK`, or `UNIV`.
  - Transfer modal (`M`): migrate the selected dialogue to another agent with interactive confirmation.
  - Dual view: Active dialogues and Trash (`Tab` / `T`).
  - Dialogue viewer with smooth vertical and horizontal scrolling.
  - Quick navigation between User (`u`) and Assistant (`m`) messages.
  - In-dialogue text search with highlighted matches (`/`, `n`, `N`).
  - Multi-select sessions with `Space` and batch operations (`B`).
  - View filters (`ALL`, `USER`, `SUBAGENT`, `EMPTY`, `MARKED`, `TRASH`) and sorting modes (`NEWEST`, `OLDEST`, `SIZE`, `MSGS`).
  - Full vim-style navigation (`j`/`k`, `g`/`G`, `h`/`l`) and standard arrow keys.
- 🤖 **Scriptable CLI Mode**:
  - Full automation and pipeline support: `--list`, `--providers`, `--provider`, `--transfer`, `--export-json`, `--import-json`, `--view`, `--delete`, `--restore`, `--export`, `--json`, and `--force`.

---

## 📦 Installation & Building

### Prerequisites
- Current stable Rust toolchain (Edition 2024)
- `cargo`

### Build from Source

```bash
git clone git@github.com:a269ch/ai-dialogs.git
cd ai-dialogs
cargo build --release
```

The compiled binary will be placed at `target/release/ai-dialogs`.

### Install to System

```bash
# Install via cargo
cargo install --path .

# Or copy binary to ~/.local/bin
cp target/release/ai-dialogs ~/.local/bin/ai-dialogs
```

---

## 🚀 CLI Usage

```bash
ai-dialogs [OPTIONS]
```

### Available Options:

| Option | Description |
|---|---|
| `--providers` | Detect and list installed AI agents and their session counts |
| `-p`, `--provider <AGENT>` | Limit listing, JSON output and dialogue operations to an agent (`agy`, `claude`, `codex`, `grok`, `universal`, `all`) |
| `-l`, `--list` | Print dialogues from all providers by default |
| `-t`, `--trash` | Display deleted dialogues currently in trash |
| `-v`, `--view <ID>` | Read and render dialogue Markdown by ID |
| `-d`, `--delete <ID>` | Move dialogue to trash (or permanently delete if in trash) |
| `-r`, `--restore <ID>` | Restore dialogue from trash back to active sessions |
| `-e`, `--export <ID>` | Export dialogue to Markdown file on Desktop |
| `--transfer <ID>` | Transfer dialogue to another agent (requires `--from` and `--to`) |
| `--from <AGENT>` | Source agent for transfer |
| `--to <AGENT>` | Target agent for transfer |
| `--export-json <PATH>` | Export dialogue to a new canonical JSON file (specify ID with `--view` or `--export`) |
| `--import-json <PATH>` | Import dialogue from canonical JSON file (requires `--to <AGENT>`) |
| `--clean-empty` | Find and move all empty sessions (0 messages) to trash |
| `--empty-trash` | Completely empty trash (permanently delete all files) |
| `--force` | Skip interactive confirmation prompts `[y/N]` |
| `--json` | Export active and deleted dialogues data in JSON format |
| `-h`, `--help` | Print help information |

### Examples:

```bash
# Show installed AI agents and session counts
ai-dialogs --providers

# List all sessions across all AI agents
ai-dialogs --list

# List sessions from a specific agent
ai-dialogs --list --provider claude
ai-dialogs --list --provider agy

# View any dialogue with syntax highlighting (auto-detects agent)
ai-dialogs --view 1ffad7c1

# Transfer a Claude Code session to Google Antigravity
ai-dialogs --transfer 1ffad7c1 --from claude --to agy

# Transfer an Antigravity session to OpenAI Codex
ai-dialogs --transfer 4991ce29 --from agy --to codex

# Export dialogue to canonical JSON
ai-dialogs --view 4991ce29 --provider agy --export-json ~/Desktop/my_dialogue.json

# Import canonical JSON into Claude Code
ai-dialogs --import-json ~/Desktop/my_dialogue.json --to claude

# Export to Markdown on Desktop
ai-dialogs --export 4991ce29
```

IDs may be shortened only when the prefix resolves to a single session. When an ID
exists in multiple providers, specify `--provider`; ambiguous matches fail without
changing files. The filter also applies to `--restore`, `--clean-empty`, and
`--empty-trash`.

Transfers create a destination copy with a new ID. Universal imports never use an
imported ID as a path or overwrite an existing session. Canonical JSON exports
also refuse existing output files. Keep canonical JSON archives when exact
provider-specific metadata matters: native formats represent tools and context
differently, and imported histories are not a replacement for an agent's complete
workspace state.

---

## 🎮 Interactive Mode (TUI) Keybindings

Launch TUI without arguments:
```bash
ai-dialogs
```

### Dialogue List:
- `↑` / `k`, `↓` / `j` — Move cursor through dialogues.
- `PgUp`, `PgDn` — Fast page scrolling.
- `Home` / `g`, `End` / `G` — Jump to start / end of list.
- `Enter` / `v` — Open and read selected dialogue.
- `p` / `P` — Cycle active agent filter (`ALL` -> `AGY` -> `CLAUDE` -> `CODEX` -> `GROK` -> `UNIV`).
- `m` / `M` — Open transfer/migration modal to send dialogue to another agent.
- `Space` — Toggle selection mark `[✓]`.
- `a` / `A` — Select all / deselect all.
- `t` / `T` — Toggle between Active dialogues and Trash (`TRASH`).
- `u` / `U` — Restore selected dialogue from trash.
- `d` / `x` — Move to trash / permanently delete (in trash view).
- `b` / `B` — Batch delete all marked sessions.
- `c` / `C` — Empty trash (in trash view) / clean empty sessions (in active view).
- `/` or `s` — Search by ID or dialogue topic / prompts.
- `Esc` — Reset search filter or quit if search is empty.
- `Tab` / `f` — Cycle filter mode (`ALL` -> `USER` -> `SUBAGENT` -> `EMPTY` -> `MARKED` -> `TRASH`).
- `o` / `O` — Cycle sort order (`NEWEST` -> `OLDEST` -> `SIZE` -> `MSGS`).
- `e` / `E` — Export selected dialogue to Markdown.
- `R` / `F5` — Refresh all dialogues from disk.
- `h` / `?` — Display help window.
- `q` / `Q` — Quit application.

### Dialogue Viewer:
- `↑` / `↓` / `←` / `→`, `h`/`j`/`k`/`l` — Free scrolling in all directions.
- `0` — Reset horizontal scroll to beginning of lines.
- `u` — Jump to next user prompt.
- `m` / `a` — Jump to next assistant response.
- `/` — Search text inside dialogue.
- `n` / `N` — Next / previous search match.
- `e` — Quick export current dialogue to Markdown.
- `d` — Delete dialogue directly from viewer.
- `q` / `Esc` — Return to dialogue list.

---

## 🏗️ Storage Layouts

`ai-dialogs` seamlessly interfaces with standard storage directories:

| Agent | Storage Directory | Session Format |
|---|---|---|
| **Google Antigravity** | `~/.gemini/antigravity-cli/brain/<id>/` | JSONL (`transcript.jsonl`) & SQLite (`.db`) |
| **Anthropic Claude Code** | `~/.claude/projects/<project>/<id>.jsonl` | JSONL turns (`type`: `user` / `assistant`) |
| **OpenAI Codex** | `~/.codex/sessions/<year>/<month>/<day>/rollout-*.jsonl` | JSONL rollout sessions |
| **xAI Grok** | `~/.grok/sessions/<id>.json` | Chat JSON format |
| **Universal Format** | `~/.local/share/ai-dialogs/sessions/<id>.json` | Standardized Canonical Dialogue JSON |

Universal storage uses the platform data directory: on macOS it is
`~/Library/Application Support/ai-dialogs/sessions`, on Windows
`%APPDATA%/ai-dialogs/sessions`, and on Linux `$XDG_DATA_HOME/ai-dialogs/sessions`
(defaulting to `~/.local/share`). Run `--providers` to see resolved paths.

## Development checks

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release
```

Regression tests use temporary provider directories. CLI integration tests launch
the binary with an isolated home and data directory; they do not use installed
conversation histories. The suite covers import collisions and unsafe IDs,
restoration conflicts and failed moves, provider filters, tool records, historical
timestamps, Unicode search, and terminal viewport navigation.

With a Codex CLI that supports `migrate-rollouts` installed, run its optional
native compatibility check (inspection only, no model requests or migration):

```bash
cargo test --test codex_native -- --ignored
```

Set `CODEX_NATIVE_TEST_BIN` if the executable is not named `codex` on `PATH`.

---

## 📄 License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.
