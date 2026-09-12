mod support;

use ai_dialogs::canonical::{
    AgentKind, CanonicalDialogue, CanonicalMessage, CanonicalRole, CanonicalToolCall,
};
use ai_dialogs::providers::{DialogueProvider, codex::CodexProvider};
use std::process::Command;

#[test]
#[ignore = "requires an installed Codex CLI with migrate-rollouts"]
fn generated_rollout_passes_native_codex_inspection() {
    let directory = support::TestDir::new("native-codex");
    let codex_dir = directory.path().join("codex");
    let provider = CodexProvider::with_base_dir(codex_dir.clone());
    let mut dialogue = CanonicalDialogue::new("source", "Native validation", AgentKind::Universal);
    dialogue.metadata.project_path = Some(directory.path().to_string_lossy().into_owned());
    dialogue.messages.push(CanonicalMessage::new(
        CanonicalRole::User,
        "Read the example file",
    ));
    let mut assistant = CanonicalMessage::new(CanonicalRole::Assistant, "Reading the file");
    assistant.tool_calls.push(CanonicalToolCall {
        name: "read_file".into(),
        args: "{\"path\":\"example.rs\"}".into(),
        result: Some("example contents".into()),
    });
    dialogue.messages.push(assistant);
    let id = provider.save_canonical(&dialogue).unwrap();
    let output =
        Command::new(std::env::var_os("CODEX_NATIVE_TEST_BIN").unwrap_or_else(|| "codex".into()))
            .args(["migrate-rollouts", "--json", "--thread", &id])
            .current_dir(directory.path())
            .env("HOME", directory.path())
            .env("USERPROFILE", directory.path())
            .env("CODEX_HOME", &codex_dir)
            .output()
            .expect("installed Codex CLI");
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "{}\n{report}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_str(&report).unwrap();
    let outcomes = parsed["outcomes"].as_array().unwrap();
    assert_eq!(outcomes.len(), 1, "{report}");
    assert_eq!(outcomes[0]["thread_id"], id, "{report}");
    assert_eq!(outcomes[0]["status"], "eligible", "{report}");
}
