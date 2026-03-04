use tokio::sync::mpsc;

use crate::types::InboundMessage;

/// Inbound pipeline: normalize -> route -> dispatch.
/// Phase 0: passthrough (single agent, single channel).
pub struct InboundPipeline {
    rx: mpsc::Receiver<InboundMessage>,
}

impl InboundPipeline {
    pub fn new(rx: mpsc::Receiver<InboundMessage>) -> Self {
        Self { rx }
    }

    pub async fn next(&mut self) -> Option<InboundMessage> {
        self.rx.recv().await
    }
}
