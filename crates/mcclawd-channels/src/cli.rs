use async_trait::async_trait;
use chrono::Utc;
use std::io::{self, BufRead, Write};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::envelope::Platform;
use crate::registry::ChannelCapabilities;
use crate::types::*;

/// CLI channel adapter — reads from stdin, writes to stdout/stderr.
pub struct CliChannel;

impl CliChannel {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl crate::traits::Channel for CliChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Cli
    }

    async fn start(
        &self,
        inbound_tx: mpsc::Sender<InboundMessage>,
        shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()> {
        let tx = inbound_tx.clone();
        let token = shutdown.clone();

        tokio::task::spawn_blocking(move || {
            let stdin = io::stdin();
            let mut reader = stdin.lock();
            loop {
                if token.is_cancelled() {
                    break;
                }
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim().to_string();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let msg = InboundMessage {
                            id: Uuid::new_v4().to_string(),
                            channel: ChannelKind::Cli,
                            peer: Peer {
                                id: "local".to_string(),
                                display_name: Some("User".to_string()),
                            },
                            content: MessageContent::Text(trimmed),
                            timestamp: Utc::now(),
                        };
                        if tx.blocking_send(msg).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }

    async fn send_chunk(&self, chunk: OutboundChunk) -> mcclawd_core::Result<()> {
        match chunk {
            OutboundChunk::UserMessage(text) => {
                eprintln!("You: {}", text);
            }
            OutboundChunk::TextDelta(text) => {
                print!("{}", text);
                io::stdout().flush().ok();
            }
            OutboundChunk::TextBlock(text) => {
                println!("{}", text);
            }
            OutboundChunk::ToolStart { name } => {
                eprintln!("[tool: {}]", name);
            }
            OutboundChunk::ToolEnd { name, summary } => {
                if let Some(s) = summary {
                    eprintln!("[/{}: {}]", name, s);
                }
            }
            OutboundChunk::Done => {
                println!();
            }
            OutboundChunk::Error(msg) => {
                eprintln!("Error: {}", msg);
            }
            // Phase 2 variants — render as text for CLI
            OutboundChunk::Media {
                mime_type, caption, ..
            } => {
                if let Some(cap) = caption {
                    println!("[media: {} — {}]", mime_type, cap);
                } else {
                    println!("[media: {}]", mime_type);
                }
            }
            OutboundChunk::Buttons { text, buttons } => {
                println!("{}", text);
                for (row_idx, row) in buttons.iter().enumerate() {
                    for (col_idx, btn) in row.iter().enumerate() {
                        let sep = if col_idx + 1 < row.len() { " | " } else { "" };
                        print!("[{}]{}", btn.label, sep);
                    }
                    if row_idx + 1 < buttons.len() {
                        println!();
                    }
                }
                println!();
            }
            OutboundChunk::StatusIndicator(status) => {
                let label = match status {
                    ChannelStatus::Typing => "typing...",
                    ChannelStatus::Processing => "processing...",
                    ChannelStatus::UploadingMedia => "uploading...",
                    ChannelStatus::Done => "done",
                };
                eprintln!("[status: {}]", label);
            }
        }
        Ok(())
    }

    // Phase 2 overrides

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            supports_streaming: true,
            supports_edit: false,
            supports_markdown: true,
            max_message_len: 0, // unlimited
            supports_files: false,
            max_file_size: 0,
        }
    }

    fn platform(&self) -> Platform {
        Platform::Cli
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Channel;

    #[test]
    fn cli_channel_kind() {
        let ch = CliChannel::new();
        assert_eq!(ch.kind(), ChannelKind::Cli);
    }

    #[test]
    fn cli_channel_platform() {
        let ch = CliChannel::new();
        assert_eq!(ch.platform(), Platform::Cli);
    }

    #[test]
    fn cli_channel_capabilities() {
        let ch = CliChannel::new();
        let caps = ch.capabilities();
        assert!(caps.supports_streaming);
        assert!(!caps.supports_edit);
        assert!(caps.supports_markdown);
        assert_eq!(caps.max_message_len, 0);
        assert!(!caps.supports_files);
    }

    #[tokio::test]
    async fn cli_send_chunk_text_delta() {
        let ch = CliChannel::new();
        // TextDelta should not panic
        let result = ch.send_chunk(OutboundChunk::TextDelta("hello".into())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cli_send_chunk_media() {
        let ch = CliChannel::new();
        let result = ch
            .send_chunk(OutboundChunk::Media {
                mime_type: "image/png".into(),
                data: vec![1, 2, 3],
                caption: Some("A chart".into()),
            })
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cli_send_chunk_buttons() {
        let ch = CliChannel::new();
        let result = ch
            .send_chunk(OutboundChunk::Buttons {
                text: "Choose:".into(),
                buttons: vec![vec![
                    InlineButton {
                        label: "Yes".into(),
                        callback_data: Some("yes".into()),
                        url: None,
                    },
                    InlineButton {
                        label: "No".into(),
                        callback_data: Some("no".into()),
                        url: None,
                    },
                ]],
            })
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cli_send_chunk_status() {
        let ch = CliChannel::new();
        let result = ch
            .send_chunk(OutboundChunk::StatusIndicator(ChannelStatus::Typing))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cli_send_chunk_error() {
        let ch = CliChannel::new();
        let result = ch
            .send_chunk(OutboundChunk::Error("something broke".into()))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cli_recv_envelope_returns_none() {
        let mut ch = CliChannel::new();
        let result = ch.recv_envelope().await.unwrap();
        assert!(result.is_none());
    }
}
