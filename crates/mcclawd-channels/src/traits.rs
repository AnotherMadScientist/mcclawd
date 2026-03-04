use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::types::*;

#[async_trait]
pub trait Channel: Send + Sync + 'static {
    fn kind(&self) -> ChannelKind;

    async fn start(
        &self,
        inbound_tx: mpsc::Sender<InboundMessage>,
        shutdown: CancellationToken,
    ) -> mcclawd_core::Result<()>;

    async fn send_chunk(&self, chunk: OutboundChunk) -> mcclawd_core::Result<()>;
}
