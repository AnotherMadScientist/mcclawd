//! JSONL protocol — emits OutboundChunk as one JSON object per line to stdout.
//!
//! Stdout carries the structured protocol. Stderr is reserved for tracing/logs.

use mcclawd_channels::OutboundChunk;

/// Emit a single OutboundChunk as a JSON line to stdout.
pub fn emit(chunk: &OutboundChunk) {
    let json = serde_json::to_string(chunk).expect("OutboundChunk is always serializable");
    println!("{json}");
}
