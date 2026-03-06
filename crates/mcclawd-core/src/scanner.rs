//! Security scanner for installed skills.
//!
//! Runs `uvx snyk-agent-scan@latest --json --skills <path>` as a subprocess
//! and parses the JSON output into structured results.

use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

/// Overall scan result for a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub status: ScanStatus,
    pub issues: Vec<ScanIssue>,
}

/// Aggregate status derived from the worst issue severity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanStatus {
    Pass,
    Warning,
    Critical,
    NotScanned,
}

/// A single issue found by the scanner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanIssue {
    /// Issue code (e.g. E001, W007, TF001).
    pub code: String,
    /// Severity category: "issue", "warning", or "toxic_flow".
    pub severity: String,
    /// Human-readable description.
    pub description: String,
}

/// Raw JSON output shape from snyk-agent-scan.
#[derive(Debug, Deserialize)]
struct SnykScanOutput {
    #[serde(default)]
    issues: Vec<SnykIssue>,
}

#[derive(Debug, Deserialize)]
struct SnykIssue {
    #[serde(default)]
    code: String,
    #[serde(default)]
    severity: String,
    #[serde(default, alias = "message")]
    description: String,
}

/// Scan a skill directory using snyk-agent-scan.
///
/// If the scanner is not installed, returns `ScanStatus::NotScanned`.
pub async fn scan_skill(skill_path: &Path) -> anyhow::Result<ScanResult> {
    // Check if uvx is available
    let uvx_check = Command::new("which").arg("uvx").output().await;
    if uvx_check.is_err() || !uvx_check.unwrap().status.success() {
        tracing::debug!("uvx not found — falling back to basic static analysis");
        return basic_scan(skill_path).await;
    }

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        Command::new("uvx")
            .arg("snyk-agent-scan@latest")
            .arg("--json")
            .arg("--skills")
            .arg(skill_path)
            .output(),
    )
    .await;

    let output = match output {
        Ok(inner) => inner,
        Err(_) => {
            tracing::warn!("snyk-agent-scan timed out after 120s");
            return Ok(ScanResult {
                status: ScanStatus::NotScanned,
                issues: vec![],
            });
        }
    };

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("Failed to run snyk-agent-scan: {e}");
            return Ok(ScanResult {
                status: ScanStatus::NotScanned,
                issues: vec![],
            });
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("snyk-agent-scan exited with {}: {stderr}", output.status);
        // Non-zero exit may still produce JSON on stdout (e.g. issues found)
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(ScanResult {
            status: ScanStatus::NotScanned,
            issues: vec![],
        });
    }

    let parsed: SnykScanOutput = match serde_json::from_str(&stdout) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse snyk-agent-scan output: {e}");
            return Ok(ScanResult {
                status: ScanStatus::NotScanned,
                issues: vec![],
            });
        }
    };

    let issues: Vec<ScanIssue> = parsed
        .issues
        .into_iter()
        .map(|i| ScanIssue {
            code: i.code,
            severity: i.severity,
            description: i.description,
        })
        .collect();

    let status = derive_status(&issues);

    Ok(ScanResult { status, issues })
}

/// Basic static analysis fallback when uvx/snyk-agent-scan is not available.
/// Reads SKILL.md and checks for common security-sensitive patterns.
async fn basic_scan(skill_path: &Path) -> anyhow::Result<ScanResult> {
    let skill_md = skill_path.join("SKILL.md");
    let content = match tokio::fs::read_to_string(&skill_md).await {
        Ok(c) => c.to_lowercase(),
        Err(_) => {
            return Ok(ScanResult {
                status: ScanStatus::NotScanned,
                issues: vec![],
            });
        }
    };

    let mut issues = Vec::new();

    // Check for dangerous patterns in skill instructions
    let patterns: &[(&str, &str, &str, &str)] = &[
        ("rm -rf", "W001", "warning", "Skill references destructive file deletion (rm -rf)"),
        ("sudo ", "W002", "warning", "Skill references sudo/elevated privileges"),
        ("curl.*| sh", "E001", "issue", "Skill pipes remote content to shell (curl | sh)"),
        ("wget.*| sh", "E002", "issue", "Skill pipes remote content to shell (wget | sh)"),
        ("eval(", "W003", "warning", "Skill uses eval() which can execute arbitrary code"),
        ("exec(", "W004", "warning", "Skill uses exec() which can execute arbitrary commands"),
        (".env", "W005", "warning", "Skill references .env files (potential secret exposure)"),
        ("api_key", "W006", "warning", "Skill references API keys directly"),
        ("password", "W007", "warning", "Skill references passwords directly"),
        ("chmod 777", "E003", "issue", "Skill sets overly permissive file permissions (777)"),
        ("--no-verify", "W008", "warning", "Skill bypasses verification checks"),
    ];

    for &(pattern, code, severity, description) in patterns {
        if content.contains(pattern) {
            issues.push(ScanIssue {
                code: code.to_string(),
                severity: severity.to_string(),
                description: description.to_string(),
            });
        }
    }

    let status = derive_status(&issues);
    Ok(ScanResult { status, issues })
}

/// Derive the aggregate status from the list of issues.
fn derive_status(issues: &[ScanIssue]) -> ScanStatus {
    if issues.is_empty() {
        return ScanStatus::Pass;
    }

    let has_critical = issues.iter().any(|i| {
        i.severity == "issue"
            || i.severity == "toxic_flow"
            || i.code.starts_with('E')
            || i.code.starts_with("TF")
    });

    if has_critical {
        ScanStatus::Critical
    } else {
        ScanStatus::Warning
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_status_empty() {
        assert_eq!(derive_status(&[]), ScanStatus::Pass);
    }

    #[test]
    fn test_derive_status_warning() {
        let issues = vec![ScanIssue {
            code: "W001".to_string(),
            severity: "warning".to_string(),
            description: "Minor concern".to_string(),
        }];
        assert_eq!(derive_status(&issues), ScanStatus::Warning);
    }

    #[test]
    fn test_derive_status_critical_e_code() {
        let issues = vec![ScanIssue {
            code: "E001".to_string(),
            severity: "issue".to_string(),
            description: "Critical issue".to_string(),
        }];
        assert_eq!(derive_status(&issues), ScanStatus::Critical);
    }

    #[test]
    fn test_derive_status_critical_toxic_flow() {
        let issues = vec![ScanIssue {
            code: "TF001".to_string(),
            severity: "toxic_flow".to_string(),
            description: "Toxic flow detected".to_string(),
        }];
        assert_eq!(derive_status(&issues), ScanStatus::Critical);
    }

    #[tokio::test]
    async fn test_basic_scan_clean() {
        let dir = std::env::temp_dir().join("mcclawd_test_basic_scan_clean");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: safe-skill\n---\n# Safe Skill\n\n## Purpose\nDoes safe things.\n",
        )
        .unwrap();
        let result = basic_scan(&dir).await.unwrap();
        assert_eq!(result.status, ScanStatus::Pass);
        assert!(result.issues.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_basic_scan_warnings() {
        let dir = std::env::temp_dir().join("mcclawd_test_basic_scan_warn");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: risky-skill\n---\n# Risky\n\nRun sudo rm -rf /tmp/stuff\nUse eval( to process\n",
        )
        .unwrap();
        let result = basic_scan(&dir).await.unwrap();
        assert!(result.issues.len() >= 2);
        assert!(
            result.status == ScanStatus::Warning || result.status == ScanStatus::Critical
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_basic_scan_missing_file() {
        let dir = std::env::temp_dir().join("mcclawd_test_basic_scan_missing");
        let _ = std::fs::create_dir_all(&dir);
        let result = basic_scan(&dir).await.unwrap();
        assert_eq!(result.status, ScanStatus::NotScanned);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scan_result_serialization() {
        let result = ScanResult {
            status: ScanStatus::Warning,
            issues: vec![ScanIssue {
                code: "W007".to_string(),
                severity: "warning".to_string(),
                description: "Potential concern".to_string(),
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Warning"));
        assert!(json.contains("W007"));

        let parsed: ScanResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, ScanStatus::Warning);
        assert_eq!(parsed.issues.len(), 1);
    }
}
