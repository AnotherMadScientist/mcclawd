"""
AgentGuard — regex-based security analysis sidecar.
Detects command injection, prompt injection, unicode homoglyphs,
social engineering, path traversal, and encoding bypass attempts.
Port: 8084
"""

from __future__ import annotations

import re
import uvicorn
from fastapi import FastAPI
from pydantic import BaseModel

app = FastAPI(title="AgentGuard", version="1.0.0")

# ---------------------------------------------------------------------------
# Detection patterns
# Each entry: (compiled_regex, category, pattern_name, confidence)
# ---------------------------------------------------------------------------

DETECTION_PATTERNS: list[tuple[re.Pattern, str, str, float]] = []


def _add(pattern: str, category: str, name: str, confidence: float) -> None:
    DETECTION_PATTERNS.append((re.compile(pattern, re.IGNORECASE | re.MULTILINE), category, name, confidence))


# ── Command Injection ──────────────────────────────────────────────────────
_add(r"rm\s+-[rRfF]{1,3}\s+[/~\.\*]", "command_injection", "rm_rf", 0.95)
_add(r";\s*(?:rm|mv|cp|dd|mkfs|wget|curl|nc|ncat|bash|sh|zsh)\b", "command_injection", "semicolon_chain", 0.90)
_add(r"&&\s*(?:rm|mv|cp|dd|mkfs|wget|curl|nc|ncat|bash|sh|zsh)\b", "command_injection", "and_chain", 0.90)
_add(r"\|\|\s*(?:rm|mv|cp|dd|mkfs|wget|curl|nc|ncat|bash|sh|zsh)\b", "command_injection", "or_chain", 0.90)
_add(r"`[^`]{1,200}`", "command_injection", "backtick_exec", 0.85)
_add(r"\$\([^)]{1,200}\)", "command_injection", "dollar_paren_exec", 0.85)
_add(r"\|\s*(?:sh|bash|zsh|dash|ksh|csh)\b", "command_injection", "pipe_to_shell", 0.95)
_add(r"chmod\s+(?:777|a\+[rwx]{1,3}|0?777)\s+", "command_injection", "chmod_777", 0.90)
_add(r"curl\s+[^\s]+\s*\|\s*(?:sudo\s+)?(?:sh|bash)", "command_injection", "curl_pipe_shell", 0.98)
_add(r"wget\s+[^\s]+\s*(?:-O\s*-\s*)?\|\s*(?:sudo\s+)?(?:sh|bash)", "command_injection", "wget_pipe_shell", 0.98)
_add(r">\s*/dev/(?:sda|hda|nvme|null)\b", "command_injection", "device_write", 0.95)
_add(r"\beval\s+(?:\"|\$|\`)", "command_injection", "eval_exec", 0.88)

# ── Prompt Injection ───────────────────────────────────────────────────────
_add(r"ignore\s+(?:all\s+)?(?:previous|prior|above)\s+instructions?", "prompt_injection", "ignore_previous_instructions", 0.95)
_add(r"you\s+are\s+now\s+(?:a|an|the)\s+\w", "prompt_injection", "you_are_now", 0.88)
_add(r"system\s+prompt", "prompt_injection", "system_prompt_reference", 0.80)
_add(r"forget\s+(?:your|all|the|these)\s+(?:rules|instructions?|guidelines?|constraints?|training)", "prompt_injection", "forget_your_rules", 0.92)
_add(r"disregard\s+(?:all\s+)?(?:previous|prior|your)\s+(?:instructions?|rules|guidelines?)", "prompt_injection", "disregard_instructions", 0.92)
_add(r"override\s+(?:your\s+)?(?:instructions?|safety|guidelines?|rules)", "prompt_injection", "override_instructions", 0.90)
_add(r"new\s+instructions?:\s*\n", "prompt_injection", "new_instructions_block", 0.85)
_add(r"<\s*/?(?:system|instruction|prompt)\s*>", "prompt_injection", "xml_injection_tag", 0.88)
_add(r"\[\s*INST\s*\]|\[/?SYS\]", "prompt_injection", "llama_injection_tokens", 0.92)

# ── Unicode Homoglyphs ─────────────────────────────────────────────────────
# Common Cyrillic lookalikes for Latin characters
_add(r"[\u0430\u0435\u043e\u0441\u0440\u0440\u0456\u0458\u04cf]", "unicode_homoglyph", "cyrillic_latin_lookalike", 0.75)
# Greek lookalikes
_add(r"[\u03BF\u03B1\u03B5\u03BA\u03BD\u03C1\u03C3]", "unicode_homoglyph", "greek_latin_lookalike", 0.70)
# Zero-width characters (invisible injection)
_add(r"[\u200b\u200c\u200d\u2060\ufeff]", "unicode_homoglyph", "zero_width_char", 0.85)
# Fullwidth ASCII
_add(r"[\uff01-\uff5e]", "unicode_homoglyph", "fullwidth_ascii", 0.72)

# ── Social Engineering ─────────────────────────────────────────────────────
_add(r"pretend\s+(?:to\s+be|you\s+are|you're)", "social_engineering", "pretend_to_be", 0.88)
_add(r"roleplay\s+as\b", "social_engineering", "roleplay_as", 0.85)
_add(r"act\s+as\s+(?:root|admin|superuser|sudo|god|unrestricted)", "social_engineering", "act_as_privileged", 0.92)
_add(r"(?:jailbreak|DAN|do\s+anything\s+now)", "social_engineering", "jailbreak_keyword", 0.90)
_add(r"(?:developer|god|admin|root)\s+mode", "social_engineering", "privileged_mode", 0.85)
_add(r"bypass\s+(?:your\s+)?(?:safety|filters?|restrictions?|alignment)", "social_engineering", "bypass_safety", 0.90)
_add(r"(?:for\s+educational|for\s+research|hypothetically|in\s+fiction)\s+(?:purposes?,?\s+)?(?:how\s+(?:do|would|can)\s+(?:I|you|one)\s+)", "social_engineering", "educational_bypass", 0.75)

# ── Path Traversal ─────────────────────────────────────────────────────────
_add(r"(?:\.\./){2,}", "path_traversal", "dotdot_slash", 0.92)
_add(r"\.\.\\(?:\.\.\\)+", "path_traversal", "dotdot_backslash", 0.92)
_add(r"/etc/(?:passwd|shadow|sudoers|crontab|hosts|ssh)", "path_traversal", "etc_sensitive_file", 0.95)
_add(r"/proc/(?:self|[0-9]+)/(?:environ|mem|maps|cmdline)", "path_traversal", "proc_sensitive", 0.95)
_add(r"(?:%2e%2e%2f|%2e%2e/|\.\.%2f){1,}", "path_traversal", "url_encoded_traversal", 0.90)
_add(r"/(?:root|home/\w+)/\.(?:ssh|aws|gnupg|bash_history|zsh_history)", "path_traversal", "home_dotfile", 0.88)

# ── Encoding Bypass ────────────────────────────────────────────────────────
_add(r"base64\s*(?:-d|--decode|decode)", "encoding_bypass", "base64_decode", 0.85)
_add(r"echo\s+[A-Za-z0-9+/]{20,}={0,2}\s*\|\s*base64\s*-d", "encoding_bypass", "base64_pipe_decode", 0.92)
_add(r"\\x(?:[0-9a-fA-F]{2}){4,}", "encoding_bypass", "hex_escape_sequence", 0.80)
_add(r"\\u(?:[0-9a-fA-F]{4}){2,}", "encoding_bypass", "unicode_escape_sequence", 0.78)
_add(r"(?:fromCharCode|charCodeAt|btoa|atob)\s*\(", "encoding_bypass", "js_encoding_func", 0.80)
_add(r"python\s*-c\s*['\"].*(?:exec|eval|__import__|compile)", "encoding_bypass", "python_exec_oneliner", 0.92)
_add(r"perl\s+-e\s*['\"].*(?:exec|system|backtick)", "encoding_bypass", "perl_exec_oneliner", 0.90)

# ---------------------------------------------------------------------------
# Threat level classification
# ---------------------------------------------------------------------------

CRITICAL_THRESHOLD = 2      # 2+ high-confidence detections
HIGH_CONFIDENCE = 0.88      # threshold to count as "high confidence"

SANITIZE_BLOCKLIST: list[tuple[re.Pattern, str]] = [
    (re.compile(r"rm\s+-[rRfF]{1,3}\s+[/~\.\*]", re.IGNORECASE), "[CMD_BLOCKED]"),
    (re.compile(r"`[^`]{0,200}`"), "[BACKTICK_BLOCKED]"),
    (re.compile(r"\$\([^)]{0,200}\)"), "[SUBSHELL_BLOCKED]"),
    (re.compile(r"\|\s*(?:sh|bash|zsh|dash)", re.IGNORECASE), "[PIPE_SHELL_BLOCKED]"),
    (re.compile(r"(?:\.\./){2,}"), "[TRAVERSAL_BLOCKED]"),
    (re.compile(r"/etc/(?:passwd|shadow|sudoers)", re.IGNORECASE), "[SENSITIVE_PATH_BLOCKED]"),
    (re.compile(r"ignore\s+(?:all\s+)?(?:previous|prior|above)\s+instructions?", re.IGNORECASE), "[PROMPT_INJECTION_BLOCKED]"),
    (re.compile(r"[\u200b\u200c\u200d\u2060\ufeff]"), "[ZWC_BLOCKED]"),
]


def _classify_threat(detections: list[dict]) -> str:
    if not detections:
        return "safe"
    high_conf = [d for d in detections if d["confidence"] >= HIGH_CONFIDENCE]
    if len(high_conf) >= CRITICAL_THRESHOLD:
        return "critical"
    if high_conf:
        return "dangerous"
    return "suspicious"


def _summarize(threat_level: str, detections: list[dict]) -> str:
    if threat_level == "safe":
        return "No threats detected."
    cats = list({d["category"] for d in detections})
    cat_str = ", ".join(cats)
    count = len(detections)
    return f"{threat_level.capitalize()}: {count} detection(s) across categories: {cat_str}."


# ---------------------------------------------------------------------------
# Request / Response models
# ---------------------------------------------------------------------------

class AnalyzeRequest(BaseModel):
    text: str
    context: str = ""


class Detection(BaseModel):
    category: str
    pattern: str
    confidence: float


class AnalyzeResponse(BaseModel):
    threat_level: str
    detections: list[Detection]
    summary: str


class SanitizeRequest(BaseModel):
    text: str


class SanitizeResponse(BaseModel):
    sanitized: str
    patterns_blocked: list[str]


class HealthResponse(BaseModel):
    status: str
    version: str
    categories: int


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------

@app.post("/analyze", response_model=AnalyzeResponse)
def analyze(req: AnalyzeRequest) -> AnalyzeResponse:
    combined = f"{req.context}\n{req.text}" if req.context else req.text
    detections: list[dict] = []
    for pattern, category, name, confidence in DETECTION_PATTERNS:
        if pattern.search(combined):
            detections.append({"category": category, "pattern": name, "confidence": confidence})
    threat_level = _classify_threat(detections)
    return AnalyzeResponse(
        threat_level=threat_level,
        detections=[Detection(**d) for d in detections],
        summary=_summarize(threat_level, detections),
    )


@app.post("/sanitize", response_model=SanitizeResponse)
def sanitize(req: SanitizeRequest) -> SanitizeResponse:
    text = req.text
    blocked: list[str] = []
    for pattern, replacement in SANITIZE_BLOCKLIST:
        new_text, n = pattern.subn(replacement, text)
        if n > 0:
            blocked.append(replacement)
            text = new_text
    return SanitizeResponse(sanitized=text, patterns_blocked=blocked)


@app.get("/health", response_model=HealthResponse)
def health() -> HealthResponse:
    categories = len({cat for _, cat, _, _ in DETECTION_PATTERNS})
    return HealthResponse(status="ok", version="1.0.0", categories=categories)


# ---------------------------------------------------------------------------
# Entrypoint
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8084, log_level="info")
