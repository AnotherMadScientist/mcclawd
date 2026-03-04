//! WhatsApp Cloud API webhook handler and payload parser.
//!
//! This module provides:
//!
//! 1. **Always available**: [`parse_webhook_payload`] parses raw WhatsApp Cloud API
//!    webhook JSON into [`WhatsAppMessage`] without any SDK dependency.
//!    [`parse_verification_request`] handles the webhook verification challenge.
//!
//! 2. **Feature-gated (`webhook-crypto`)**: [`verify_webhook_signature`] validates
//!    the X-Hub-Signature-256 header using HMAC-SHA256.

use chrono::{TimeZone, Utc};

use crate::normalize::{WhatsAppMedia, WhatsAppMessage};

// ---------------------------------------------------------------------------
// Webhook payload parsing (always available)
// ---------------------------------------------------------------------------

/// Parse a WhatsApp Cloud API webhook payload into a list of [`WhatsAppMessage`]s.
///
/// WhatsApp sends batched changes in this format:
/// ```json
/// {
///   "object": "whatsapp_business_account",
///   "entry": [{
///     "id": "BUSINESS_ID",
///     "changes": [{
///       "value": {
///         "messaging_product": "whatsapp",
///         "metadata": { "phone_number_id": "...", "display_phone_number": "..." },
///         "contacts": [{ "profile": { "name": "Alice" }, "wa_id": "14155552671" }],
///         "messages": [{
///           "from": "14155552671",
///           "id": "wamid.abc123",
///           "timestamp": "1700000000",
///           "type": "text",
///           "text": { "body": "Hello!" }
///         }]
///       }
///     }]
///   }]
/// }
/// ```
pub fn parse_webhook_payload(json: &serde_json::Value) -> Vec<WhatsAppMessage> {
    let mut results = Vec::new();

    let entries = match json.get("entry").and_then(|e| e.as_array()) {
        Some(e) => e,
        None => return results,
    };

    for entry in entries {
        let changes = match entry.get("changes").and_then(|c| c.as_array()) {
            Some(c) => c,
            None => continue,
        };

        for change in changes {
            let value = match change.get("value") {
                Some(v) => v,
                None => continue,
            };

            // Build a contact name lookup: wa_id -> profile name.
            let mut contact_names = std::collections::HashMap::new();
            if let Some(contacts) = value.get("contacts").and_then(|c| c.as_array()) {
                for contact in contacts {
                    if let (Some(wa_id), Some(name)) = (
                        contact.get("wa_id").and_then(|v| v.as_str()),
                        contact
                            .get("profile")
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str()),
                    ) {
                        contact_names.insert(wa_id.to_string(), name.to_string());
                    }
                }
            }

            let messages = match value.get("messages").and_then(|m| m.as_array()) {
                Some(m) => m,
                None => continue,
            };

            for msg in messages {
                if let Some(parsed) = parse_single_message(msg, &contact_names) {
                    results.push(parsed);
                }
            }
        }
    }

    results
}

/// Parse a single message object from the webhook payload.
fn parse_single_message(
    msg: &serde_json::Value,
    contact_names: &std::collections::HashMap<String, String>,
) -> Option<WhatsAppMessage> {
    let message_id = msg.get("id")?.as_str()?.to_string();
    let from = msg.get("from")?.as_str()?.to_string();
    let from_name = contact_names.get(&from).cloned();

    let timestamp_str = msg.get("timestamp").and_then(|v| v.as_str()).unwrap_or("0");
    let timestamp_secs: i64 = timestamp_str.parse().unwrap_or(0);
    let timestamp = Utc
        .timestamp_opt(timestamp_secs, 0)
        .single()
        .unwrap_or_else(Utc::now);

    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

    let (text, media) = match msg_type {
        "text" => {
            let body = msg
                .get("text")
                .and_then(|t| t.get("body"))
                .and_then(|v| v.as_str())
                .map(String::from);
            (body, None)
        }
        "image" | "video" | "audio" | "document" | "sticker" => {
            let media_obj = msg.get(msg_type)?;
            let media = WhatsAppMedia {
                id: media_obj
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                mime_type: media_obj
                    .get("mime_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream")
                    .to_string(),
                filename: media_obj
                    .get("filename")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            };
            let caption = media_obj
                .get("caption")
                .and_then(|v| v.as_str())
                .map(String::from);
            (caption, Some(media))
        }
        _ => (None, None),
    };

    Some(WhatsAppMessage {
        message_id,
        from,
        from_name,
        text,
        media,
        timestamp,
    })
}

/// Parse a WhatsApp webhook verification request.
///
/// When setting up the webhook, Meta sends a GET with query parameters:
/// - `hub.mode=subscribe`
/// - `hub.verify_token=<your_token>`
/// - `hub.challenge=<challenge_string>`
///
/// This function checks the mode and token, returning the challenge on success.
pub fn parse_verification_request(
    mode: &str,
    verify_token: &str,
    expected_token: &str,
    challenge: &str,
) -> Option<String> {
    if mode == "subscribe" && verify_token == expected_token {
        Some(challenge.to_string())
    } else {
        None
    }
}

/// Check if a phone number is in the allowed list.
pub fn is_number_allowed(from: &str, allowed: &Option<Vec<String>>) -> bool {
    match allowed {
        None => true,
        Some(numbers) => numbers.iter().any(|n| n == from),
    }
}

// ---------------------------------------------------------------------------
// Webhook signature verification (feature-gated)
// ---------------------------------------------------------------------------

/// Verify the WhatsApp webhook signature (X-Hub-Signature-256 header).
///
/// The signature is `sha256=<hex_digest>` where the digest is HMAC-SHA256
/// of the raw request body using the app secret as the key.
///
/// Requires the `webhook-crypto` feature for real HMAC verification.
/// Without it, this function always returns `false` and logs a warning.
#[cfg(feature = "webhook-crypto")]
pub fn verify_webhook_signature(payload: &[u8], signature: &str, app_secret: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let sig_hex = match signature.strip_prefix("sha256=") {
        Some(hex) => hex,
        None => return false,
    };

    let expected = match hex_decode(sig_hex) {
        Some(bytes) => bytes,
        None => return false,
    };

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = match HmacSha256::new_from_slice(app_secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(payload);

    mac.verify_slice(&expected).is_ok()
}

#[cfg(not(feature = "webhook-crypto"))]
pub fn verify_webhook_signature(_payload: &[u8], _signature: &str, _app_secret: &str) -> bool {
    tracing::warn!(
        "webhook-crypto feature not enabled; signature verification always fails. \
         Enable the `webhook-crypto` feature for HMAC-SHA256 verification."
    );
    false
}

/// Simple hex decode helper (avoids adding a hex crate dependency).
#[allow(dead_code)]
fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// Outbound formatting
// ---------------------------------------------------------------------------

use mcclawd_channels::types::OutboundChunk;

/// Format an [`OutboundChunk`] as a WhatsApp Cloud API JSON payload.
///
/// The `platform_meta` should contain `from` (recipient phone number)
/// and `phone_number_id` from the inbound envelope.
/// Returns a JSON object suitable for the Messages API.
/// Returns `None` for chunks that have no WhatsApp representation.
pub fn format_outbound(
    chunk: &OutboundChunk,
    platform_meta: &serde_json::Value,
) -> Option<serde_json::Value> {
    let to = platform_meta.get("from")?.as_str()?;
    let phone_number_id = platform_meta
        .get("phone_number_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match chunk {
        OutboundChunk::TextBlock(text) | OutboundChunk::TextDelta(text) => {
            Some(serde_json::json!({
                "messaging_product": "whatsapp",
                "recipient_type": "individual",
                "to": to,
                "type": "text",
                "text": { "body": text },
                "_phone_number_id": phone_number_id,
            }))
        }
        OutboundChunk::Media {
            mime_type, caption, ..
        } => {
            let media_type = if mime_type.starts_with("image/") {
                "image"
            } else if mime_type.starts_with("video/") {
                "video"
            } else if mime_type.starts_with("audio/") {
                "audio"
            } else {
                "document"
            };
            let mut media_obj = serde_json::json!({});
            if let Some(c) = caption {
                media_obj["caption"] = serde_json::Value::String(c.clone());
            }
            Some(serde_json::json!({
                "messaging_product": "whatsapp",
                "recipient_type": "individual",
                "to": to,
                "type": media_type,
                media_type: media_obj,
                "_phone_number_id": phone_number_id,
            }))
        }
        OutboundChunk::Buttons { text, buttons } => {
            // WhatsApp interactive buttons (max 3 per message).
            let button_list: Vec<serde_json::Value> = buttons
                .iter()
                .flatten()
                .take(3) // WhatsApp limit
                .enumerate()
                .map(|(i, btn)| {
                    serde_json::json!({
                        "type": "reply",
                        "reply": {
                            "id": btn.callback_data.clone().unwrap_or_else(|| format!("btn_{}", i)),
                            "title": btn.label,
                        },
                    })
                })
                .collect();
            Some(serde_json::json!({
                "messaging_product": "whatsapp",
                "recipient_type": "individual",
                "to": to,
                "type": "interactive",
                "interactive": {
                    "type": "button",
                    "body": { "text": text },
                    "action": { "buttons": button_list },
                },
                "_phone_number_id": phone_number_id,
            }))
        }
        OutboundChunk::StatusIndicator(_) => {
            // WhatsApp doesn't support typing indicators via Cloud API.
            None
        }
        OutboundChunk::Error(msg) => Some(serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "text",
            "text": { "body": format!("Error: {}", msg) },
            "_phone_number_id": phone_number_id,
        })),
        OutboundChunk::UserMessage(_) | OutboundChunk::ToolStart { .. } | OutboundChunk::ToolEnd { .. } | OutboundChunk::Done => {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mcclawd_channels::types::ChannelStatus;
    use serde_json::json;

    fn sample_webhook_payload() -> serde_json::Value {
        json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "BIZ_ID",
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp",
                        "metadata": {
                            "phone_number_id": "PHONE_ID",
                            "display_phone_number": "15551234567"
                        },
                        "contacts": [{
                            "profile": { "name": "Alice" },
                            "wa_id": "14155552671"
                        }],
                        "messages": [{
                            "from": "14155552671",
                            "id": "wamid.abc123",
                            "timestamp": "1700000000",
                            "type": "text",
                            "text": { "body": "Hello!" }
                        }]
                    }
                }]
            }]
        })
    }

    #[test]
    fn parse_text_webhook() {
        let payload = sample_webhook_payload();
        let messages = parse_webhook_payload(&payload);

        assert_eq!(messages.len(), 1);
        let msg = &messages[0];
        assert_eq!(msg.message_id, "wamid.abc123");
        assert_eq!(msg.from, "14155552671");
        assert_eq!(msg.from_name, Some("Alice".into()));
        assert_eq!(msg.text, Some("Hello!".into()));
        assert!(msg.media.is_none());
    }

    #[test]
    fn parse_image_webhook() {
        let payload = json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "BIZ_ID",
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp",
                        "contacts": [{ "profile": { "name": "Bob" }, "wa_id": "14155559999" }],
                        "messages": [{
                            "from": "14155559999",
                            "id": "wamid.img456",
                            "timestamp": "1700000001",
                            "type": "image",
                            "image": {
                                "id": "media_id_123",
                                "mime_type": "image/jpeg",
                                "caption": "Check this out"
                            }
                        }]
                    }
                }]
            }]
        });

        let messages = parse_webhook_payload(&payload);
        assert_eq!(messages.len(), 1);
        let msg = &messages[0];
        assert_eq!(msg.text, Some("Check this out".into()));
        let media = msg.media.as_ref().unwrap();
        assert_eq!(media.id, "media_id_123");
        assert_eq!(media.mime_type, "image/jpeg");
    }

    #[test]
    fn parse_document_webhook() {
        let payload = json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "BIZ_ID",
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp",
                        "contacts": [],
                        "messages": [{
                            "from": "14155550000",
                            "id": "wamid.doc789",
                            "timestamp": "1700000002",
                            "type": "document",
                            "document": {
                                "id": "doc_media_id",
                                "mime_type": "application/pdf",
                                "filename": "report.pdf"
                            }
                        }]
                    }
                }]
            }]
        });

        let messages = parse_webhook_payload(&payload);
        assert_eq!(messages.len(), 1);
        let media = messages[0].media.as_ref().unwrap();
        assert_eq!(media.filename, Some("report.pdf".into()));
        assert_eq!(media.mime_type, "application/pdf");
    }

    #[test]
    fn parse_batched_messages() {
        let payload = json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "BIZ_ID",
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp",
                        "contacts": [],
                        "messages": [
                            {
                                "from": "14155551111",
                                "id": "wamid.msg1",
                                "timestamp": "1700000003",
                                "type": "text",
                                "text": { "body": "First" }
                            },
                            {
                                "from": "14155552222",
                                "id": "wamid.msg2",
                                "timestamp": "1700000004",
                                "type": "text",
                                "text": { "body": "Second" }
                            }
                        ]
                    }
                }]
            }]
        });

        let messages = parse_webhook_payload(&payload);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text, Some("First".into()));
        assert_eq!(messages[1].text, Some("Second".into()));
    }

    #[test]
    fn parse_empty_payload() {
        let payload = json!({});
        assert!(parse_webhook_payload(&payload).is_empty());

        let no_messages = json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "BIZ_ID",
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp"
                    }
                }]
            }]
        });
        assert!(parse_webhook_payload(&no_messages).is_empty());
    }

    #[test]
    fn verification_request_valid() {
        let result = parse_verification_request(
            "subscribe",
            "my_secret_token",
            "my_secret_token",
            "challenge_string_123",
        );
        assert_eq!(result, Some("challenge_string_123".into()));
    }

    #[test]
    fn verification_request_wrong_token() {
        let result = parse_verification_request(
            "subscribe",
            "wrong_token",
            "my_secret_token",
            "challenge_string_123",
        );
        assert!(result.is_none());
    }

    #[test]
    fn verification_request_wrong_mode() {
        let result = parse_verification_request(
            "unsubscribe",
            "my_secret_token",
            "my_secret_token",
            "challenge_string_123",
        );
        assert!(result.is_none());
    }

    #[test]
    fn number_allowed_filtering() {
        let allowed = Some(vec!["14155551111".to_string(), "14155552222".to_string()]);
        assert!(is_number_allowed("14155551111", &allowed));
        assert!(!is_number_allowed("14155559999", &allowed));

        let no_filter: Option<Vec<String>> = None;
        assert!(is_number_allowed("14155559999", &no_filter));
    }

    #[test]
    fn hex_decode_valid() {
        assert_eq!(hex_decode("48656c6c6f"), Some(b"Hello".to_vec()));
        assert_eq!(hex_decode(""), Some(vec![]));
    }

    #[test]
    fn hex_decode_invalid() {
        assert!(hex_decode("gg").is_none());
        assert!(hex_decode("abc").is_none()); // odd length
    }

    #[test]
    fn signature_verification_without_feature() {
        // Without the webhook-crypto feature, this should always return false.
        #[cfg(not(feature = "webhook-crypto"))]
        assert!(!verify_webhook_signature(b"payload", "sha256=abc", "secret"));
    }

    // -----------------------------------------------------------------------
    // Outbound formatting tests
    // -----------------------------------------------------------------------

    #[test]
    fn format_text_message() {
        let chunk = OutboundChunk::TextBlock("Hello!".into());
        let meta = json!({"from": "14155552671", "phone_number_id": "PHONE_ID"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["messaging_product"], "whatsapp");
        assert_eq!(payload["to"], "14155552671");
        assert_eq!(payload["type"], "text");
        assert_eq!(payload["text"]["body"], "Hello!");
    }

    #[test]
    fn format_media_message() {
        let chunk = OutboundChunk::Media {
            mime_type: "image/jpeg".into(),
            data: vec![1, 2, 3],
            caption: Some("Photo".into()),
        };
        let meta = json!({"from": "14155552671"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["type"], "image");
    }

    #[test]
    fn format_buttons_message() {
        use mcclawd_channels::types::InlineButton;
        let chunk = OutboundChunk::Buttons {
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
        };
        let meta = json!({"from": "14155552671"});
        let payload = format_outbound(&chunk, &meta).unwrap();
        assert_eq!(payload["type"], "interactive");
        let btns = &payload["interactive"]["action"]["buttons"];
        assert_eq!(btns[0]["reply"]["title"], "Yes");
    }

    #[test]
    fn format_status_returns_none() {
        let chunk = OutboundChunk::StatusIndicator(ChannelStatus::Typing);
        let meta = json!({"from": "14155552671"});
        assert!(format_outbound(&chunk, &meta).is_none());
    }

    #[test]
    fn format_done_returns_none() {
        let chunk = OutboundChunk::Done;
        let meta = json!({"from": "14155552671"});
        assert!(format_outbound(&chunk, &meta).is_none());
    }

    #[test]
    fn format_missing_from_returns_none() {
        let chunk = OutboundChunk::TextBlock("Hello".into());
        let meta = json!({});
        assert!(format_outbound(&chunk, &meta).is_none());
    }
}
