"""McClawd Security Sidecar — unified PII/secrets/injection scanning."""
import hashlib
import os
import re
import time
from typing import Optional
from enum import Enum

from fastapi import FastAPI
from pydantic import BaseModel

app = FastAPI(title="McClawd Security Sidecar", version="0.1.0")


# ─── Enums ───────────────────────────────────────────────────────────
class ThreatLevel(str, Enum):
    safe = "safe"
    suspicious = "suspicious"
    dangerous = "dangerous"
    critical = "critical"


class SecurityAction(str, Enum):
    allowed = "allowed"
    warned = "warned"
    blocked = "blocked"
    redacted = "redacted"


# ─── Models ──────────────────────────────────────────────────────────
class ScanRequest(BaseModel):
    text: str
    context: str = "general"
    tool_name: Optional[str] = None
    trace_id: Optional[str] = None
    span_id: Optional[str] = None


class Detection(BaseModel):
    detector: str
    finding_type: str
    tag: str
    pattern_name: str
    confidence: float
    start: Optional[int] = None
    end: Optional[int] = None
    redacted_preview: Optional[str] = None


class ScanResponse(BaseModel):
    detections: list[Detection] = []
    tags: list[str] = []
    threat_level: ThreatLevel = ThreatLevel.safe
    action: SecurityAction = SecurityAction.allowed
    scan_time_ms: float = 0
    trace_context: dict = {}


class TraceEvalRequest(BaseModel):
    messages: list[dict]
    trace_id: Optional[str] = None


class TraceEvalResponse(BaseModel):
    violations: list[dict] = []
    action: SecurityAction = SecurityAction.allowed


class HealthResponse(BaseModel):
    status: str
    components: dict


# ─── Initialize detectors ────────────────────────────────────────────
from presidio_analyzer import AnalyzerEngine, PatternRecognizer, Pattern
from presidio_analyzer.nlp_engine import NlpEngineProvider

# Use en_core_web_md for better NER accuracy (person names, orgs, locations).
nlp_config = {
    "nlp_engine_name": "spacy",
    "models": [{"lang_code": "en", "model_name": "en_core_web_md"}],
}
nlp_engine = NlpEngineProvider(nlp_configuration=nlp_config).create_engine()

# Build custom recognizers for patterns Presidio doesn't cover natively
custom_recognizers = []

# Cloud provider API keys
cloud_patterns = [
    ("AWS_ACCESS_KEY", r"AKIA[0-9A-Z]{16}", 0.95),
    ("AWS_SECRET_KEY", r"(?:aws_secret_access_key|secret_key)\s*[=:]\s*[A-Za-z0-9/+=]{40}", 0.95),
    ("AZURE_KEY", r"[A-Za-z0-9+/]{86}==", 0.6),
    ("GCP_API_KEY", r"AIza[A-Za-z0-9_\-]{35}", 0.95),
    ("GCP_SERVICE_ACCOUNT", r'"type"\s*:\s*"service_account"', 0.9),
]

# AI/ML provider keys
ai_patterns = [
    ("OPENAI_KEY", r"sk-[A-Za-z0-9]{20}T3BlbkFJ[A-Za-z0-9]{20}", 0.98),
    ("ANTHROPIC_KEY", r"sk-(?:proj|ant)-[A-Za-z0-9\-_]{80,}", 0.98),
    ("HUGGINGFACE_TOKEN", r"hf_[A-Za-z0-9]{34,}", 0.95),
    ("REPLICATE_TOKEN", r"r8_[A-Za-z0-9]{40}", 0.95),
]

# SaaS platform tokens
saas_patterns = [
    ("GITHUB_TOKEN", r"(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36}", 0.95),
    ("GITHUB_CLASSIC", r"github_pat_[A-Za-z0-9_]{82}", 0.98),
    ("GITLAB_TOKEN", r"glpat-[A-Za-z0-9\-]{20}", 0.95),
    ("SLACK_TOKEN", r"xox[boaprs]-[0-9A-Za-z\-]{10,}", 0.95),
    ("SLACK_WEBHOOK", r"https://hooks\.slack\.com/services/T[A-Z0-9]+/B[A-Z0-9]+/[A-Za-z0-9]+", 0.95),
    ("DISCORD_TOKEN", r"[MN][A-Za-z0-9]{23,}\.[A-Za-z0-9\-_]{6}\.[A-Za-z0-9\-_]{27}", 0.9),
    ("DISCORD_WEBHOOK", r"https://discord(?:app)?\.com/api/webhooks/\d+/[A-Za-z0-9\-_]+", 0.95),
    ("STRIPE_KEY", r"(?:r|s)k_(?:live|test)_[A-Za-z0-9]{24,}", 0.95),
    ("SQUARE_KEY", r"sq0[a-z]{3}-[A-Za-z0-9\-_]{22,}", 0.9),
    ("SENDGRID_KEY", r"SG\.[A-Za-z0-9\-_]{22}\.[A-Za-z0-9\-_]{43}", 0.95),
    ("TWILIO_KEY", r"SK[0-9a-fA-F]{32}", 0.85),
    ("MAILGUN_KEY", r"key-[A-Za-z0-9]{32}", 0.85),
    ("DATADOG_KEY", r"[a-f0-9]{32}", 0.4),  # low confidence — too generic alone
    ("SHOPIFY_KEY", r"shpat_[A-Fa-f0-9]{32}", 0.95),
    ("VERCEL_TOKEN", r"[A-Za-z0-9]{24}", 0.3),  # very generic
    ("FIREBASE_KEY", r"AIza[A-Za-z0-9\-_]{35}", 0.9),
]

# Package registry tokens
registry_patterns = [
    ("NPM_TOKEN", r"npm_[A-Za-z0-9]{36}", 0.95),
    ("PYPI_TOKEN", r"pypi-AgEIcHlwaS5vcmc[A-Za-z0-9\-_]{50,}", 0.98),
    ("NUGET_KEY", r"oy2[A-Za-z0-9]{43}", 0.9),
    ("RUBYGEMS_KEY", r"rubygems_[A-Za-z0-9]{48}", 0.95),
    ("DOCKER_TOKEN", r"dckr_pat_[A-Za-z0-9\-_]{27}", 0.95),
]

# Crypto/blockchain
crypto_patterns = [
    ("ETH_PRIVATE_KEY", r"0x[0-9a-fA-F]{64}", 0.7),
    ("BTC_WIF", r"[5KL][1-9A-HJ-NP-Za-km-z]{50,51}", 0.7),
]

# Infrastructure secrets
infra_patterns = [
    ("JWT_TOKEN", r"eyJ[A-Za-z0-9\-_]{20,}\.eyJ[A-Za-z0-9\-_]{20,}\.[A-Za-z0-9\-_]{20,}", 0.85),
    ("DATABASE_URL", r"(?:postgres|mysql|mongodb|redis|amqp)://[^\s\"']+:[^\s\"']+@", 0.95),
    ("PRIVATE_KEY_PEM", r"-----BEGIN\s+(?:RSA\s+|EC\s+|DSA\s+|OPENSSH\s+)?PRIVATE\s+KEY-----", 0.99),
    ("BEARER_TOKEN", r"[Bb]earer\s+[A-Za-z0-9\-_.~+/]{20,}", 0.75),
    ("BASIC_AUTH", r"[Bb]asic\s+[A-Za-z0-9+/=]{20,}", 0.8),
    ("GENERIC_SECRET", r"(?:password|secret|token|key|credential)\s*[=:]\s*['\"][^\s'\"]{8,}['\"]", 0.7),
]

# Medical/HIPAA
medical_patterns = [
    ("MEDICAL_RECORD", r"MRN[:\s]*\d{6,}", 0.8),
    ("DEA_NUMBER", r"[A-Z][A-Z9][0-9]{7}", 0.6),
    ("NPI_NUMBER", r"\b\d{10}\b", 0.3),  # low confidence — too generic alone
]

# Register all custom patterns with Presidio
all_custom_patterns = (
    cloud_patterns + ai_patterns + saas_patterns + registry_patterns +
    crypto_patterns + infra_patterns + medical_patterns
)
for entity_type, regex_str, score in all_custom_patterns:
    recognizer = PatternRecognizer(
        supported_entity=entity_type,
        patterns=[Pattern(name=entity_type.lower(), regex=regex_str, score=score)],
    )
    custom_recognizers.append(recognizer)

presidio_analyzer = AnalyzerEngine(nlp_engine=nlp_engine)
for r in custom_recognizers:
    presidio_analyzer.registry.add_recognizer(r)

from detect_secrets.core.scan import scan_line
from detect_secrets.settings import default_settings

invariant_available = False
try:
    from invariant.analyzer import LocalPolicy
    invariant_available = True
except Exception:
    pass

# ─── Injection detection patterns ────────────────────────────────────
INJECTION_PATTERNS = [
    (r"ignore\s+(all\s+)?previous\s+instructions", "PROMPT_INJECTION", "ignore_previous", 0.9),
    (r"you\s+are\s+now\s+(?:a\s+)?(?:DAN|jailbreak|unrestricted)", "PROMPT_INJECTION", "jailbreak_identity", 0.95),
    (r"system:\s*you\s+are", "PROMPT_INJECTION", "system_override", 0.85),
    (r"<\|im_start\|>|<\|im_end\|>", "PROMPT_INJECTION", "chat_ml_injection", 0.95),
    (r"\[INST\]|\[/INST\]", "PROMPT_INJECTION", "llama_format_injection", 0.9),
    (r";\s*(?:rm|cat|curl|wget|nc|bash|sh|python|perl|ruby)\s", "COMMAND_INJECTION", "shell_command", 0.9),
    (r"\$\(.*\)|`.*`", "COMMAND_INJECTION", "command_substitution", 0.8),
    (r"\|\s*(?:bash|sh|python|perl)", "COMMAND_INJECTION", "pipe_to_shell", 0.9),
    (r"[\u0400-\u04FF]", "ENCODING_BYPASS", "cyrillic_chars", 0.6),
    (r"[\u200B-\u200F\u2028-\u202F\uFEFF]", "ENCODING_BYPASS", "zero_width_chars", 0.8),
    (r"(?:pretend|act\s+as\s+if|imagine)\s+(?:you\s+are|that)", "SOCIAL_ENGINEERING", "role_play", 0.7),
    (r"(?:emergency|urgent|critical).*(?:override|bypass|skip)", "SOCIAL_ENGINEERING", "urgency_bypass", 0.8),
    (r"(?:send|post|upload|transmit)\s+(?:all|the|this)\s+(?:data|content|file|secret|key|password|token)", "DATA_EXFIL", "exfil_attempt", 0.85),
    (r"(?:curl|wget|fetch)\s+https?://", "DATA_EXFIL", "external_request", 0.6),
]
compiled_injection_patterns = [
    (re.compile(p, re.IGNORECASE), tag, name, conf)
    for p, tag, name, conf in INJECTION_PATTERNS
]

# ─── Extra secrets patterns (secrets-patterns-db style) ──────────────
EXTRA_SECRET_PATTERNS = [
    (r"(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36}", "GITHUB_TOKEN", "github_fine_grained", 0.95),
    (r"xox[boaprs]-[0-9A-Za-z\-]{10,}", "SLACK_TOKEN", "slack_token", 0.95),
    (r"sk-[A-Za-z0-9]{20}T3BlbkFJ[A-Za-z0-9]{20}", "OPENAI_KEY", "openai_api_key", 0.98),
    (r"sk-(?:proj|ant)-[A-Za-z0-9\-_]{80,}", "ANTHROPIC_KEY", "anthropic_api_key", 0.98),
    (r"SG\.[A-Za-z0-9\-_]{22}\.[A-Za-z0-9\-_]{43}", "SENDGRID_KEY", "sendgrid_api_key", 0.95),
    (r"(?:r|s)k_(?:live|test)_[A-Za-z0-9]{24,}", "STRIPE_KEY", "stripe_api_key", 0.95),
    (r"sq0[a-z]{3}-[A-Za-z0-9\-_]{22,}", "SQUARE_KEY", "square_token", 0.9),
    (r"eyJ[A-Za-z0-9\-_]{20,}\.eyJ[A-Za-z0-9\-_]{20,}\.[A-Za-z0-9\-_]{20,}", "JWT_TOKEN", "jwt_bearer", 0.85),
    (r"(?:postgres|mysql|mongodb)://[^\s\"']+:[^\s\"']+@", "DATABASE_URL", "db_connection_string", 0.95),
    (r"-----BEGIN\s+(?:RSA\s+)?PRIVATE\s+KEY-----", "PRIVATE_KEY", "pem_private_key", 0.99),
    (r"AKIA[0-9A-Z]{16}", "AWS_ACCESS_KEY", "aws_access_key", 0.98),
    (r"(?:aws_secret_access_key|secret_key)\s*[=:]\s*[A-Za-z0-9/+=]{40}", "AWS_SECRET_KEY", "aws_secret_key", 0.95),
]
compiled_secret_patterns = [
    (re.compile(p), tag, name, conf)
    for p, tag, name, conf in EXTRA_SECRET_PATTERNS
]


def _redact_preview(text: str, start: int, end: int) -> str:
    matched = text[start:end]
    if len(matched) <= 6:
        return "***"
    return matched[:2] + "***" + matched[-2:]


# ─── Scan implementations ────────────────────────────────────────────
def scan_pii(text: str) -> list[Detection]:
    """Run Presidio with ALL registered recognizers (built-in + custom).

    Presidio natively detects: PERSON, PHONE_NUMBER, EMAIL_ADDRESS, CREDIT_CARD,
    CRYPTO, IBAN_CODE, IP_ADDRESS, MEDICAL_LICENSE, NRP, LOCATION, DATE_TIME,
    US_SSN, US_BANK_NUMBER, US_DRIVER_LICENSE, US_ITIN, US_PASSPORT, UK_NHS,
    SG_NRIC_FIN, AU_ABN, AU_ACN, AU_TFN, AU_MEDICARE, IN_PAN, IN_AADHAAR,
    plus all custom recognizers registered above (cloud keys, AI tokens, etc.)
    """
    detections = []
    try:
        # Analyze with ALL entities (None = use all registered recognizers)
        results = presidio_analyzer.analyze(
            text=text,
            language="en",
            entities=None,  # All registered entities
            score_threshold=0.3,  # Low threshold — we use confidence for triage
        )
        for r in results:
            rec_name = "unknown"
            if r.recognition_metadata:
                rec_name = r.recognition_metadata.get("recognizer_name", "unknown")
            # Classify finding_type based on entity category
            finding_type = "pii"
            if r.entity_type in (
                "AWS_ACCESS_KEY", "AWS_SECRET_KEY", "AZURE_KEY", "GCP_API_KEY",
                "GCP_SERVICE_ACCOUNT", "OPENAI_KEY", "ANTHROPIC_KEY",
                "HUGGINGFACE_TOKEN", "REPLICATE_TOKEN", "GITHUB_TOKEN",
                "GITHUB_CLASSIC", "GITLAB_TOKEN", "SLACK_TOKEN", "SLACK_WEBHOOK",
                "DISCORD_TOKEN", "DISCORD_WEBHOOK", "STRIPE_KEY", "SQUARE_KEY",
                "SENDGRID_KEY", "TWILIO_KEY", "MAILGUN_KEY", "SHOPIFY_KEY",
                "FIREBASE_KEY", "NPM_TOKEN", "PYPI_TOKEN", "NUGET_KEY",
                "RUBYGEMS_KEY", "DOCKER_TOKEN", "ETH_PRIVATE_KEY", "BTC_WIF",
                "JWT_TOKEN", "DATABASE_URL", "PRIVATE_KEY_PEM", "BEARER_TOKEN",
                "BASIC_AUTH", "GENERIC_SECRET", "DATADOG_KEY", "VERCEL_TOKEN",
            ):
                finding_type = "secret"
            elif r.entity_type in ("MEDICAL_RECORD", "DEA_NUMBER", "NPI_NUMBER"):
                finding_type = "medical"
            detections.append(Detection(
                detector="presidio",
                finding_type=finding_type,
                tag=r.entity_type,
                pattern_name=rec_name,
                confidence=r.score,
                start=r.start,
                end=r.end,
                redacted_preview=_redact_preview(text, r.start, r.end),
            ))
    except Exception as e:
        print(f"[presidio] Error: {e}")
    return detections


def scan_secrets_detect(text: str) -> list[Detection]:
    detections = []
    try:
        with default_settings():
            for line in text.splitlines():
                for secret in scan_line(line):
                    detections.append(Detection(
                        detector="detect_secrets",
                        finding_type="secret",
                        tag=secret.type.upper().replace(" ", "_"),
                        pattern_name=secret.type,
                        confidence=0.8,
                    ))
    except Exception as e:
        print(f"[detect-secrets] Error: {e}")
    return detections


def scan_secrets_extra(text: str) -> list[Detection]:
    detections = []
    for regex, tag, name, conf in compiled_secret_patterns:
        for m in regex.finditer(text):
            detections.append(Detection(
                detector="extra_patterns",
                finding_type="secret",
                tag=tag,
                pattern_name=name,
                confidence=conf,
                start=m.start(),
                end=m.end(),
                redacted_preview=_redact_preview(text, m.start(), m.end()),
            ))
    return detections


def scan_injection(text: str) -> list[Detection]:
    detections = []
    for regex, tag, name, conf in compiled_injection_patterns:
        for m in regex.finditer(text):
            detections.append(Detection(
                detector="injection",
                finding_type="injection",
                tag=tag,
                pattern_name=name,
                confidence=conf,
                start=m.start(),
                end=m.end(),
                redacted_preview=_redact_preview(text, m.start(), m.end()),
            ))
    return detections


def determine_threat_level(
    detections: list[Detection],
) -> tuple[ThreatLevel, SecurityAction]:
    if not detections:
        return ThreatLevel.safe, SecurityAction.allowed
    max_conf = max(d.confidence for d in detections)
    has_injection = any(d.finding_type == "injection" for d in detections)
    has_critical = any(
        d.tag in ("PRIVATE_KEY", "AWS_SECRET_KEY", "DATABASE_URL")
        for d in detections
    )
    if has_injection and max_conf >= 0.9:
        return ThreatLevel.critical, SecurityAction.blocked
    if has_critical:
        return ThreatLevel.dangerous, SecurityAction.blocked
    if has_injection and max_conf >= 0.7:
        return ThreatLevel.dangerous, SecurityAction.warned
    if max_conf >= 0.7:
        return ThreatLevel.suspicious, SecurityAction.warned
    return ThreatLevel.safe, SecurityAction.allowed


# ─── Endpoints ────────────────────────────────────────────────────────
@app.post("/scan", response_model=ScanResponse)
async def scan(req: ScanRequest):
    start_time = time.monotonic()
    detections = []
    detections.extend(scan_pii(req.text))
    detections.extend(scan_secrets_detect(req.text))
    detections.extend(scan_secrets_extra(req.text))
    detections.extend(scan_injection(req.text))
    tags = sorted(set(d.tag for d in detections))
    threat_level, action = determine_threat_level(detections)
    scan_time_ms = (time.monotonic() - start_time) * 1000
    return ScanResponse(
        detections=detections,
        tags=tags,
        threat_level=threat_level,
        action=action,
        scan_time_ms=round(scan_time_ms, 2),
        trace_context={
            "trace_id": req.trace_id,
            "span_id": req.span_id,
            "tool_name": req.tool_name,
            "context": req.context,
        },
    )


@app.post("/trace/evaluate", response_model=TraceEvalResponse)
async def trace_evaluate(req: TraceEvalRequest):
    if not invariant_available:
        return TraceEvalResponse(violations=[], action=SecurityAction.allowed)
    violations = []
    try:
        import yaml
        policy_path = os.path.join(os.path.dirname(__file__), "config", "policies.yaml")
        if os.path.exists(policy_path):
            with open(policy_path) as f:
                policy_config = yaml.safe_load(f)
            for rule in policy_config.get("rules", []):
                policy = LocalPolicy.from_string(rule["policy"])
                result = policy.analyze(req.messages)
                if result and result.errors:
                    for err in result.errors:
                        violations.append({
                            "rule_name": rule.get("name", "unnamed"),
                            "description": rule.get("description", ""),
                            "message": str(err),
                            "trace_id": req.trace_id,
                        })
    except Exception as e:
        print(f"[invariant] Error: {e}")
    action = SecurityAction.blocked if violations else SecurityAction.allowed
    return TraceEvalResponse(violations=violations, action=action)


@app.get("/health", response_model=HealthResponse)
async def health():
    supported = presidio_analyzer.get_supported_entities()
    return HealthResponse(
        status="ok",
        components={
            "presidio": f"ok ({len(supported)} entities)",
            "presidio_entities": ", ".join(sorted(supported)),
            "detect_secrets": "ok",
            "injection_patterns": f"{len(compiled_injection_patterns)} loaded",
            "secret_patterns": f"{len(compiled_secret_patterns)} loaded",
            "custom_recognizers": f"{len(custom_recognizers)} loaded",
            "spacy_model": "en_core_web_md",
            "invariant": "ok" if invariant_available else "unavailable",
        },
    )


# ─── Skill scan models & endpoint ─────────────────────────────────
class SkillScanIssue(BaseModel):
    code: str
    severity: str  # "critical", "high", "medium", "low", "info"
    description: str


class SkillScanRequest(BaseModel):
    content: str  # Full SKILL.md text
    skill_name: str = "unknown"


class SkillScanResponse(BaseModel):
    status: str  # "clean", "warning", "critical", "not_scanned"
    issues: list[SkillScanIssue] = []
    vt_verdict: Optional[str] = None  # "benign", "suspicious", "malicious", None
    vt_code_insight: Optional[str] = None


# Skill-specific dangerous patterns
SKILL_PATTERNS = [
    ("shell_exec", "subprocess.run or os.system call", r"(subprocess\.|os\.system|os\.popen|exec\(|eval\()"),
    ("network_access", "Network access attempt", r"(requests\.|urllib\.|httpx\.|aiohttp\.|curl |wget )"),
    ("file_write", "File write operation", r"(open\(.+['\"]w|write\(|shutil\.|os\.remove|os\.unlink)"),
    ("env_access", "Environment variable access", r"(os\.environ|os\.getenv|env\[)"),
]
compiled_skill_patterns = [
    (re.compile(p, re.IGNORECASE), code, desc)
    for code, desc, p in SKILL_PATTERNS
]


@app.post("/scan/skill", response_model=SkillScanResponse)
async def scan_skill(req: SkillScanRequest):
    issues: list[SkillScanIssue] = []

    # 1. Reuse existing detectors on the skill content
    local_detections = scan_injection(req.content) + scan_secrets_extra(req.content)
    for d in local_detections:
        issues.append(SkillScanIssue(
            code=d.tag,
            severity="high" if d.confidence > 0.8 else "medium",
            description=f"{d.detector}: {d.pattern_name}",
        ))

    # 2. Skill-specific pattern checks
    for regex, code, desc in compiled_skill_patterns:
        if regex.search(req.content):
            issues.append(SkillScanIssue(code=code, severity="medium", description=desc))

    # 3. VirusTotal analysis (optional — requires VIRUSTOTAL_API_KEY)
    vt_verdict: Optional[str] = None
    vt_code_insight: Optional[str] = None
    vt_api_key = os.environ.get("VIRUSTOTAL_API_KEY")

    if vt_api_key:
        try:
            import vt
            content_hash = hashlib.sha256(req.content.encode()).hexdigest()

            async with vt.Client(vt_api_key) as client:
                try:
                    file_report = await client.get_object_async(f"/files/{content_hash}")
                    stats = file_report.last_analysis_stats
                    if stats.get("malicious", 0) > 0:
                        vt_verdict = "malicious"
                    elif stats.get("suspicious", 0) > 0:
                        vt_verdict = "suspicious"
                    else:
                        vt_verdict = "benign"

                    # Get Code Insight if available
                    if hasattr(file_report, "crowdsourced_ai_results"):
                        for result in file_report.crowdsourced_ai_results:
                            if result.get("source") == "Code Insight":
                                vt_code_insight = result.get("analysis", "")
                                break
                except vt.error.APIError as e:
                    if e.code == "NotFoundError":
                        vt_verdict = None  # Not yet analyzed — skip
                    else:
                        raise
        except ImportError:
            pass  # vt-py not installed
        except Exception as e:
            import logging
            logging.warning(f"VT scan failed for {req.skill_name}: {e}")

    # 4. Determine overall status
    if vt_verdict == "malicious" or any(i.severity == "critical" for i in issues):
        status = "critical"
    elif vt_verdict == "suspicious" or any(i.severity == "high" for i in issues):
        status = "warning"
    elif issues:
        status = "warning"
    else:
        status = "clean"

    return SkillScanResponse(
        status=status,
        issues=issues,
        vt_verdict=vt_verdict,
        vt_code_insight=vt_code_insight,
    )


@app.get("/detectors")
async def list_detectors():
    supported = presidio_analyzer.get_supported_entities()
    return {
        "detectors": [
            {"name": "presidio", "type": "pii+secrets", "status": "active",
             "entities": sorted(supported),
             "entity_count": len(supported),
             "model": "en_core_web_md",
             "custom_recognizers": len(custom_recognizers)},
            {"name": "detect_secrets", "type": "secret", "status": "active"},
            {"name": "extra_patterns", "type": "secret", "status": "active",
             "patterns": len(compiled_secret_patterns)},
            {"name": "injection", "type": "injection", "status": "active",
             "patterns": len(compiled_injection_patterns)},
            {"name": "invariant", "type": "flow",
             "status": "active" if invariant_available else "inactive"},
        ]
    }
