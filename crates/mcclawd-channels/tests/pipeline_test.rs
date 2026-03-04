use chrono::Utc;
use mcclawd_channels::{ChannelKind, InboundMessage, InboundPipeline, MessageContent, Peer};

#[tokio::test]
async fn test_pipeline_receives_messages() {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let mut pipeline = InboundPipeline::new(rx);

    let msg = InboundMessage {
        id: "1".to_string(),
        channel: ChannelKind::Cli,
        peer: Peer {
            id: "user".to_string(),
            display_name: None,
        },
        content: MessageContent::Text("hello".to_string()),
        timestamp: Utc::now(),
    };

    tx.send(msg).await.unwrap();
    let received = pipeline.next().await.unwrap();
    assert!(matches!(received.content, MessageContent::Text(ref t) if t == "hello"));
}
