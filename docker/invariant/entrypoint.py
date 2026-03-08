"""
Invariant Gateway — inline MCP proxy with flow-based security policies.
Listens on :8083, proxies to UPSTREAM_URL (default: http://agentgateway:3000).
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import time
from collections import defaultdict, deque
from datetime import datetime
from typing import Any

import httpx
import yaml
from fastapi import FastAPI, Request, Response
from fastapi.responses import StreamingResponse
from pydantic import BaseModel

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("invariant-gateway")

UPSTREAM_URL = os.environ.get("UPSTREAM_URL", "http://agentgateway:3000").rstrip("/")
POLICIES_PATH = os.environ.get("POLICIES_PATH", "/app/config/policies.yaml")
MAX_VIOLATIONS = 1000

# ---------------------------------------------------------------------------
# Policy engine
# ---------------------------------------------------------------------------

class Policy(BaseModel):
    name: str
    type: str  # after_sequence | rate_limit | blocked_tools
    message: str = "Policy violation"
    # after_sequence
    trigger: str | None = None
    block: str | None = None
    # rate_limit
    tool: str | None = None
    max_calls: int | None = None
    window_seconds: int | None = None
    # blocked_tools
    tools: list[str] | None = None


def load_policies(path: str) -> list[Policy]:
    try:
        with open(path) as f:
            data = yaml.safe_load(f)
        return [Policy(**p) for p in data.get("policies", [])]
    except Exception as exc:
        log.warning("Failed to load policies from %s: %s", path, exc)
        return []


policies: list[Policy] = load_policies(POLICIES_PATH)

# Per-session state: session_id → list of tool names called (in order)
session_sequences: dict[str, list[str]] = defaultdict(list)

# Per-session rate limit counters: session_id → tool → deque of timestamps
session_rate_buckets: dict[str, dict[str, deque]] = defaultdict(lambda: defaultdict(deque))

# Violation log (last MAX_VIOLATIONS entries)
violation_log: deque[dict[str, Any]] = deque(maxlen=MAX_VIOLATIONS)


def record_violation(session_id: str, policy: Policy, tool_name: str) -> None:
    entry = {
        "ts": datetime.utcnow().isoformat() + "Z",
        "session_id": session_id,
        "policy": policy.name,
        "tool": tool_name,
        "message": policy.message,
    }
    violation_log.appendleft(entry)
    log.warning("VIOLATION session=%s policy=%s tool=%s", session_id, policy.name, tool_name)


def evaluate(session_id: str, tool_name: str) -> Policy | None:
    """Return the first violated policy, or None if clear."""
    now = time.monotonic()

    for pol in policies:
        if pol.type == "blocked_tools":
            if pol.tools and tool_name in pol.tools:
                record_violation(session_id, pol, tool_name)
                return pol

        elif pol.type == "after_sequence":
            if pol.block == tool_name:
                seq = session_sequences[session_id]
                if pol.trigger and pol.trigger in seq:
                    record_violation(session_id, pol, tool_name)
                    return pol

        elif pol.type == "rate_limit":
            if pol.tool == tool_name and pol.max_calls and pol.window_seconds:
                bucket = session_rate_buckets[session_id][tool_name]
                # Evict old entries
                cutoff = now - pol.window_seconds
                while bucket and bucket[0] < cutoff:
                    bucket.popleft()
                if len(bucket) >= pol.max_calls:
                    record_violation(session_id, pol, tool_name)
                    return pol

    return None


def record_call(session_id: str, tool_name: str) -> None:
    session_sequences[session_id].append(tool_name)
    session_rate_buckets[session_id][tool_name].append(time.monotonic())


def extract_tool_name(body: dict[str, Any]) -> str | None:
    """Best-effort extraction of MCP tool name from request body."""
    # MCP call_tool: {"method": "tools/call", "params": {"name": "bash", ...}}
    if body.get("method") == "tools/call":
        return body.get("params", {}).get("name")
    # Generic name field
    return body.get("name") or body.get("tool")


# ---------------------------------------------------------------------------
# HTTP proxy
# ---------------------------------------------------------------------------

app = FastAPI(title="Invariant Gateway", version="1.0.0")
transport = httpx.AsyncHTTPTransport(retries=2)
client = httpx.AsyncClient(base_url=UPSTREAM_URL, transport=transport, timeout=120.0)


@app.on_event("shutdown")
async def shutdown_event() -> None:
    await client.aclose()


@app.get("/health")
async def health() -> dict[str, Any]:
    return {
        "status": "ok",
        "upstream": UPSTREAM_URL,
        "policies_loaded": len(policies),
    }


@app.get("/api/v1/events")
async def events() -> dict[str, Any]:
    return {"violations": list(violation_log)}


@app.api_route("/{path:path}", methods=["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"])
async def proxy(path: str, request: Request) -> Response:
    # Read request body
    raw_body = await request.body()
    session_id = request.headers.get("x-session-id", "default")

    # Parse body for tool extraction (best-effort)
    tool_name: str | None = None
    if raw_body:
        try:
            body_json = json.loads(raw_body)
            tool_name = extract_tool_name(body_json)
        except (json.JSONDecodeError, AttributeError):
            pass

    # Policy evaluation
    if tool_name:
        violated = evaluate(session_id, tool_name)
        if violated:
            return Response(
                content=json.dumps({"error": violated.message, "policy": violated.name}),
                status_code=403,
                media_type="application/json",
            )
        record_call(session_id, tool_name)

    # Forward to upstream
    upstream_url = f"/{path}"
    if request.url.query:
        upstream_url += f"?{request.url.query}"

    headers = {
        k: v
        for k, v in request.headers.items()
        if k.lower() not in ("host", "content-length", "transfer-encoding")
    }

    try:
        upstream_req = client.build_request(
            method=request.method,
            url=upstream_url,
            headers=headers,
            content=raw_body,
        )
        upstream_resp = await client.send(upstream_req, stream=True)
    except httpx.RequestError as exc:
        log.error("Upstream error: %s", exc)
        return Response(
            content=json.dumps({"error": f"Upstream unreachable: {exc}"}),
            status_code=502,
            media_type="application/json",
        )

    # Stream response back
    response_headers = {
        k: v
        for k, v in upstream_resp.headers.items()
        if k.lower() not in ("transfer-encoding", "content-encoding")
    }

    async def stream_body():
        async for chunk in upstream_resp.aiter_bytes():
            yield chunk
        await upstream_resp.aclose()

    return StreamingResponse(
        stream_body(),
        status_code=upstream_resp.status_code,
        headers=response_headers,
        media_type=upstream_resp.headers.get("content-type"),
    )


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="0.0.0.0", port=8083, log_level="info")
