//! RFC 822 email parser and outbound email builder.
//!
//! Provides basic email parsing without depending on external crates like
//! `mailparse`. Handles common text/plain emails, extracts headers, and
//! builds simple outbound RFC 822 messages.
//!
//! For production MIME parsing (multipart, encoded attachments), enable the
//! `live` feature which brings in `lettre` and `async-imap`.

use chrono::{DateTime, Utc};

use crate::normalize::{EmailAttachment, EmailMessage};

// ---------------------------------------------------------------------------
// RFC 822 parsing (always available, no external deps)
// ---------------------------------------------------------------------------

/// Parse a raw RFC 822 email message into an [`EmailMessage`].
///
/// Handles:
/// - Standard headers: From, Subject, Message-ID, In-Reply-To, Date, Content-Type
/// - Simple text/plain body (single-part)
/// - Basic multipart/mixed with text/plain and attachment parts
///
/// Returns `None` if the message cannot be parsed (missing required headers).
pub fn parse_raw_email(raw: &str) -> Option<EmailMessage> {
    let (header_section, body_section) = split_headers_body(raw);
    let headers = parse_headers(&header_section);

    let message_id = headers
        .get("message-id")
        .cloned()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let from_raw = headers.get("from")?.clone();
    let (from_name, from_address) = parse_from_header(&from_raw);
    let subject = headers.get("subject").cloned();
    let in_reply_to = headers.get("in-reply-to").cloned();
    let date = headers
        .get("date")
        .and_then(|d| parse_rfc2822_date(d))
        .unwrap_or_else(Utc::now);

    let content_type = headers
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| "text/plain".to_string());

    let (body_text, body_html, attachments) = if content_type.contains("multipart/") {
        parse_multipart_body(&body_section, &content_type)
    } else if content_type.contains("text/html") {
        (None, Some(body_section.to_string()), vec![])
    } else {
        (Some(body_section.to_string()), None, vec![])
    };

    Some(EmailMessage {
        message_id,
        from_address,
        from_name,
        subject,
        body_text,
        body_html,
        in_reply_to,
        attachments,
        date,
    })
}

/// Split raw email into header section and body section.
fn split_headers_body(raw: &str) -> (String, String) {
    // Headers and body are separated by a blank line (\r\n\r\n or \n\n).
    if let Some(pos) = raw.find("\r\n\r\n") {
        (raw[..pos].to_string(), raw[pos + 4..].to_string())
    } else if let Some(pos) = raw.find("\n\n") {
        (raw[..pos].to_string(), raw[pos + 2..].to_string())
    } else {
        // No body.
        (raw.to_string(), String::new())
    }
}

/// Parse headers into a case-insensitive map.
/// Handles header folding (continuation lines starting with whitespace).
fn parse_headers(header_section: &str) -> std::collections::HashMap<String, String> {
    let mut headers = std::collections::HashMap::new();
    let mut current_key: Option<String> = None;
    let mut current_value = String::new();

    for raw_line in header_section.lines() {
        // Strip trailing \r for CRLF line endings.
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation of previous header.
            if current_key.is_some() {
                current_value.push(' ');
                current_value.push_str(line.trim());
            }
        } else if let Some(colon_pos) = line.find(':') {
            // Save previous header.
            if let Some(key) = current_key.take() {
                headers.insert(key, current_value.trim().to_string());
            }
            current_key = Some(line[..colon_pos].trim().to_lowercase());
            current_value = line[colon_pos + 1..].trim().to_string();
        }
    }

    // Save last header.
    if let Some(key) = current_key {
        headers.insert(key, current_value.trim().to_string());
    }

    headers
}

/// Parse a From header value like `"Alice <alice@example.com>"` or `"alice@example.com"`.
fn parse_from_header(from: &str) -> (Option<String>, String) {
    let from = from.trim();
    if let Some(angle_start) = from.find('<') {
        if let Some(angle_end) = from.find('>') {
            let address = from[angle_start + 1..angle_end].trim().to_string();
            let name = from[..angle_start].trim().trim_matches('"').to_string();
            let name = if name.is_empty() { None } else { Some(name) };
            return (name, address);
        }
    }
    (None, from.to_string())
}

/// Try to parse an RFC 2822 date string.
fn parse_rfc2822_date(date_str: &str) -> Option<DateTime<Utc>> {
    // Try standard RFC 2822 parsing.
    DateTime::parse_from_rfc2822(date_str)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Parse a multipart body, extracting text/plain, text/html, and attachments.
fn parse_multipart_body(
    body: &str,
    content_type: &str,
) -> (Option<String>, Option<String>, Vec<EmailAttachment>) {
    let boundary = extract_boundary(content_type);
    let boundary = match boundary {
        Some(b) => b,
        None => return (Some(body.to_string()), None, vec![]),
    };

    let delimiter = format!("--{}", boundary);
    let end_delimiter = format!("--{}--", boundary);

    let mut text_part = None;
    let mut html_part = None;
    let mut attachments = Vec::new();

    let parts: Vec<&str> = body.split(&delimiter).collect();

    for part in parts.iter().skip(1) {
        // skip preamble
        let part = part.trim();
        if part.starts_with("--") || part == end_delimiter || part.is_empty() {
            continue;
        }

        let (part_headers_str, part_body) = if let Some(pos) = part.find("\r\n\r\n") {
            (part[..pos].to_string(), part[pos + 4..].to_string())
        } else if let Some(pos) = part.find("\n\n") {
            (part[..pos].to_string(), part[pos + 2..].to_string())
        } else {
            continue;
        };

        let part_headers = parse_headers(&part_headers_str);
        let part_ct = part_headers
            .get("content-type")
            .cloned()
            .unwrap_or_else(|| "text/plain".to_string());
        let part_disposition = part_headers
            .get("content-disposition")
            .cloned()
            .unwrap_or_default();

        // Trim trailing boundary markers from the body.
        let part_body = part_body
            .trim_end_matches(&end_delimiter)
            .trim_end_matches(&delimiter)
            .trim()
            .to_string();

        if part_disposition.contains("attachment") {
            let filename = extract_param(&part_disposition, "filename")
                .unwrap_or_else(|| "attachment".to_string());
            attachments.push(EmailAttachment {
                filename,
                content_type: part_ct.split(';').next().unwrap_or("application/octet-stream").trim().to_string(),
                data: part_body.into_bytes(),
            });
        } else if part_ct.starts_with("text/plain") && text_part.is_none() {
            text_part = Some(part_body);
        } else if part_ct.starts_with("text/html") && html_part.is_none() {
            html_part = Some(part_body);
        }
    }

    (text_part, html_part, attachments)
}

/// Extract the boundary parameter from a Content-Type header.
fn extract_boundary(content_type: &str) -> Option<String> {
    extract_param(content_type, "boundary")
}

/// Extract a named parameter from a header value like `key="value"` or `key=value`.
fn extract_param(header: &str, param: &str) -> Option<String> {
    let search = format!("{}=", param);
    let pos = header.find(&search)?;
    let rest = &header[pos + search.len()..];
    let rest = rest.trim();
    if rest.starts_with('"') {
        // Quoted value.
        let end = rest[1..].find('"')?;
        Some(rest[1..1 + end].to_string())
    } else {
        // Unquoted value (ends at ; or end of string).
        let end = rest.find(';').unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

// ---------------------------------------------------------------------------
// Outbound email building
// ---------------------------------------------------------------------------

/// Build a simple RFC 822 text/plain email message for outbound delivery.
///
/// Returns a string suitable for sending via SMTP.
pub fn build_outbound_email(
    from: &str,
    to: &str,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
) -> String {
    let message_id = format!("<{}.mcclawd@{}>", uuid::Uuid::new_v4(), extract_domain(from));
    let date = Utc::now().format("%a, %d %b %Y %H:%M:%S +0000").to_string();

    let mut headers = format!(
        "From: {}\r\nTo: {}\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n",
        from, to, subject, date, message_id
    );

    if let Some(reply_to) = in_reply_to {
        headers.push_str(&format!("In-Reply-To: {}\r\nReferences: {}\r\n", reply_to, reply_to));
    }

    format!("{}\r\n{}", headers, body)
}

/// Extract domain from an email address or "From" header value.
fn extract_domain(from: &str) -> String {
    // Handle "Name <addr>" format.
    let addr = if let Some(start) = from.find('<') {
        if let Some(end) = from.find('>') {
            &from[start + 1..end]
        } else {
            from
        }
    } else {
        from
    };

    addr.split('@')
        .nth(1)
        .unwrap_or("localhost")
        .to_string()
}

// ---------------------------------------------------------------------------
// Outbound chunk formatting
// ---------------------------------------------------------------------------

use mcclawd_channels::types::OutboundChunk;

/// Format an [`OutboundChunk`] as an outbound email RFC 822 string.
///
/// The `platform_meta` should contain `from_address` (sender) and
/// `reply_to` (original sender to reply to), `subject`, `message_id`.
/// Returns `None` for chunks that have no email representation.
pub fn format_outbound(
    chunk: &OutboundChunk,
    platform_meta: &serde_json::Value,
) -> Option<String> {
    let reply_to_addr = platform_meta.get("from_address")?.as_str()?;
    let our_address = platform_meta
        .get("our_address")
        .and_then(|v| v.as_str())
        .unwrap_or("agent@mcclawd.local");
    let subject = platform_meta
        .get("subject")
        .and_then(|v| v.as_str())
        .map(|s| format!("Re: {}", s))
        .unwrap_or_else(|| "Re: (no subject)".to_string());
    let in_reply_to = platform_meta
        .get("message_id")
        .and_then(|v| v.as_str());

    match chunk {
        OutboundChunk::TextBlock(text) => Some(build_outbound_email(
            our_address,
            reply_to_addr,
            &subject,
            text,
            in_reply_to,
        )),
        OutboundChunk::Error(msg) => Some(build_outbound_email(
            our_address,
            reply_to_addr,
            &subject,
            &format!("Error: {}", msg),
            in_reply_to,
        )),
        OutboundChunk::ToolEnd { name, summary } => {
            if let Some(s) = summary {
                Some(build_outbound_email(
                    our_address,
                    reply_to_addr,
                    &subject,
                    &format!("{}: {}", name, s),
                    in_reply_to,
                ))
            } else {
                None
            }
        }
        // TextDelta, Media, Buttons, StatusIndicator, ToolStart, Done
        // are not meaningful for email.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_text_email() {
        let raw = "From: alice@example.com\r\n\
                    To: bob@example.com\r\n\
                    Subject: Hello\r\n\
                    Message-ID: <msg001@example.com>\r\n\
                    Date: Mon, 15 Jan 2024 10:30:00 +0000\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    Hello, Bob!\r\n\
                    How are you?";

        let msg = parse_raw_email(raw).unwrap();
        assert_eq!(msg.from_address, "alice@example.com");
        assert!(msg.from_name.is_none());
        assert_eq!(msg.subject, Some("Hello".into()));
        assert_eq!(msg.message_id, "<msg001@example.com>");
        assert_eq!(msg.body_text, Some("Hello, Bob!\r\nHow are you?".into()));
        assert!(msg.body_html.is_none());
        assert!(msg.in_reply_to.is_none());
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn parse_with_display_name() {
        let raw = "From: \"Alice Smith\" <alice@example.com>\r\n\
                    To: bob@example.com\r\n\
                    Subject: Test\r\n\
                    \r\n\
                    Body text.";

        let msg = parse_raw_email(raw).unwrap();
        assert_eq!(msg.from_name, Some("Alice Smith".into()));
        assert_eq!(msg.from_address, "alice@example.com");
    }

    #[test]
    fn parse_reply_with_in_reply_to() {
        let raw = "From: bob@example.com\r\n\
                    To: alice@example.com\r\n\
                    Subject: Re: Hello\r\n\
                    In-Reply-To: <msg001@example.com>\r\n\
                    Message-ID: <msg002@example.com>\r\n\
                    \r\n\
                    I'm good, thanks!";

        let msg = parse_raw_email(raw).unwrap();
        assert_eq!(msg.in_reply_to, Some("<msg001@example.com>".into()));
        assert_eq!(msg.message_id, "<msg002@example.com>");
    }

    #[test]
    fn parse_multipart_with_attachment() {
        let raw = "From: alice@example.com\r\n\
                    To: bob@example.com\r\n\
                    Subject: With attachment\r\n\
                    Content-Type: multipart/mixed; boundary=\"BOUNDARY123\"\r\n\
                    \r\n\
                    --BOUNDARY123\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    See attached file.\r\n\
                    --BOUNDARY123\r\n\
                    Content-Type: application/pdf\r\n\
                    Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
                    \r\n\
                    PDF_CONTENT_HERE\r\n\
                    --BOUNDARY123--";

        let msg = parse_raw_email(raw).unwrap();
        assert_eq!(msg.body_text, Some("See attached file.".into()));
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].filename, "report.pdf");
        assert_eq!(msg.attachments[0].content_type, "application/pdf");
    }

    #[test]
    fn parse_html_only_email() {
        let raw = "From: alice@example.com\r\n\
                    To: bob@example.com\r\n\
                    Subject: HTML\r\n\
                    Content-Type: text/html\r\n\
                    \r\n\
                    <html><body>Hello</body></html>";

        let msg = parse_raw_email(raw).unwrap();
        assert!(msg.body_text.is_none());
        assert!(msg.body_html.is_some());
    }

    #[test]
    fn parse_missing_from_returns_none() {
        let raw = "To: bob@example.com\r\n\
                    Subject: No From\r\n\
                    \r\n\
                    Body.";

        assert!(parse_raw_email(raw).is_none());
    }

    #[test]
    fn parse_with_folded_headers() {
        // RFC 822 header folding: continuation line starts with whitespace.
        // We build the raw string with explicit \r\n to avoid Rust's line continuation eating spaces.
        let raw = "From: alice@example.com\r\nSubject: This is a very long\r\n subject line that wraps\r\nTo: bob@example.com\r\n\r\nBody.";

        let msg = parse_raw_email(raw).unwrap();
        assert_eq!(
            msg.subject,
            Some("This is a very long subject line that wraps".into())
        );
    }

    #[test]
    fn parse_unix_line_endings() {
        let raw = "From: alice@example.com\n\
                    To: bob@example.com\n\
                    Subject: Unix\n\
                    \n\
                    Body with unix endings.";

        let msg = parse_raw_email(raw).unwrap();
        assert_eq!(msg.body_text, Some("Body with unix endings.".into()));
    }

    // -----------------------------------------------------------------------
    // Outbound building tests
    // -----------------------------------------------------------------------

    #[test]
    fn build_simple_outbound() {
        let email = build_outbound_email(
            "agent@mcclawd.io",
            "user@example.com",
            "Re: Hello",
            "Here is my response.",
            None,
        );

        assert!(email.contains("From: agent@mcclawd.io"));
        assert!(email.contains("To: user@example.com"));
        assert!(email.contains("Subject: Re: Hello"));
        assert!(email.contains("Here is my response."));
        assert!(email.contains("Message-ID:"));
        assert!(email.contains("MIME-Version: 1.0"));
    }

    #[test]
    fn build_outbound_with_reply() {
        let email = build_outbound_email(
            "agent@mcclawd.io",
            "user@example.com",
            "Re: Hello",
            "Response",
            Some("<original@example.com>"),
        );

        assert!(email.contains("In-Reply-To: <original@example.com>"));
        assert!(email.contains("References: <original@example.com>"));
    }

    #[test]
    fn roundtrip_parse_build() {
        let built = build_outbound_email(
            "agent@mcclawd.io",
            "user@example.com",
            "Test Subject",
            "Test body content.",
            None,
        );

        let parsed = parse_raw_email(&built).unwrap();
        assert_eq!(parsed.from_address, "agent@mcclawd.io");
        assert_eq!(parsed.subject, Some("Test Subject".into()));
        assert_eq!(parsed.body_text, Some("Test body content.".into()));
    }

    // -----------------------------------------------------------------------
    // Outbound chunk formatting tests
    // -----------------------------------------------------------------------

    #[test]
    fn format_text_block_outbound() {
        let chunk = OutboundChunk::TextBlock("Hello!".into());
        let meta = serde_json::json!({
            "from_address": "user@example.com",
            "our_address": "agent@mcclawd.io",
            "subject": "Hello",
            "message_id": "<orig@example.com>",
        });
        let email = format_outbound(&chunk, &meta).unwrap();
        assert!(email.contains("From: agent@mcclawd.io"));
        assert!(email.contains("To: user@example.com"));
        assert!(email.contains("Subject: Re: Hello"));
        assert!(email.contains("In-Reply-To: <orig@example.com>"));
        assert!(email.contains("Hello!"));
    }

    #[test]
    fn format_done_returns_none() {
        let chunk = OutboundChunk::Done;
        let meta = serde_json::json!({"from_address": "user@example.com"});
        assert!(format_outbound(&chunk, &meta).is_none());
    }

    #[test]
    fn format_missing_from_returns_none() {
        let chunk = OutboundChunk::TextBlock("Hello".into());
        let meta = serde_json::json!({});
        assert!(format_outbound(&chunk, &meta).is_none());
    }

    // -----------------------------------------------------------------------
    // Helper function tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_from_header_with_name() {
        let (name, addr) = parse_from_header("\"Alice\" <alice@example.com>");
        assert_eq!(name, Some("Alice".into()));
        assert_eq!(addr, "alice@example.com");
    }

    #[test]
    fn parse_from_header_bare_address() {
        let (name, addr) = parse_from_header("alice@example.com");
        assert!(name.is_none());
        assert_eq!(addr, "alice@example.com");
    }

    #[test]
    fn extract_boundary_from_content_type() {
        let ct = "multipart/mixed; boundary=\"ABC123\"";
        assert_eq!(extract_boundary(ct), Some("ABC123".into()));

        let ct2 = "multipart/mixed; boundary=ABC123";
        assert_eq!(extract_boundary(ct2), Some("ABC123".into()));
    }

    #[test]
    fn extract_domain_from_address() {
        assert_eq!(extract_domain("alice@example.com"), "example.com");
        assert_eq!(
            extract_domain("\"Alice\" <alice@example.com>"),
            "example.com"
        );
        assert_eq!(extract_domain("noat"), "localhost");
    }
}
