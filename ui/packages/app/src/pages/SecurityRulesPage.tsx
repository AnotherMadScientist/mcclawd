import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  ShieldAlert,
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
  Lock,
  BookOpen,
  Shield,
  Activity,
} from "lucide-react";
import { api } from "../api/client";
import type { DlpPolicy, DlpPatternInfo } from "../api/types";

// --- Action badge (shared display) ---

function ActionBadge({ action }: { action: string }) {
  const map: Record<string, string> = {
    allow: "bg-green-900/60 text-green-300 border-green-700",
    allowed: "bg-green-900/60 text-green-300 border-green-700",
    warn: "bg-yellow-900/60 text-yellow-300 border-yellow-700",
    warned: "bg-yellow-900/60 text-yellow-300 border-yellow-700",
    block: "bg-red-900/60 text-red-300 border-red-700",
    blocked: "bg-red-900/60 text-red-300 border-red-700",
    redact: "bg-blue-900/60 text-blue-300 border-blue-700",
    redacted: "bg-blue-900/60 text-blue-300 border-blue-700",
  };
  const cls = map[action] ?? "bg-zinc-800 text-zinc-300 border-zinc-700";
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs border font-medium ${cls}`}>
      {action}
    </span>
  );
}

// --- Detection Patterns (read-only, from DlpHook) ---

function PatternCategoryGroup({
  category,
  patterns,
}: {
  category: string;
  patterns: DlpPatternInfo[];
}) {
  const [open, setOpen] = useState(true);

  return (
    <div className="border border-zinc-800 rounded-lg overflow-hidden">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center gap-3 px-4 py-2.5 bg-zinc-900 hover:bg-zinc-800/60 transition-colors text-left"
      >
        {open ? (
          <ChevronDown className="w-3.5 h-3.5 text-zinc-400 flex-shrink-0" />
        ) : (
          <ChevronRight className="w-3.5 h-3.5 text-zinc-400 flex-shrink-0" />
        )}
        <span className="text-sm font-medium text-zinc-100 capitalize">
          {category.replace(/_/g, " ")}
        </span>
        <span className="ml-auto text-xs text-zinc-500">
          {patterns.length} pattern{patterns.length !== 1 ? "s" : ""}
        </span>
      </button>

      {open && (
        <div className="border-t border-zinc-800">
          <table className="w-full text-sm table-fixed">
            <colgroup>
              <col style={{ width: "75%" }} />
              <col style={{ width: "25%" }} />
            </colgroup>
            <tbody className="divide-y divide-zinc-800/50">
              {patterns.map((p, i) => (
                <tr key={i} className="hover:bg-zinc-800/20 transition-colors">
                  <td className="px-4 py-2 text-zinc-100 font-medium truncate">{p.name}</td>
                  <td className="px-4 py-2 text-right">
                    <ActionBadge action={p.action} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function DetectionPatternsSection() {
  const { data: patterns = [], isLoading, isError } = useQuery({
    queryKey: ["security-patterns"],
    queryFn: () => api.security.patterns(),
    retry: false,
  });

  // Group by category
  const grouped = patterns.reduce<Record<string, DlpPatternInfo[]>>((acc, p) => {
    const cat = p.category || "other";
    if (!acc[cat]) acc[cat] = [];
    acc[cat].push(p);
    return acc;
  }, {});

  const categories = Object.keys(grouped).sort();

  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
      <div className="mb-4">
        <h2 className="text-zinc-100 font-semibold text-sm flex items-center gap-2">
          <Lock className="w-4 h-4 text-zinc-400" />
          Detection Patterns
        </h2>
        <p className="text-zinc-500 text-xs mt-1">
          Built-in regex patterns from DlpHook &mdash; scans user prompts, tool arguments, tool results, and LLM responses
        </p>
      </div>

      {isLoading && (
        <div className="py-6 text-center text-zinc-500 text-sm">Loading patterns&hellip;</div>
      )}

      {isError && (
        <div className="py-6 text-center text-zinc-500 text-sm">
          Could not load patterns. The <code className="font-mono text-xs">/api/security/patterns</code> endpoint may not be available yet.
        </div>
      )}

      {!isLoading && !isError && categories.length === 0 && (
        <div className="py-6 text-center text-zinc-500 text-sm">No detection patterns found.</div>
      )}

      {!isLoading && !isError && categories.length > 0 && (
        <div className="space-y-2">
          {categories.map((cat) => (
            <PatternCategoryGroup key={cat} category={cat} patterns={grouped[cat] ?? []} />
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
                {p.description ?? "\u2014"}
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

// --- Response Rules Section ---

function ResponseRulesSection() {
  const [showAddPolicy, setShowAddPolicy] = useState(false);
  const queryClient = useQueryClient();

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

  return (
    <>
      <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="text-zinc-100 font-semibold text-sm flex items-center gap-2">
              <ShieldAlert className="w-4 h-4 text-zinc-400" />
              Response Rules
            </h2>
            <p className="text-zinc-500 text-xs mt-0.5">
              User-configurable policies that determine what action (allow / warn / block / redact) to take when a detection pattern matches
            </p>
          </div>
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
    </>
  );
}

// --- Main Export ---

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
        <span className={`w-2 h-2 rounded-full ${status == null ? "bg-zinc-600" : status.sidecar_healthy ? "bg-green-400" : "bg-red-400"}`} />
        <span className="text-zinc-400">Sidecar:</span>
        <span className="text-zinc-100 font-medium">{status == null ? "\u2014" : status.sidecar_healthy ? "Healthy" : "Down"}</span>
      </div>
    </div>
  );
}

export function SecurityRulesPage() {
  return (
    <div className="flex flex-col gap-6 p-6 max-w-7xl mx-auto w-full">
      <div>
        <h1 className="text-2xl font-bold text-zinc-100 flex items-center gap-2">
          <BookOpen className="w-6 h-6 text-primary" />
          Detection &amp; Rules
        </h1>
        <p className="text-zinc-400 text-sm mt-1">Built-in detection patterns and response policies</p>
      </div>

      <SecurityStatusBar />

      <DetectionPatternsSection />
      <ResponseRulesSection />
    </div>
  );
}
