import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Shield,
  ShieldAlert,
  ShieldCheck,
  ShieldX,
  Activity,
  Trash2,
  Plus,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import { api } from "../api/client";
import type { DlpPolicy, DlpFindingRow, SecurityEvent } from "../api/types";

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

function EventTypeBadge({ type }: { type: string }) {
  const map: Record<string, string> = {
    dlp_match: "bg-purple-900/60 text-purple-300 border-purple-700",
    secret_detected: "bg-red-900/60 text-red-300 border-red-700",
    injection_attempt: "bg-orange-900/60 text-orange-300 border-orange-700",
    pii_detected: "bg-blue-900/60 text-blue-300 border-blue-700",
    flow_violation: "bg-pink-900/60 text-pink-300 border-pink-700",
    tool_blocked: "bg-yellow-900/60 text-yellow-300 border-yellow-700",
  };
  const cls = map[type] ?? "bg-zinc-800 text-zinc-300 border-zinc-700";
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs border font-medium ${cls}`}>
      {type.replace(/_/g, " ")}
    </span>
  );
}

function ThreatBadge({ level }: { level: string | null }) {
  if (!level) return <span className="text-zinc-500 text-xs">—</span>;
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

function FindingsCell({ findings }: { findings: DlpFindingRow[] }) {
  const [open, setOpen] = useState(false);
  if (!findings || findings.length === 0) {
    return <span className="text-zinc-500 text-xs">—</span>;
  }
  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1 text-xs text-zinc-300 hover:text-zinc-100 transition-colors"
      >
        {open ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
        {findings.length} finding{findings.length !== 1 ? "s" : ""}
      </button>
      {open && (
        <div className="mt-1 space-y-1">
          {findings.map((f, i) => (
            <div key={i} className="text-xs bg-zinc-800 rounded px-2 py-1 border border-zinc-700">
              <span className="text-zinc-300 font-medium">{f.tag}</span>
              {f.pattern_name && (
                <span className="text-zinc-500 ml-1">({f.pattern_name})</span>
              )}
              {f.confidence != null && (
                <span className="text-zinc-500 ml-1">{Math.round(f.confidence * 100)}%</span>
              )}
              {f.redacted_preview && (
                <div className="text-zinc-400 mt-0.5 font-mono">{f.redacted_preview}</div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// --- Add Policy Dialog ---

const EMPTY_POLICY: Omit<DlpPolicy, "id" | "updated_at"> = {
  name: "",
  description: "",
  tag_pattern: "*",
  tool_pattern: "*",
  action: "warn",
  enabled: true,
};

function AddPolicyDialog({
  onClose,
  onSave,
}: {
  onClose: () => void;
  onSave: (policy: Omit<DlpPolicy, "id" | "updated_at">) => void;
}) {
  const [form, setForm] = useState(EMPTY_POLICY);

  function set(field: keyof typeof EMPTY_POLICY, value: string | boolean) {
    setForm((f) => ({ ...f, [field]: value }));
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-zinc-900 border border-zinc-700 rounded-lg p-6 w-full max-w-lg shadow-2xl">
        <h3 className="text-zinc-100 font-semibold text-lg mb-4">Add DLP Policy</h3>
        <div className="space-y-3">
          <div>
            <label className="block text-xs text-zinc-400 mb-1">Name</label>
            <input
              className="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-100 focus:outline-none focus:border-zinc-500"
              value={form.name}
              onChange={(e) => set("name", e.target.value)}
              placeholder="e.g. block-api-keys"
            />
          </div>
          <div>
            <label className="block text-xs text-zinc-400 mb-1">Description</label>
            <input
              className="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-100 focus:outline-none focus:border-zinc-500"
              value={form.description ?? ""}
              onChange={(e) => set("description", e.target.value)}
              placeholder="Optional description"
            />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs text-zinc-400 mb-1">Tag Pattern</label>
              <input
                className="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-100 focus:outline-none focus:border-zinc-500"
                value={form.tag_pattern}
                onChange={(e) => set("tag_pattern", e.target.value)}
                placeholder="e.g. SECRET_*"
              />
            </div>
            <div>
              <label className="block text-xs text-zinc-400 mb-1">Tool Pattern</label>
              <input
                className="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-100 focus:outline-none focus:border-zinc-500"
                value={form.tool_pattern}
                onChange={(e) => set("tool_pattern", e.target.value)}
                placeholder="e.g. * or bash"
              />
            </div>
          </div>
          <div>
            <label className="block text-xs text-zinc-400 mb-1">Action</label>
            <select
              className="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-100 focus:outline-none focus:border-zinc-500"
              value={form.action}
              onChange={(e) => set("action", e.target.value)}
            >
              <option value="allow">Allow</option>
              <option value="warn">Warn</option>
              <option value="block">Block</option>
              <option value="redact">Redact</option>
            </select>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="policy-enabled"
              checked={form.enabled}
              onChange={(e) => set("enabled", e.target.checked)}
              className="accent-primary"
            />
            <label htmlFor="policy-enabled" className="text-sm text-zinc-300">
              Enabled
            </label>
          </div>
        </div>
        <div className="flex justify-end gap-2 mt-5">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 text-sm text-zinc-400 hover:text-zinc-100 transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => onSave(form)}
            disabled={!form.name.trim()}
            className="px-4 py-2 text-sm bg-primary text-primary-foreground rounded font-medium hover:opacity-90 transition-opacity disabled:opacity-40"
          >
            Create Policy
          </button>
        </div>
      </div>
    </div>
  );
}

// --- Summary Card ---

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

// --- Events Table ---

function EventsTable({ events }: { events: SecurityEvent[] }) {
  if (events.length === 0) {
    return (
      <div className="text-center py-10 text-zinc-500 text-sm">
        No security events recorded.
      </div>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-zinc-800">
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Time</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Task</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Tool</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Type</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Threat</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Action</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2">Findings</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/50">
          {events.map((ev) => (
            <tr key={ev.id} className="hover:bg-zinc-800/30 transition-colors">
              <td className="py-2 pr-4 text-zinc-500 text-xs whitespace-nowrap">
                {timeAgo(ev.created_at)}
              </td>
              <td className="py-2 pr-4">
                {ev.task_id ? (
                  <span className="font-mono text-xs text-zinc-400">{ev.task_id.slice(0, 8)}…</span>
                ) : (
                  <span className="text-zinc-600 text-xs">system</span>
                )}
              </td>
              <td className="py-2 pr-4">
                {ev.tool_name ? (
                  <span className="font-mono text-xs text-zinc-300">{ev.tool_name}</span>
                ) : (
                  <span className="text-zinc-600 text-xs">—</span>
                )}
              </td>
              <td className="py-2 pr-4">
                <EventTypeBadge type={ev.event_type} />
              </td>
              <td className="py-2 pr-4">
                <ThreatBadge level={ev.threat_level} />
              </td>
              <td className="py-2 pr-4">
                <ActionBadge action={ev.action_taken} />
              </td>
              <td className="py-2">
                <FindingsCell findings={ev.findings} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// --- Policies Table ---

function PoliciesTable({
  policies,
  onDelete,
}: {
  policies: DlpPolicy[];
  onDelete: (id: number) => void;
}) {
  if (policies.length === 0) {
    return (
      <div className="text-center py-8 text-zinc-500 text-sm">
        No DLP policies configured.
      </div>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-zinc-800">
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Name</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Description</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Tag Pattern</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Tool Pattern</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Action</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2 pr-4">Enabled</th>
            <th className="text-left text-xs text-zinc-400 font-medium pb-2"></th>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/50">
          {policies.map((p) => (
            <tr key={p.id} className="hover:bg-zinc-800/30 transition-colors">
              <td className="py-2 pr-4 text-zinc-100 font-medium">{p.name}</td>
              <td className="py-2 pr-4 text-zinc-400 text-xs max-w-xs truncate">
                {p.description ?? "—"}
              </td>
              <td className="py-2 pr-4 font-mono text-xs text-zinc-300">{p.tag_pattern}</td>
              <td className="py-2 pr-4 font-mono text-xs text-zinc-300">{p.tool_pattern}</td>
              <td className="py-2 pr-4">
                <ActionBadge action={p.action} />
              </td>
              <td className="py-2 pr-4">
                <span
                  className={`inline-block w-2 h-2 rounded-full ${p.enabled ? "bg-green-400" : "bg-zinc-600"}`}
                />
              </td>
              <td className="py-2">
                <button
                  type="button"
                  onClick={() => onDelete(p.id)}
                  className="p-1 text-zinc-500 hover:text-red-400 transition-colors rounded"
                  title="Delete policy"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// --- Main Page ---

const PERIOD_OPTIONS = [
  { label: "1h", value: "1h" },
  { label: "24h", value: "24h" },
  { label: "7d", value: "7d" },
  { label: "30d", value: "30d" },
];

export function SecurityPage() {
  const [period, setPeriod] = useState("24h");
  const [showAddPolicy, setShowAddPolicy] = useState(false);
  const queryClient = useQueryClient();

  const { data: status } = useQuery({
    queryKey: ["security-status"],
    queryFn: () => api.security.status(),
    retry: false,
  });

  const { data: summary } = useQuery({
    queryKey: ["security-summary", period],
    queryFn: () => api.security.summary(period),
    retry: false,
  });

  const { data: events = [] } = useQuery({
    queryKey: ["security-events"],
    queryFn: () => api.security.events(),
    refetchInterval: 10_000,
    retry: false,
  });

  const { data: policies = [] } = useQuery({
    queryKey: ["security-policies"],
    queryFn: () => api.security.policies(),
    retry: false,
  });

  const createPolicyMutation = useMutation({
    mutationFn: (policy: Omit<DlpPolicy, "id" | "updated_at">) =>
      api.security.createPolicy(policy),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["security-policies"] });
      setShowAddPolicy(false);
    },
  });

  const deletePolicyMutation = useMutation({
    mutationFn: (id: number) => api.security.deletePolicy(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["security-policies"] });
    },
  });

  const totalEvents = summary?.total_events ?? 0;
  const blocked = summary?.blocked ?? 0;
  const warned = summary?.warned ?? 0;
  const clean = Math.max(0, totalEvents - blocked - warned);

  return (
    <div className="flex flex-col gap-6 p-6 max-w-7xl mx-auto w-full">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-zinc-100 flex items-center gap-2">
            <Shield className="w-6 h-6 text-primary" />
            Security
          </h1>
          <p className="text-zinc-400 text-sm mt-1">
            Agent security pipeline monitoring &amp; DLP policies
          </p>
        </div>
      </div>

      {/* Status Bar */}
      <div className="bg-zinc-900 border border-zinc-800 rounded-lg px-4 py-3 flex items-center gap-6 text-sm">
        <div className="flex items-center gap-2">
          <Activity className="w-4 h-4 text-zinc-400" />
          <span className="text-zinc-400">Pipeline hooks:</span>
          <span className="text-zinc-100 font-medium">
            {status ? status.pipeline_hooks : "—"}
          </span>
        </div>
        <div className="w-px h-4 bg-zinc-700" />
        <div className="flex items-center gap-2">
          <span
            className={`w-2 h-2 rounded-full ${
              status == null
                ? "bg-zinc-600"
                : status.sidecar_healthy
                  ? "bg-green-400"
                  : "bg-red-400"
            }`}
          />
          <span className="text-zinc-400">Sidecar:</span>
          <span className="text-zinc-100 font-medium">
            {status == null ? "—" : status.sidecar_healthy ? "Healthy" : "Down"}
          </span>
          {status && (
            <span className="text-zinc-500 font-mono text-xs">{status.sidecar_url}</span>
          )}
        </div>
      </div>

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
          <SummaryCard label="Clean Scans" value={clean} accent="green" icon={ShieldCheck} />
        </div>
      </div>

      {/* Recent Events */}
      <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
        <h2 className="text-zinc-100 font-semibold mb-4 text-sm flex items-center gap-2">
          <Activity className="w-4 h-4 text-zinc-400" />
          Recent Events
          <span className="ml-auto text-zinc-500 text-xs font-normal">
            Auto-refreshes every 10s
          </span>
        </h2>
        <EventsTable events={events} />
      </div>

      {/* DLP Policies */}
      <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-zinc-100 font-semibold text-sm flex items-center gap-2">
            <ShieldAlert className="w-4 h-4 text-zinc-400" />
            DLP Policies
          </h2>
          <button
            type="button"
            onClick={() => setShowAddPolicy(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 bg-primary text-primary-foreground text-xs font-medium rounded hover:opacity-90 transition-opacity"
          >
            <Plus className="w-3.5 h-3.5" />
            Add Policy
          </button>
        </div>
        <PoliciesTable
          policies={policies}
          onDelete={(id) => deletePolicyMutation.mutate(id)}
        />
      </div>

      {showAddPolicy && (
        <AddPolicyDialog
          onClose={() => setShowAddPolicy(false)}
          onSave={(policy) => createPolicyMutation.mutate(policy)}
        />
      )}
    </div>
  );
}
