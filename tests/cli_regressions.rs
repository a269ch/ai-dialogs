mod support;

use ai_dialogs::canonical::{AgentKind, CanonicalDialogue, CanonicalMessage, CanonicalRole};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    root: PathBuf,
    _directory: support::TestDir,
}

impl Fixture {
    fn new() -> Self {
        let directory = support::TestDir::new("cli");
        Self {
            root: directory.path().to_path_buf(),
            _directory: directory,
        }
    }

    fn universal_dir(&self) -> PathBuf {
        let data = if cfg!(target_os = "macos") {
            self.root.join("Library/Application Support")
        } else {
            self.root.join("data")
        };
        data.join("ai-dialogs/sessions")
    }

    fn write(&self, path: &Path, contents: impl AsRef<[u8]>) {
        assert!(path.starts_with(&self.root));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn seed(&self) {
        let id = "shared-session";
        self.write(&self.root.join(format!(".gemini/antigravity-cli/brain/{id}/transcript.jsonl")),
            format!("{}\n", json!({"type":"USER_INPUT","source":"USER_EXPLICIT","content":"Antigravity prompt","created_at":"2024-01-02T03:04:05Z"})));
        self.write(&self.root.join(format!(".claude/projects/project/{id}.jsonl")),
            format!("{}\n", json!({"type":"user","timestamp":"2024-01-02T03:04:05Z","message":{"role":"user","content":"Claude prompt"}})));
        self.write(&self.root.join(".codex/sessions/2024/01/02/rollout-2024-01-02T03-04-05-12345678-1234-4234-8234-123456789abc.jsonl"),
            format!("{}\n{}\n{}\n",
                json!({"type":"session_meta","timestamp":"2024-01-02T03:04:05Z","payload":{"id":"12345678-1234-4234-8234-123456789abc","timestamp":"2024-01-02T03:04:05Z","cwd":"/tmp"}}),
                json!({"type":"response_item","timestamp":"2024-01-02T03:04:06Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Codex prompt"}]}}),
                json!({"type":"response_item","timestamp":"2024-01-02T03:04:07Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Codex answer"}]}})));
        self.write(&self.root.join(".grok/sessions/grok-session.json"),
            json!({"title":"Grok title","created_at":"2024-01-02T03:04:05Z","messages":[{"role":"user","content":"Grok prompt"}]}).to_string());
        let mut dialogue = CanonicalDialogue::new(id, "Universal title", AgentKind::Universal);
        dialogue.messages.push(CanonicalMessage::new(
            CanonicalRole::User,
            "Universal prompt",
        ));
        self.write(
            &self.universal_dir().join(format!("{id}.json")),
            dialogue.to_json_pretty().unwrap(),
        );
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ai-dialogs"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.root)
            .env("USERPROFILE", &self.root)
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("APPDATA", self.root.join("data"))
            .env("LOCALAPPDATA", self.root.join("local-data"))
            .output()
            .unwrap()
    }

    fn success(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "args={args:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

#[test]
fn cli_inventory_and_view_cover_all_providers() {
    let fixture = Fixture::new();
    fixture.seed();
    let all: Value = serde_json::from_str(&fixture.success(&["--json"])).unwrap();
    assert_eq!(all["active"].as_array().unwrap().len(), 5);
    let table = fixture.success(&["--list"]);
    for agent in ["agy", "claude", "codex", "grok", "universal"] {
        assert!(table.contains(agent));
        let filtered: Value =
            serde_json::from_str(&fixture.success(&["--json", "--provider", agent])).unwrap();
        let items = filtered["active"].as_array().unwrap();
        assert_eq!(items.len(), 1, "{agent}");
        assert_eq!(items[0]["agent"], agent);
    }
    let view = fixture.success(&["--view", "12345678", "--provider", "codex"]);
    assert!(view.contains("Codex prompt") && view.contains("Codex answer"));
    let positional_view = fixture.success(&["view", "12345678", "--provider", "codex"]);
    assert_eq!(view, positional_view);
    let missing_open = fixture.run(&["open"]);
    assert!(!missing_open.status.success());
    let err_str = String::from_utf8_lossy(&missing_open.stderr);
    assert!(err_str.contains("Specify dialogue ID"));
    let universal = fixture.success(&["--view", "shared-session", "--provider", "universal"]);
    assert!(universal.contains("Universal prompt"));
    assert!(!universal.contains("Claude prompt") && !universal.contains("Antigravity prompt"));
}

#[test]
fn filtered_delete_restore_and_export_never_touch_another_provider() {
    let fixture = Fixture::new();
    fixture.seed();
    let source = fixture
        .root
        .join(".gemini/antigravity-cli/brain/shared-session/transcript.jsonl");
    let original = fs::read(&source).unwrap();
    let universal_path = fixture.universal_dir().join("shared-session.json");
    let universal_original = fs::read(&universal_path).unwrap();
    assert!(
        !fixture
            .run(&["--delete", "shared-session", "--force"])
            .status
            .success()
    );
    assert!(
        !fixture
            .run(&[
                "--delete",
                "shared-session",
                "--provider",
                "typo",
                "--force"
            ])
            .status
            .success()
    );

    let export = fixture.root.join("export.json");
    fixture.success(&[
        "--view",
        "shared-session",
        "--provider",
        "universal",
        "--export-json",
        export.to_str().unwrap(),
    ]);
    let exported: CanonicalDialogue = serde_json::from_slice(&fs::read(&export).unwrap()).unwrap();
    assert_eq!(exported.title, "Universal title");
    let output = fixture.success(&["--export", "shared-session", "--provider", "universal"]);
    let markdown_path = output
        .trim()
        .strip_prefix("Dialogue exported to: ")
        .unwrap();
    let markdown = fs::read_to_string(markdown_path).unwrap();
    assert!(markdown.contains("Universal prompt"));
    assert!(!markdown.contains("Antigravity prompt"));

    fixture.success(&[
        "--delete",
        "shared-session",
        "--provider",
        "universal",
        "--force",
    ]);
    assert!(!universal_path.exists());
    assert_eq!(fs::read(&source).unwrap(), original);
    let trash: Value =
        serde_json::from_str(&fixture.success(&["--json", "--provider", "universal"])).unwrap();
    assert_eq!(trash["active"].as_array().unwrap().len(), 0);
    assert_eq!(trash["trash"].as_array().unwrap().len(), 1);
    assert_eq!(trash["trash"][0]["agent"], "universal");
    fixture.success(&["--restore", "shared-session", "--provider", "universal"]);
    assert_eq!(fs::read(&universal_path).unwrap(), universal_original);
    assert_eq!(fs::read(source).unwrap(), original);
}

#[test]
fn transferred_codex_id_is_immediately_resolvable_and_keeps_history_dates() {
    let fixture = Fixture::new();
    fixture.seed();
    fixture.success(&[
        "--transfer",
        "shared-session",
        "--from",
        "claude",
        "--to",
        "codex",
    ]);
    let inventory: Value =
        serde_json::from_str(&fixture.success(&["--json", "--provider", "codex"])).unwrap();
    let imported = inventory["active"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["topic"].as_str().unwrap().contains("Claude prompt"))
        .unwrap();
    let id = imported["id"].as_str().unwrap();
    assert_eq!(id.len(), 36);
    assert!(
        fixture
            .success(&["--view", id, "--provider", "codex"])
            .contains("Claude prompt")
    );
    let export = fixture.root.join("codex-export.json");
    fixture.success(&[
        "--view",
        id,
        "--provider",
        "codex",
        "--export-json",
        export.to_str().unwrap(),
    ]);
    let dialogue: CanonicalDialogue = serde_json::from_slice(&fs::read(export).unwrap()).unwrap();
    assert_eq!(dialogue.created_at.as_deref(), Some("2024-01-02T03:04:05Z"));
    assert_eq!(dialogue.messages[0].content, "Claude prompt");
}
