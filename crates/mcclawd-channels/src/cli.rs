use async_trait::async_trait;
use chrono::Utc;
use std::io::{self, BufRead, Write};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::types::*;

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
        }
        Ok(())
    }
}
