use mcclawd_channels::{ChannelStatus, OutboundChunk};

/// Helper: serialize to JSON, verify single-line, deserialize back
fn roundtrip(chunk: &OutboundChunk) -> OutboundChunk {
    let json = serde_json::to_string(chunk).expect("serialize");
    assert!(!json.contains('\n'), "JSONL must be single-line: {json}");
    serde_json::from_str(&json).expect("deserialize")
}

#[test]
fn text_delta_roundtrip() {
    let chunk = OutboundChunk::TextDelta("Hello world".to_string());
    let back = roundtrip(&chunk);
    match back {
        OutboundChunk::TextDelta(t) => assert_eq!(t, "Hello world"),
        other => panic!("Expected TextDelta, got {other:?}"),
    }
}

#[test]
fn text_block_roundtrip() {
    let chunk = OutboundChunk::TextBlock("Complete response".to_string());
    let back = roundtrip(&chunk);
    match back {
        OutboundChunk::TextBlock(t) => assert_eq!(t, "Complete response"),
        other => panic!("Expected TextBlock, got {other:?}"),
    }
}

#[test]
fn tool_start_roundtrip() {
    let chunk = OutboundChunk::ToolStart {
        name: "memory.store".to_string(),
    };
    let back = roundtrip(&chunk);
    match back {
        OutboundChunk::ToolStart { name } => assert_eq!(name, "memory.store"),
        other => panic!("Expected ToolStart, got {other:?}"),
    }
}

#[test]
fn tool_end_roundtrip() {
    let chunk = OutboundChunk::ToolEnd {
        name: "memory.store".to_string(),
        summary: Some("Stored value".to_string()),
    };
    let back = roundtrip(&chunk);
    match back {
        OutboundChunk::ToolEnd { name, summary } => {
            assert_eq!(name, "memory.store");
            assert_eq!(summary.as_deref(), Some("Stored value"));
        }
        other => panic!("Expected ToolEnd, got {other:?}"),
    }
}

#[test]
fn done_roundtrip() {
    let chunk = OutboundChunk::Done;
    let back = roundtrip(&chunk);
    assert!(matches!(back, OutboundChunk::Done));
}

#[test]
fn error_roundtrip() {
    let chunk = OutboundChunk::Error("Something went wrong".to_string());
    let back = roundtrip(&chunk);
    match back {
        OutboundChunk::Error(msg) => assert_eq!(msg, "Something went wrong"),
        other => panic!("Expected Error, got {other:?}"),
    }
}

#[test]
fn status_indicator_roundtrip() {
    let chunk = OutboundChunk::StatusIndicator(ChannelStatus::Processing);
    let back = roundtrip(&chunk);
    assert!(matches!(
        back,
        OutboundChunk::StatusIndicator(ChannelStatus::Processing)
    ));
}

#[test]
fn user_message_roundtrip() {
    let chunk = OutboundChunk::UserMessage("What is 2+2?".to_string());
    let back = roundtrip(&chunk);
    match back {
        OutboundChunk::UserMessage(msg) => assert_eq!(msg, "What is 2+2?"),
        other => panic!("Expected UserMessage, got {other:?}"),
    }
}

#[test]
fn text_with_special_chars() {
    let chunk = OutboundChunk::TextDelta("line1\nline2\ttab\"quoted\"".to_string());
    let json = serde_json::to_string(&chunk).unwrap();
    // JSON string escaping should handle newlines/tabs — the raw JSON line must not contain literal newlines
    assert!(
        !json.contains('\n'),
        "JSON must not contain literal newlines"
    );
    let back: OutboundChunk = serde_json::from_str(&json).unwrap();
    match back {
        OutboundChunk::TextDelta(t) => assert_eq!(t, "line1\nline2\ttab\"quoted\""),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn usage_roundtrip() {
    let chunk = OutboundChunk::Usage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        model: Some("claude-sonnet-4-5".to_string()),
    };
    let back = roundtrip(&chunk);
    match back {
        OutboundChunk::Usage {
            input_tokens,
            output_tokens,
            total_tokens,
            model,
        } => {
            assert_eq!(input_tokens, 100);
            assert_eq!(output_tokens, 50);
            assert_eq!(total_tokens, 150);
            assert_eq!(model.as_deref(), Some("claude-sonnet-4-5"));
        }
        other => panic!("Expected Usage, got {other:?}"),
    }
}

#[test]
fn chat_history_roundtrip() {
    let chunk = OutboundChunk::ChatHistory("[]".to_string());
    let back = roundtrip(&chunk);
    match back {
        OutboundChunk::ChatHistory(h) => assert_eq!(h, "[]"),
        other => panic!("Expected ChatHistory, got {other:?}"),
    }
}

#[test]
fn generated_files_roundtrip() {
    use mcclawd_channels::AttachmentInfo;
    let chunk = OutboundChunk::GeneratedFiles(vec![AttachmentInfo {
        name: "output.csv".to_string(),
        size: 2048,
        content_type: "text/csv".to_string(),
        url: "/api/tasks/abc/files/output.csv".to_string(),
    }]);
    let back = roundtrip(&chunk);
    match back {
        OutboundChunk::GeneratedFiles(files) => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].name, "output.csv");
            assert_eq!(files[0].size, 2048);
            assert_eq!(files[0].content_type, "text/csv");
        }
        other => panic!("Expected GeneratedFiles, got {other:?}"),
    }
}
