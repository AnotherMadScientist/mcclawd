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
from presidio_analyzer import AnalyzerEngine
presidio_analyzer = AnalyzerEngine()

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
    detections = []
    try:
        results = presidio_analyzer.analyze(text=text, language="en")
        for r in results:
            rec_name = "unknown"
            if r.recognition_metadata:
                rec_name = r.recognition_metadata.get("recognizer_name", "unknown")
            detections.append(Detection(
                detector="presidio",
                finding_type="pii",
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
    return HealthResponse(
        status="ok",
        components={
            "presidio": "ok",
            "detect_secrets": "ok",
            "injection_patterns": f"{len(compiled_injection_patterns)} loaded",
            "secret_patterns": f"{len(compiled_secret_patterns)} loaded",
            "invariant": "ok" if invariant_available else "unavailable",
        },
    )


@app.get("/detectors")
async def list_detectors():
    return {
        "detectors": [
            {"name": "presidio", "type": "pii", "status": "active",
             "entities": len(presidio_analyzer.get_supported_entities())},
            {"name": "detect_secrets", "type": "secret", "status": "active"},
            {"name": "extra_patterns", "type": "secret", "status": "active",
             "patterns": len(compiled_secret_patterns)},
            {"name": "injection", "type": "injection", "status": "active",
             "patterns": len(compiled_injection_patterns)},
            {"name": "invariant", "type": "flow",
             "status": "active" if invariant_available else "inactive"},
        ]
    }
