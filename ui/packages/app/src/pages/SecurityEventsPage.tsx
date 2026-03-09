import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Shield,
  ShieldAlert,
  ShieldCheck,
  ShieldX,
  Activity,
  ChevronDown,
  ChevronRight,
  FileSearch,
  AlertTriangle,
  X,
  Eye,
} from "lucide-react";
import { api } from "../api/client";
import type { DlpFindingRow, SecurityEvent, TaskSecurityGroup } from "../api/types";

// --- Status Bar (shared look) ---

function SecurityStatusBar() {
  const { data: status } = useQuery({
    queryKey: ["security-status"],
    queryFn: () => api.security.status(),
    retry: false,
  });
  const { data: policies = [] } = useQuery({
    queryKey: ["security-policies"],
    queryFn: () => api.security.policies(),
    retry: false,
  });

  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded-lg px-4 py-3 flex items-center gap-6 text-sm flex-wrap">
      <div className="flex items-center gap-2">
        <Activity className="w-4 h-4 text-zinc-400" />
        <span className="text-zinc-400">Pipeline hooks:</span>
        <span className="text-zinc-100 font-medium">{status ? status.pipeline_hooks : "\u2014"}</span>
      </div>
      <div className="w-px h-4 bg-zinc-700" />
      <div className="flex items-center gap-2">
        <Shield className="w-4 h-4 text-zinc-400" />
        <span className="text-zinc-400">Detection Patterns:</span>
        <span className="text-zinc-100 font-medium">{status ? status.dlp_pattern_count : "\u2014"}</span>
      </div>
      <div className="w-px h-4 bg-zinc-700" />
      <div className="flex items-center gap-2">
        <Shield className="w-4 h-4 text-zinc-400" />
        <span className="text-zinc-400">Response Rules:</span>
        <span className="text-zinc-100 font-medium">{policies.length}</span>
      </div>
      <div className="w-px h-4 bg-zinc-700" />
      <div className="flex items-center gap-2">
        <span className={`w-2 h-2 rounded-full ${
          status == null ? "bg-zinc-600"
            : status.sidecar_status === "healthy" ? "bg-green-400"
            : status.sidecar_status === "unhealthy" ? "bg-red-400"
            : "bg-zinc-500"
        }`} />
        <span className="text-zinc-400">Sidecar:</span>
        <span className="text-zinc-100 font-medium">{
          status == null ? "\u2014"
            : status.sidecar_status === "healthy" ? "Healthy"
            : status.sidecar_status === "unhealthy" ? "Unhealthy"
            : "Not configured"
        }</span>
      </div>
    </div>
  );
}

// --- Helpers ---

function timeAgo(isoString: string): string {
  const diff = Date.now() - new Date(isoString).getTime();
  const s = Math.floor(diff / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

function truncate(s: string, max = 80): string {
  return s.length > max ? s.slice(0, max) + "\u2026" : s;
}

function EventTypeBadge({ type }: { type: string }) {
  const map: Record<string, string> = {
    dlp_match: "bg-purple-900/60 text-purple-300 border-purple-700",
    secret_detected: "bg-red-900/60 text-red-300 border-red-700",
    injection_attempt: "bg-orange-900/60 text-orange-300 border-orange-700",
    pii_detected: "bg-blue-900/60 text-blue-300 border-blue-700",
    flow_violation: "bg-pink-900/60 text-pink-300 border-pink-700",
    tool_blocked: "bg-yellow-900/60 text-yellow-300 border-yellow-700",
    audit: "bg-zinc-800 text-zinc-400 border-zinc-700",
  };
  const cls = map[type] ?? "bg-zinc-800 text-zinc-300 border-zinc-700";
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs border font-medium ${cls}`}>
      {type.replace(/_/g, " ")}
    </span>
  );
}

function ThreatBadge({ level }: { level: string | null }) {
  if (!level || level === "none") return null;
  const map: Record<string, string> = {
    safe: "bg-green-900/60 text-green-300 border-green-700",
    suspicious: "bg-yellow-900/60 text-yellow-300 border-yellow-700",
    dangerous: "bg-orange-900/60 text-orange-300 border-orange-700",
    critical: "bg-red-900/60 text-red-300 border-red-700",
  };
  const cls = map[level] ?? "bg-zinc-800 text-zinc-300 border-zinc-700";
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs border font-medium ${cls}`}>
      {level}
    </span>
  );
}

function ActionBadge({ action }: { action: string }) {
  const map: Record<string, string> = {
    allowed: "bg-green-900/60 text-green-300 border-green-700",
    warned: "bg-yellow-900/60 text-yellow-300 border-yellow-700",
    blocked: "bg-red-900/60 text-red-300 border-red-700",
    redacted: "bg-blue-900/60 text-blue-300 border-blue-700",
  };
  const cls = map[action] ?? "bg-zinc-800 text-zinc-300 border-zinc-700";
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs border font-medium ${cls}`}>
      {action}
    </span>
  );
}

/** Renders highlighted source text: text before match, highlighted match, text after match. */
function HighlightedSource({ source_text, match_offset, match_length }: {
  source_text: string;
  match_offset: number;
  match_length: number;
}) {
  const before = source_text.slice(0, match_offset);
  const matched = source_text.slice(match_offset, match_offset + match_length);
  const after = source_text.slice(match_offset + match_length);
  return (
    <span>
      {before}
      <mark className="bg-yellow-500/40 text-yellow-200 px-0.5 rounded">{matched}</mark>
      {after}
    </span>
  );
}

/** Modal showing the full source context of a DLP finding with highlighted match. */
function FindingContextModal({ finding, onClose }: { finding: DlpFindingRow; onClose: () => void }) {
  const hasContext = finding.source_text && finding.match_offset != null && finding.match_length != null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onClick={onClose}>
      <div
        className="bg-zinc-900 border border-zinc-700 rounded-xl shadow-2xl max-w-2xl w-full mx-4 max-h-[80vh] flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-zinc-800">
          <div className="flex items-center gap-2 min-w-0">
            <Eye className="w-5 h-5 text-yellow-400 flex-shrink-0" />
            <span className="text-sm font-semibold text-zinc-100">Finding Context</span>
            {finding.pattern_name && (
              <span className="text-xs px-2 py-0.5 rounded bg-zinc-700/60 text-zinc-300 border border-zinc-600">
                {finding.pattern_name}
              </span>
            )}
          </div>
          <button type="button" onClick={onClose} className="text-zinc-400 hover:text-zinc-100 transition-colors">
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Metadata */}
        <div className="px-5 py-3 border-b border-zinc-800 flex items-center gap-4 flex-wrap text-xs">
          <div className="flex items-center gap-1.5">
            <span className="text-zinc-500">Type:</span>
            <span className="text-zinc-300">{finding.finding_type.replace(/_/g, " ")}</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="text-zinc-500">Location:</span>
            <span className="text-zinc-300">{finding.tag}</span>
          </div>
          {finding.confidence != null && (
            <div className="flex items-center gap-1.5">
              <span className="text-zinc-500">Confidence:</span>
              <span className={`font-medium ${finding.confidence >= 0.8 ? "text-red-400" : finding.confidence >= 0.5 ? "text-yellow-400" : "text-zinc-300"}`}>
                {Math.round(finding.confidence * 100)}%
              </span>
            </div>
          )}
        </div>

        {/* Source text with highlight */}
        <div className="flex-1 overflow-y-auto px-5 py-4">
          {hasContext ? (
            <div>
              <div className="text-xs text-zinc-500 mb-2">Source context (match highlighted):</div>
              <pre className="bg-zinc-950 border border-zinc-800 rounded-lg p-4 font-mono text-sm text-zinc-300 whitespace-pre-wrap break-all leading-relaxed">
                <HighlightedSource
                  source_text={finding.source_text!}
                  match_offset={finding.match_offset!}
                  match_length={finding.match_length!}
                />
              </pre>
            </div>
          ) : finding.redacted_preview ? (
            <div>
              <div className="text-xs text-zinc-500 mb-2">Redacted preview:</div>
              <pre className="bg-zinc-950 border border-zinc-800 rounded-lg p-4 font-mono text-sm text-zinc-400 whitespace-pre-wrap break-all">
                {finding.redacted_preview}
              </pre>
            </div>
          ) : (
            <div className="text-center text-zinc-500 text-sm py-8">
              No source context available for this finding.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function FindingsList({ findings }: { findings: DlpFindingRow[] }) {
  const [selectedFinding, setSelectedFinding] = useState<DlpFindingRow | null>(null);

  if (!findings || findings.length === 0) return null;
  return (
    <div className="mt-2 space-y-2">
      {findings.map((f, i) => {
        const hasContext = !!(f.source_text && f.match_offset != null && f.match_length != null);
        return (
          <div
            key={i}
            className={`bg-zinc-800/60 border border-zinc-700/60 rounded-lg p-3 ${hasContext || f.redacted_preview ? "cursor-pointer hover:border-yellow-700/60 hover:bg-zinc-800/80 transition-colors" : ""}`}
            onClick={() => (hasContext || f.redacted_preview) && setSelectedFinding(f)}
            role={hasContext || f.redacted_preview ? "button" : undefined}
            tabIndex={hasContext || f.redacted_preview ? 0 : undefined}
            onKeyDown={(e) => {
              if ((e.key === "Enter" || e.key === " ") && (hasContext || f.redacted_preview)) {
                e.preventDefault();
                setSelectedFinding(f);
              }
            }}
          >
            <div className="flex items-center justify-between gap-4">
              <div className="flex items-center gap-2 min-w-0">
                <AlertTriangle className="w-4 h-4 text-yellow-400 flex-shrink-0" />
                <span className="text-sm font-medium text-yellow-300">{f.tag}</span>
                {f.finding_type && (
                  <span className="text-xs px-2 py-0.5 rounded bg-zinc-700/60 text-zinc-300 border border-zinc-600">
                    {f.finding_type.replace(/_/g, " ")}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-3 flex-shrink-0">
                {f.pattern_name && <span className="text-xs text-zinc-500">{f.pattern_name}</span>}
                {f.confidence != null && (
                  <div className="flex items-center gap-1">
                    <div className="w-16 h-1.5 bg-zinc-700 rounded-full overflow-hidden">
                      <div
                        className={`h-full rounded-full ${f.confidence >= 0.8 ? "bg-red-400" : f.confidence >= 0.5 ? "bg-yellow-400" : "bg-zinc-400"}`}
                        style={{ width: `${Math.round(f.confidence * 100)}%` }}
                      />
                    </div>
                    <span className="text-xs text-zinc-500">{Math.round(f.confidence * 100)}%</span>
                  </div>
                )}
                {(hasContext || f.redacted_preview) && (
                  <Eye className="w-3.5 h-3.5 text-zinc-500" />
                )}
              </div>
            </div>
            {f.redacted_preview && (
              <div className="mt-2 px-3 py-2 bg-zinc-900/80 rounded border border-zinc-700/40 font-mono text-xs text-zinc-400 break-all">
                {f.redacted_preview}
              </div>
            )}
          </div>
        );
      })}
      {selectedFinding && (
        <FindingContextModal finding={selectedFinding} onClose={() => setSelectedFinding(null)} />
      )}
    </div>
  );
}

function TaskGroup({ group }: { group: TaskSecurityGroup }) {
  const [open, setOpen] = useState(false);
  const hasFindings = group.finding_count > 0;
  const hasDanger = Object.keys(group.threat_levels).some(
    (k) => k === "dangerous" || k === "critical"
  );

  return (
    <div className={`bg-zinc-900 border rounded-lg overflow-hidden ${
      hasDanger ? "border-red-800/60" : hasFindings ? "border-yellow-800/60" : "border-zinc-800"
    }`}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center gap-3 px-4 py-3 hover:bg-zinc-800/40 transition-colors text-left"
      >
        {open ? (
          <ChevronDown className="w-4 h-4 text-zinc-400 flex-shrink-0" />
        ) : (
          <ChevronRight className="w-4 h-4 text-zinc-400 flex-shrink-0" />
        )}

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-mono text-xs text-zinc-500">
              {group.task_id ? group.task_id.slice(0, 8) + "\u2026" : "system"}
            </span>
            {group.task_status && (
              <span className={`text-xs px-1.5 py-0.5 rounded ${
                group.task_status === "Running" ? "bg-blue-900/40 text-blue-400" :
                group.task_status === "Complete" ? "bg-green-900/40 text-green-400" :
                "bg-zinc-800 text-zinc-400"
              }`}>
                {group.task_status}
              </span>
            )}
          </div>
          <div className="text-sm text-zinc-200 truncate mt-0.5">
            {truncate(group.task_prompt) || "No prompt"}
          </div>
        </div>

        <div className="flex items-center gap-3 flex-shrink-0">
          <div className="flex items-center gap-1 text-xs text-zinc-400">
            <Activity className="w-3 h-3" />
            {group.event_count}
          </div>
          {group.finding_count > 0 && (
            <div className="flex items-center gap-1 text-xs text-yellow-400">
              <FileSearch className="w-3 h-3" />
              {group.finding_count} finding{group.finding_count !== 1 ? "s" : ""}
            </div>
          )}
          {hasDanger && (
            <ShieldX className="w-4 h-4 text-red-400" />
          )}
        </div>
      </button>

      {open && (
        <div className="border-t border-zinc-800 px-4 py-2 space-y-1 max-h-96 overflow-y-auto">
          {group.events.map((ev: SecurityEvent) => (
            <div key={ev.id} className="flex items-start gap-3 py-1.5 border-b border-zinc-800/50 last:border-0">
              <span className="text-xs text-zinc-500 whitespace-nowrap w-14 flex-shrink-0 pt-0.5">
                {timeAgo(ev.created_at)}
              </span>
              <span className="font-mono text-xs text-zinc-300 w-40 truncate flex-shrink-0 pt-0.5" title={ev.tool_name ?? ""}>
                {ev.tool_name ?? "\u2014"}
              </span>
              <div className="flex items-center gap-2 flex-shrink-0">
                <EventTypeBadge type={ev.event_type} />
                <ThreatBadge level={ev.threat_level} />
                <ActionBadge action={ev.action_taken} />
              </div>
              {ev.direction && (
                <span className="text-xs text-zinc-500 flex-shrink-0 pt-0.5">
                  {ev.direction === "inbound" ? "\u2192 in" : "\u2190 out"}
                </span>
              )}
              <div className="flex-1 min-w-0">
                <FindingsList findings={ev.findings} />
              </div>
            </div>
          ))}
          {group.events.length === 0 && (
            <div className="text-center text-zinc-500 text-xs py-4">No events</div>
          )}
        </div>
      )}
    </div>
  );
}

function SummaryCard({
  label,
  value,
  accent,
  icon: Icon,
}: {
  label: string;
  value: number;
  accent?: "red" | "yellow" | "green";
  icon: React.ElementType;
}) {
  const accentCls =
    accent === "red" && value > 0
      ? "text-red-400"
      : accent === "yellow" && value > 0
        ? "text-yellow-400"
        : accent === "green"
          ? "text-green-400"
          : "text-zinc-100";

  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-4 flex items-center gap-3">
      <Icon className={`w-6 h-6 ${accentCls} flex-shrink-0`} />
      <div>
        <div className={`text-2xl font-bold ${accentCls}`}>{value.toLocaleString()}</div>
        <div className="text-xs text-zinc-400">{label}</div>
      </div>
    </div>
  );
}

const PERIOD_OPTIONS = [
  { label: "1h", value: "1h" },
  { label: "24h", value: "24h" },
  { label: "7d", value: "7d" },
  { label: "30d", value: "30d" },
];

export function SecurityEventsPage() {
  const [period, setPeriod] = useState("24h");

  const { data: summary } = useQuery({
    queryKey: ["security-summary", period],
    queryFn: () => api.security.summary(period),
    retry: false,
  });

  const { data: grouped = [] } = useQuery({
    queryKey: ["security-events-grouped", period],
    queryFn: () => api.security.eventsGrouped(period),
    refetchInterval: 10_000,
    retry: false,
  });

  const totalEvents = summary?.total_events ?? 0;
  const blocked = summary?.blocked ?? 0;
  const warned = summary?.warned ?? 0;
  const totalFindings = grouped.reduce((sum, g) => sum + g.finding_count, 0);

  return (
    <div className="flex flex-col gap-6 p-6 max-w-7xl mx-auto w-full">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold text-zinc-100 flex items-center gap-2">
          <Activity className="w-6 h-6 text-primary" />
          Audit Log
        </h1>
        <p className="text-zinc-400 text-sm mt-1">Security events and DLP findings across all agent tasks</p>
      </div>

      <SecurityStatusBar />

      {/* Period selector + Summary Cards */}
      <div>
        <div className="flex items-center gap-1 mb-3">
          {PERIOD_OPTIONS.map((opt) => (
            <button
              key={opt.value}
              type="button"
              onClick={() => setPeriod(opt.value)}
              className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
                period === opt.value
                  ? "bg-primary text-primary-foreground"
                  : "text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800"
              }`}
            >
              {opt.label}
            </button>
          ))}
        </div>
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          <SummaryCard label="Total Events" value={totalEvents} icon={Shield} />
          <SummaryCard label="Blocked" value={blocked} accent="red" icon={ShieldX} />
          <SummaryCard label="Warnings" value={warned} accent="yellow" icon={ShieldAlert} />
          <SummaryCard label="DLP Findings" value={totalFindings} accent="yellow" icon={FileSearch} />
        </div>
      </div>

      {/* Events Grouped by Task */}
      <div>
        <h2 className="text-zinc-100 font-semibold mb-3 text-sm flex items-center gap-2">
          <ShieldCheck className="w-4 h-4 text-zinc-400" />
          Security Events by Task
          <span className="ml-auto text-zinc-500 text-xs font-normal">
            {grouped.length} task{grouped.length !== 1 ? "s" : ""} &middot; Auto-refreshes every 10s
          </span>
        </h2>
        {grouped.length === 0 ? (
          <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-8 text-center text-zinc-500 text-sm">
            No security events recorded in this period.
          </div>
        ) : (
          <div className="space-y-2">
            {grouped.map((group) => (
              <TaskGroup key={group.task_id ?? "system"} group={group} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
