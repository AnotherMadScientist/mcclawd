//! Task 25: Channel registry integration test.

use mcclawd_channels::envelope::Platform;
use mcclawd_channels::registry::{ChannelCapabilities, ChannelEntry, ChannelId, ChannelRegistry};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_entry(id: &str, platform: Platform, caps: ChannelCapabilities) -> ChannelEntry {
    ChannelEntry {
        id: ChannelId::new(id),
        platform,
        capabilities: caps,
        enabled: true,
    }
}

fn telegram_caps() -> ChannelCapabilities {
    ChannelCapabilities {
        supports_streaming: false,
        supports_edit: true,
        supports_markdown: true,
        max_message_len: 4096,
        supports_files: true,
        max_file_size: 50 * 1024 * 1024,
    }
}

fn cli_caps() -> ChannelCapabilities {
    ChannelCapabilities {
        supports_streaming: true,
        supports_edit: false,
        supports_markdown: true,
        max_message_len: 0, // unlimited
        supports_files: false,
        max_file_size: 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn register_multiple_channels() {
    let mut reg = ChannelRegistry::new();

    let tg = make_entry("tg-main", Platform::Telegram, telegram_caps());
    let cli = make_entry("cli-local", Platform::Cli, cli_caps());
    let web = make_entry("web-ui", Platform::Web, ChannelCapabilities::default());

    assert!(reg.register(tg));
    assert!(reg.register(cli));
    assert!(reg.register(web));
    assert_eq!(reg.len(), 3);
}

#[test]
fn lookup_by_id_returns_correct_entry() {
    let mut reg = ChannelRegistry::new();
    reg.register(make_entry("tg-main", Platform::Telegram, telegram_caps()));
    reg.register(make_entry("cli-local", Platform::Cli, cli_caps()));

    let found = reg.get(&ChannelId::new("tg-main"));
    assert!(found.is_some());
    let entry = found.unwrap();
    assert_eq!(entry.platform, Platform::Telegram);
    assert_eq!(entry.capabilities.max_message_len, 4096);
    assert!(entry.capabilities.supports_edit);
    assert!(entry.capabilities.supports_files);
}

#[test]
fn lookup_nonexistent_returns_none() {
    let reg = ChannelRegistry::new();
    assert!(reg.get(&ChannelId::new("no-such-channel")).is_none());
}

#[test]
fn verify_capabilities_per_channel() {
    let mut reg = ChannelRegistry::new();
    reg.register(make_entry("tg-main", Platform::Telegram, telegram_caps()));
    reg.register(make_entry("cli-local", Platform::Cli, cli_caps()));

    // Telegram capabilities
    let tg = reg.get(&ChannelId::new("tg-main")).unwrap();
    assert!(tg.capabilities.supports_edit);
    assert!(tg.capabilities.supports_files);
    assert!(!tg.capabilities.supports_streaming);
    assert_eq!(tg.capabilities.max_message_len, 4096);

    // CLI capabilities
    let cli = reg.get(&ChannelId::new("cli-local")).unwrap();
    assert!(cli.capabilities.supports_streaming);
    assert!(!cli.capabilities.supports_edit);
    assert!(!cli.capabilities.supports_files);
    assert_eq!(cli.capabilities.max_message_len, 0); // unlimited
}

#[test]
fn deregister_removes_and_returns_entry() {
    let mut reg = ChannelRegistry::new();
    reg.register(make_entry("tg-main", Platform::Telegram, telegram_caps()));
    reg.register(make_entry("cli-local", Platform::Cli, cli_caps()));
    assert_eq!(reg.len(), 2);

    let removed = reg.unregister(&ChannelId::new("tg-main"));
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().platform, Platform::Telegram);
    assert_eq!(reg.len(), 1);

    // Verify it is gone
    assert!(reg.get(&ChannelId::new("tg-main")).is_none());
    // Other channel still present
    assert!(reg.get(&ChannelId::new("cli-local")).is_some());
}

#[test]
fn deregister_nonexistent_returns_none() {
    let mut reg = ChannelRegistry::new();
    assert!(reg.unregister(&ChannelId::new("ghost")).is_none());
}

#[test]
fn duplicate_registration_rejected() {
    let mut reg = ChannelRegistry::new();
    let entry1 = make_entry("ch-1", Platform::Cli, cli_caps());
    let entry2 = make_entry("ch-1", Platform::Telegram, telegram_caps());

    assert!(reg.register(entry1));
    assert!(!reg.register(entry2), "duplicate ID should be rejected");
    assert_eq!(reg.len(), 1);

    // Original entry preserved
    let found = reg.get(&ChannelId::new("ch-1")).unwrap();
    assert_eq!(found.platform, Platform::Cli);
}

#[test]
fn list_returns_all_entries() {
    let mut reg = ChannelRegistry::new();
    reg.register(make_entry("a", Platform::Cli, cli_caps()));
    reg.register(make_entry("b", Platform::Telegram, telegram_caps()));
    reg.register(make_entry("c", Platform::Web, ChannelCapabilities::default()));

    let all = reg.list();
    assert_eq!(all.len(), 3);

    let ids: Vec<String> = all.iter().map(|e| e.id.0.clone()).collect();
    assert!(ids.contains(&"a".to_string()));
    assert!(ids.contains(&"b".to_string()));
    assert!(ids.contains(&"c".to_string()));
}

#[test]
fn is_empty_reflects_state() {
    let mut reg = ChannelRegistry::new();
    assert!(reg.is_empty());

    reg.register(make_entry("x", Platform::Discord, ChannelCapabilities::default()));
    assert!(!reg.is_empty());

    reg.unregister(&ChannelId::new("x"));
    assert!(reg.is_empty());
}

#[test]
fn register_then_unregister_then_re_register() {
    let mut reg = ChannelRegistry::new();

    reg.register(make_entry("ch", Platform::Cli, cli_caps()));
    reg.unregister(&ChannelId::new("ch"));
    assert!(reg.is_empty());

    // Re-register with different platform should succeed
    let new_entry = make_entry("ch", Platform::Telegram, telegram_caps());
    assert!(reg.register(new_entry));
    assert_eq!(reg.get(&ChannelId::new("ch")).unwrap().platform, Platform::Telegram);
}
