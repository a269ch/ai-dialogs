mod support;

use ai_dialogs::canonical::{AgentKind, CanonicalDialogue, CanonicalMessage, CanonicalRole};
use ai_dialogs::providers::{
    ProviderRegistry, antigravity::AntigravityProvider, claude::ClaudeProvider,
    codex::CodexProvider, grok::GrokProvider, universal::UniversalProvider,
};
use ai_dialogs::transfer::TransferEngine;

#[test]
fn every_provider_pair_preserves_conversation_text_roles_and_timestamps() {
    let fixture = support::TestDir::new("matrix");
    let mut registry = ProviderRegistry::empty();
    registry.register(Box::new(AntigravityProvider::with_base_dir(
        fixture.path().join("agy"),
    )));
    registry.register(Box::new(ClaudeProvider::with_base_dir(
        fixture.path().join("claude"),
    )));
    registry.register(Box::new(CodexProvider::with_base_dir(
        fixture.path().join("codex"),
    )));
    registry.register(Box::new(GrokProvider::with_base_dir(
        fixture.path().join("grok"),
    )));
    registry.register(Box::new(UniversalProvider::with_base_dir(
        fixture.path().join("universal"),
    )));
    let mut dialogue =
        CanonicalDialogue::new("source", "Исторический диалог", AgentKind::Universal);
    dialogue.created_at = Some("2022-01-02T03:04:05+03:00".into());
    dialogue.updated_at = Some("2022-01-02T03:05:06+03:00".into());
    let mut user = CanonicalMessage::new(
        CanonicalRole::User,
        "Исторический диалог\nПроверь İstanbul и русский текст.",
    );
    user.timestamp = dialogue.created_at.clone();
    let mut assistant = CanonicalMessage::new(
        CanonicalRole::Assistant,
        "Ответ сохранён.\n```rust\nlet x = 1;\n```",
    );
    assistant.timestamp = dialogue.updated_at.clone();
    dialogue.messages = vec![user, assistant];

    for source in AgentKind::ALL {
        let source_id = registry
            .get(source)
            .unwrap()
            .save_canonical(&dialogue)
            .unwrap();
        for target in AgentKind::ALL {
            let result = TransferEngine::transfer(&registry, source, target, &source_id).unwrap();
            assert_ne!(result.target_id, source_id, "{source} -> {target}");
            assert_eq!(result.messages_count, 2, "{source} -> {target}");
            let (_, listed) = registry
                .find_dialogue_in(&result.target_id, Some(target))
                .unwrap()
                .unwrap();
            let loaded = registry
                .get(target)
                .unwrap()
                .load_canonical(&listed.id)
                .unwrap();
            assert_eq!(
                loaded.created_at, dialogue.created_at,
                "{source} -> {target}"
            );
            assert_eq!(
                loaded.updated_at, dialogue.updated_at,
                "{source} -> {target}"
            );
            assert_eq!(
                loaded.messages.len(),
                dialogue.messages.len(),
                "{source} -> {target}"
            );
            for (actual, expected) in loaded.messages.iter().zip(&dialogue.messages) {
                assert_eq!(actual.role, expected.role, "{source} -> {target}");
                assert_eq!(actual.content, expected.content, "{source} -> {target}");
                assert_eq!(actual.timestamp, expected.timestamp, "{source} -> {target}");
            }
        }
    }
}
