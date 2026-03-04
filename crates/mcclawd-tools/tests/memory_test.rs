//! Integration tests for builtin memory tools.

use mcclawd_tools::builtin::memory::{MemoryRecall, MemoryStore};
use rig::tool::Tool;

#[tokio::test]
async fn test_memory_store_and_recall() {
    let store_tool = MemoryStore::new_shared();
    let recall_tool = MemoryRecall::from_shared(&store_tool);

    // Store a value
    let store_args = serde_json::from_value(serde_json::json!({
        "key": "user_name",
        "value": "Alice"
    }))
    .unwrap();
    let result = store_tool.call(store_args).await.unwrap();
    assert!(result.contains("Stored"));

    // Recall the value
    let recall_args = serde_json::from_value(serde_json::json!({
        "key": "user_name"
    }))
    .unwrap();
    let result = recall_tool.call(recall_args).await.unwrap();
    assert!(result.contains("Alice"));
}

#[tokio::test]
async fn test_memory_recall_missing_key() {
    let store_tool = MemoryStore::new_shared();
    let recall_tool = MemoryRecall::from_shared(&store_tool);

    let recall_args = serde_json::from_value(serde_json::json!({
        "key": "nonexistent"
    }))
    .unwrap();
    let result = recall_tool.call(recall_args).await.unwrap();
    assert!(result.contains("No value found"));
}

#[tokio::test]
async fn test_memory_overwrite() {
    let store_tool = MemoryStore::new_shared();
    let recall_tool = MemoryRecall::from_shared(&store_tool);

    // Store initial value
    let args = serde_json::from_value(serde_json::json!({
        "key": "lang",
        "value": "Rust"
    }))
    .unwrap();
    store_tool.call(args).await.unwrap();

    // Overwrite
    let args = serde_json::from_value(serde_json::json!({
        "key": "lang",
        "value": "Zig"
    }))
    .unwrap();
    store_tool.call(args).await.unwrap();

    // Recall should return latest value
    let recall_args = serde_json::from_value(serde_json::json!({
        "key": "lang"
    }))
    .unwrap();
    let result = recall_tool.call(recall_args).await.unwrap();
    assert_eq!(result, "Zig");
}
