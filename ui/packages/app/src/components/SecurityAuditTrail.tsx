import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Shield, ChevronDown, ChevronRight } from "lucide-react";
import { api } from "../api/client";
import { cn } from "../lib/utils";
import type { SecurityEvent, DlpFindingRow } from "../api/types";

interface SecurityAuditTrailProps {
  taskId: string;
}

function timeAgo(dateStr: string): string {
  const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

function ThreatDot({ level }: { level: string | null }) {
  const color =
    level === "safe"
      ? "bg-emerald-500"
      : level === "suspicious"
        ? "bg-amber-500"
        : level === "dangerous" || level === "critical"
          ? "bg-red-500"
          : "bg-muted-foreground";
  return <span className={cn("inline-block rounded-full flex-shrink-0", color)} style={{ width: 6, height: 6 }} />;
}

function EventTypeBadge({ eventType }: { eventType: string }) {
  const map: Record<string, { label: string; className: string }> = {
    dlp_match: { label: "DLP Match", className: "bg-violet-500/15 text-violet-400 border-violet-500/30" },
    secret_detected: { label: "Secret", className: "bg-orange-500/15 text-orange-400 border-orange-500/30" },
    injection_attempt: { label: "Injection", className: "bg-red-500/15 text-red-400 border-red-500/30" },
    pii_detected: { label: "PII", className: "bg-blue-500/15 text-blue-400 border-blue-500/30" },
    flow_violation: { label: "Flow", className: "bg-amber-500/15 text-amber-400 border-amber-500/30" },
    tool_blocked: { label: "Blocked", className: "bg-red-500/15 text-red-400 border-red-500/30" },
  };
  const entry = map[eventType] ?? { label: eventType, className: "bg-muted text-muted-foreground border-border" };
  return (
    <span
      className={cn(
        "inline-flex items-center px-1.5 py-0 rounded text-[10px] font-medium border leading-4",
        entry.className,
      )}
    >
      {entry.label}
    </span>
  );
}

function ActionBadge({ action }: { action: string }) {
  const map: Record<string, string> = {
    allowed: "bg-emerald-500/15 text-emerald-400 border-emerald-500/30",
    warned: "bg-amber-500/15 text-amber-400 border-amber-500/30",
    blocked: "bg-red-500/15 text-red-400 border-red-500/30",
    redacted: "bg-violet-500/15 text-violet-400 border-violet-500/30",
  };
  const cls = map[action] ?? "bg-muted text-muted-foreground border-border";
  return (
    <span className={cn("inline-flex items-center px-1.5 py-0 rounded text-[10px] font-medium border leading-4", cls)}>
      {action}
    </span>
  );
}

function FindingRow({ finding }: { finding: DlpFindingRow }) {
  return (
    <div className="flex items-center gap-2 pl-4 py-0.5 text-[11px] text-muted-foreground">
      <span className="text-muted-foreground/40 select-none">└─</span>
      <span className="font-mono text-foreground/60">{finding.finding_type}</span>
      <span className="px-1 py-0 rounded bg-muted text-muted-foreground text-[10px]">{finding.tag}</span>
      {finding.confidence != null && (
        <span className="text-muted-foreground/60">{Math.round(finding.confidence * 100)}%</span>
      )}
      {finding.redacted_preview && (
        <span className="font-mono text-muted-foreground/50 truncate max-w-[200px]">{finding.redacted_preview}</span>
      )}
    </div>
  );
}

function EventRow({ event }: { event: SecurityEvent }) {
  const [expanded, setExpanded] = useState(false);
  const hasFindings = event.findings && event.findings.length > 0;

  return (
    <div>
      <button
        className="w-full flex items-center gap-2 px-3 py-1 hover:bg-muted/40 transition-colors text-left group"
        onClick={() => hasFindings && setExpanded((v) => !v)}
        aria-expanded={hasFindings ? expanded : undefined}
      >
        <ThreatDot level={event.threat_level} />

        <span className="text-[10px] text-muted-foreground/60 w-12 flex-shrink-0 tabular-nums">
          {timeAgo(event.created_at)}
        </span>

        <EventTypeBadge eventType={event.event_type} />

        {event.tool_name && (
          <span className="font-mono text-[11px] text-muted-foreground/70 truncate flex-1 min-w-0">
            {event.tool_name}
          </span>
        )}

        <ActionBadge action={event.action_taken} />

        {hasFindings && (
          <span className="text-muted-foreground/40 ml-1 flex-shrink-0">
            {expanded ? (
              <ChevronDown className="w-3 h-3" />
            ) : (
              <ChevronRight className="w-3 h-3" />
            )}
          </span>
        )}
      </button>

      {expanded && hasFindings && (
        <div className="pb-1">
          {event.findings.map((f, i) => (
            <FindingRow key={i} finding={f} />
          ))}
        </div>
      )}
    </div>
  );
}

export function SecurityAuditTrail({ taskId }: SecurityAuditTrailProps) {
  const [collapsed, setCollapsed] = useState(false);

  const { data: events = [] } = useQuery<SecurityEvent[]>({
    queryKey: ["security-events", taskId],
    queryFn: () => api.security.events(taskId),
    refetchInterval: 10_000,
  });

  if (events.length === 0) return null;

  return (
    <div className="mt-2 rounded-lg border border-border/60 overflow-hidden text-sm">
      {/* Header */}
      <button
        className="w-full flex items-center gap-2 px-3 py-2 bg-muted/30 hover:bg-muted/50 transition-colors text-left"
        onClick={() => setCollapsed((v) => !v)}
        aria-expanded={!collapsed}
      >
        <Shield className="w-3.5 h-3.5 text-muted-foreground flex-shrink-0" />
        <span className="text-xs font-medium text-muted-foreground flex-1">Security Audit</span>
        <span className="text-[10px] font-medium px-1.5 py-0.5 rounded-full bg-muted text-muted-foreground">
          {events.length}
        </span>
        {collapsed ? (
          <ChevronRight className="w-3.5 h-3.5 text-muted-foreground" />
        ) : (
          <ChevronDown className="w-3.5 h-3.5 text-muted-foreground" />
        )}
      </button>

      {/* Event rows */}
      {!collapsed && (
        <div className="divide-y divide-border/30">
          {events.map((event) => (
            <EventRow key={event.id} event={event} />
          ))}
        </div>
      )}
    </div>
  );
}
