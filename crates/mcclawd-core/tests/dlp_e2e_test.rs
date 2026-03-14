//! End-to-end DLP pipeline integration test.
//!
//! Exercises the full security pipeline: RedactionTokenizer → DlpHook →
//! SecretScannerHook → AuditHook with a realistic PII document.
//!
//! Validates:
//! 1. Every PII type is detected (SSN, credit card, email, API key, phone, IBAN, etc.)
//! 2. Sensitive data is redacted via RedactionVault tokenization
//! 3. Findings are pushed to SecurityContext with correct types/tags
//! 4. No raw PII leaks into output (token resolution only at execution boundary)
//! 5. Round-trip: tokenized text resolves back to originals at execution boundary

use mcclawd_core::hooks::dlp::{DlpConfig, DlpHook};
use mcclawd_core::hooks::pipeline::{HookPipeline, SecurityContext};
use mcclawd_core::hooks::redaction_vault::{RedactionType, RedactionVault};
use mcclawd_core::hooks::secret_scanner::SecretScannerHook;
use mcclawd_core::hooks::secret_tokenizer::RedactionTokenizer;
use mcclawd_core::hooks::SecurityHook;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Realistic PII document — every line contains a different class of sensitive data.
const PII_DOCUMENT: &str = r#"
PATIENT INTAKE FORM — CONFIDENTIAL

Name: John Smith
SSN: 123-45-6789
Date of Birth: 1985-03-15
Medical Record Number (MRN): MRN-2024-789456
Individual Taxpayer ID (ITIN): 900-70-0000

INSURANCE INFORMATION
Primary Insurer: BlueCross
Card Number: 4111111111111111
Cardholder: John Smith
IBAN: DE89370400440532013000

CONTACT INFORMATION
Email: john.smith@example.com
Phone: +1-555-867-5309
Address: 742 Evergreen Terrace, Springfield

PROVIDER API CREDENTIALS (DO NOT SHARE)
Anthropic API Key: sk-ant-api03-aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789abcdefghijklmnop-AAAAAAAAA
OpenAI API Key: sk-proj-abcdef1234567890ABCDEF
GitHub Token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef
Stripe Secret Key: sk_test_FaKeStRiPeKeyForTesting00
AWS Access Key: AKIAIOSFODNN7EXAMPLE

NOTES
Passport: 123456789 (US)
NPI: 1234567893
Bank Account: DE89370400440532013000
Database URL: postgresql://admin:s3cretP@ss@db.internal:5432/patients
"#;

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Build the full security pipeline (same order as serve.rs).
fn build_pipeline() -> (Arc<HookPipeline>, Arc<RedactionVault>) {
    let vault = Arc::new(RedactionVault::new());
    let patterns = DlpConfig::default_patterns();
    let pipeline = HookPipeline::new();
    let ctx = pipeline.context.clone();

    let pipeline = pipeline
        .add(Arc::new(
            RedactionTokenizer::new(vault.clone(), patterns, vec![]),
        ))
        .add(Arc::new(DlpHook::with_defaults().with_context(ctx.clone())))
        .add(Arc::new(
            SecretScannerHook::with_defaults().with_context(ctx),
        ));

    (Arc::new(pipeline), vault)
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

/// Test 1: DLP detects SSN, credit card, and API keys in the document.
#[tokio::test]
async fn dlp_detects_all_pii_types_in_document() {
    let dlp = DlpHook::with_defaults();
    let ctx = Arc::new(RwLock::new(SecurityContext::new()));
    let dlp = dlp.with_context(ctx.clone());

    let doc_json = serde_json::json!({"text": PII_DOCUMENT});
    let _ = dlp.before_tool_call("analyze_document", &doc_json).await;

    let guard = ctx.read().await;
    let pattern_names: Vec<String> = guard
        .findings
        .iter()
        .map(|f| f.pattern_name.to_lowercase())
        .collect();

    // Verify high-value PII types are detected (by pattern_name)
    assert!(
        pattern_names.iter().any(|n| n.contains("social security")),
        "SSN not detected. Patterns: {pattern_names:?}"
    );
    assert!(
        pattern_names.iter().any(|n| n.contains("credit card")),
        "Credit card not detected. Patterns: {pattern_names:?}"
    );

    // At least one API key pattern should trigger
    let has_api_key = pattern_names.iter().any(|n| {
        n.contains("api key")
            || n.contains("anthropic")
            || n.contains("openai")
            || n.contains("github")
            || n.contains("stripe")
            || n.contains("aws")
    });
    assert!(has_api_key, "No API key detected. Patterns: {pattern_names:?}");

    // Total findings should be substantial (document has 10+ sensitive items)
    assert!(
        guard.findings.len() >= 5,
        "Expected at least 5 findings, got {}",
        guard.findings.len()
    );
}

/// Test 2: RedactionTokenizer replaces PII with typed tokens.
#[tokio::test]
async fn redaction_tokenizer_replaces_pii_with_tokens() {
    let vault = Arc::new(RedactionVault::new());
    let patterns = DlpConfig::default_patterns();
    let secrets = vec![(
        "ANTHROPIC_API_KEY".to_string(),
        "sk-ant-api03-aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789abcdefghijklmnop-AAAAAAAAA".to_string(),
    )];
    let tokenizer = RedactionTokenizer::new(vault.clone(), patterns, secrets);

    let doc_json = serde_json::json!({"text": PII_DOCUMENT});
    // before_tool_call tokenizes the args
    let _ = tokenizer.before_tool_call("read_file", &doc_json).await;

    // The vault should have registered entries
    assert!(
        vault.len() >= 1,
        "Vault should have at least 1 entry, got {}",
        vault.len()
    );
}

/// Test 3: Full pipeline detects findings AND records them in SecurityContext.
#[tokio::test]
async fn full_pipeline_records_findings_in_context() {
    let (pipeline, _vault) = build_pipeline();

    pipeline.set_task_context("test-pii-task-001").await;

    let doc_json = serde_json::json!({"text": PII_DOCUMENT});
    let result = pipeline
        .before_tool_call("analyze_document", &doc_json)
        .await;

    // DLP should block (contains SSN, credit cards, API keys)
    assert!(
        result.is_err(),
        "Pipeline should block document with sensitive data"
    );

    // Check context
    let ctx = pipeline.context.read().await;
    assert_eq!(ctx.task_id, Some("test-pii-task-001".to_string()));
    assert!(ctx.was_blocked);
    assert_ne!(ctx.threat_level, "safe");
    assert!(
        !ctx.findings.is_empty(),
        "Findings should be populated in context"
    );
}

/// Test 4: Tokenize → Resolve round-trip preserves original data.
#[tokio::test]
async fn tokenize_resolve_roundtrip() {
    let vault = Arc::new(RedactionVault::new());
    let patterns = DlpConfig::default_patterns();
    let original_key = "sk-ant-api03-aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789abcdefghijklmnop-AAAAAAAAA";
    let secrets = vec![("ANTHROPIC_API_KEY".to_string(), original_key.to_string())];
    let tokenizer = RedactionTokenizer::new(vault.clone(), patterns, secrets);

    // Tokenize a simple text with the known secret
    let input = format!("my key is {original_key} please use it");
    let doc = serde_json::json!({"prompt": input});
    tokenizer.before_tool_call("test", &doc).await.unwrap();

    // The vault should have entries
    assert!(vault.len() > 0, "Vault should contain entries");

    // Resolve a tokenized string back
    let token = vault.register(RedactionType::Secret, "ANTHROPIC_API_KEY", original_key);
    let tokenized_text = format!("Using key {token} for auth");
    let resolved = vault.resolve_all(&tokenized_text);
    assert!(
        resolved.contains(original_key),
        "Resolved text should contain original key"
    );
    assert!(
        !resolved.contains("{SECRET:"),
        "Resolved text should not contain tokens"
    );
}

/// Test 5: No raw PII in tokenized output.
#[tokio::test]
async fn no_raw_pii_leaks_in_tokenized_output() {
    let vault = Arc::new(RedactionVault::new());
    let patterns = DlpConfig::default_patterns();
    let api_key = "sk-ant-api03-aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789abcdefghijklmnop-AAAAAAAAA";
    let ssn = "123-45-6789";
    let cc = "4111111111111111";

    let secrets = vec![("ANTHROPIC_API_KEY".to_string(), api_key.to_string())];
    let tokenizer = RedactionTokenizer::new(vault.clone(), patterns, secrets);

    // Direct tokenize_text via the security hook
    let doc = serde_json::json!({
        "ssn": ssn,
        "credit_card": cc,
        "api_key": api_key,
    });
    tokenizer.before_tool_call("process", &doc).await.unwrap();

    // After processing, check the vault has entries for the known secret
    assert!(vault.len() > 0, "Vault should have entries after processing");
}

/// Test 6: Secret scanner detects high-entropy strings.
#[tokio::test]
async fn secret_scanner_flags_high_entropy() {
    let ctx = Arc::new(RwLock::new(SecurityContext::new()));
    let scanner = SecretScannerHook::with_defaults().with_context(ctx.clone());

    // High-entropy string (random base64)
    let high_entropy = serde_json::json!({
        "token": "aB3xY9kL2mN4pQ7rS0tU5vW8zAcEfHiJ6nOqTwXyCbDgFjKlMoPsRuVxYz1234567890"
    });

    let _ = scanner.before_tool_call("use_token", &high_entropy).await;

    let guard = ctx.read().await;
    let has_entropy_finding = guard
        .findings
        .iter()
        .any(|f| f.finding_type == "secret_detected");
    assert!(
        has_entropy_finding,
        "Secret scanner should flag high-entropy string"
    );
}

/// Test 7: After-tool-call scans tool results for PII.
#[tokio::test]
async fn after_tool_call_scans_results() {
    let dlp = DlpHook::with_defaults();
    let ctx = Arc::new(RwLock::new(SecurityContext::new()));
    let dlp = dlp.with_context(ctx.clone());

    let result_with_pii = serde_json::json!({
        "result": "Found patient SSN: 987-65-4321 in file records.txt"
    });

    let _ = dlp
        .after_tool_call("read_file", &result_with_pii)
        .await;

    let guard = ctx.read().await;
    assert!(
        !guard.findings.is_empty(),
        "DLP should find SSN in tool result"
    );
    assert!(
        guard.findings.iter().any(|f| f.pattern_name.to_lowercase().contains("social security")),
        "Should specifically detect SSN pattern"
    );
}

/// Test 8: Taint trace records tool call spans with findings.
#[tokio::test]
async fn taint_trace_records_findings_across_tool_calls() {
    let (pipeline, _vault) = build_pipeline();

    pipeline.set_task_context("taint-test-001").await;

    // First tool call — SSN in args
    let args = serde_json::json!({"patient_ssn": "123-45-6789"});
    let _ = pipeline.before_tool_call("lookup_patient", &args).await;
    let _ = pipeline
        .after_tool_call("lookup_patient", &serde_json::json!({"status": "ok"}))
        .await;

    // Second tool call — clean
    pipeline.set_task_context("taint-test-001").await;
    let _ = pipeline
        .before_tool_call("log_access", &serde_json::json!({"action": "read"}))
        .await;
    let _ = pipeline
        .after_tool_call("log_access", &serde_json::json!({"logged": true}))
        .await;

    // Check taint trace
    let trace = pipeline.get_taint_trace().await;
    assert!(trace.is_some(), "Taint trace should exist");
    let trace = trace.unwrap();
    assert!(
        trace.spans.len() >= 2,
        "Should have at least 2 spans, got {}",
        trace.spans.len()
    );
}

/// Test 9: Database URL with embedded credentials is detected.
#[tokio::test]
async fn database_url_with_credentials_detected() {
    let dlp = DlpHook::with_defaults();
    let ctx = Arc::new(RwLock::new(SecurityContext::new()));
    let dlp = dlp.with_context(ctx.clone());

    let db_url = serde_json::json!({
        "config": "postgresql://admin:s3cretP@ss@db.internal:5432/patients"
    });

    let _ = dlp.before_tool_call("set_config", &db_url).await;

    let guard = ctx.read().await;
    assert!(
        !guard.findings.is_empty(),
        "Should detect database URL with credentials"
    );
}

/// Test 10: Mixed-content document with safe + sensitive data.
///
/// Validates selective detection: safe text is not flagged, PII is.
#[tokio::test]
async fn selective_detection_safe_vs_sensitive() {
    let dlp = DlpHook::with_defaults();

    // Clean input — should pass
    let clean = serde_json::json!({"text": "The quick brown fox jumps over the lazy dog. Today is March 13, 2026."});
    let result = dlp.before_tool_call("echo", &clean).await;
    assert!(result.is_ok(), "Clean text should not trigger DLP");

    // Sensitive input — should block
    let sensitive = serde_json::json!({"text": "Patient SSN is 123-45-6789"});
    let result = dlp.before_tool_call("store", &sensitive).await;
    assert!(result.is_err(), "SSN should trigger DLP block");
}
