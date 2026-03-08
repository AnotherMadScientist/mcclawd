import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Pencil, Check, X } from "lucide-react";
import { api } from "../api/client";
import { FieldSkeleton } from "../components/LoadingSkeleton";
import { ErrorState } from "../components/ErrorState";
import type { AnthropicModel, ModelPricing } from "../api/types";

const FALLBACK_MODELS = ["claude-sonnet-4-6-20250514", "claude-opus-4-6-20250514", "claude-haiku-4-5-20251001"];

export function SettingsPage() {
  const {
    data: config,
    isLoading,
    isError,
    refetch,
  } = useQuery({
    queryKey: ["config"],
    queryFn: api.config.get,
  });

  const { data: liveModels } = useQuery({
    queryKey: ["providers", "models"],
    queryFn: api.providers.models,
    staleTime: 3600_000,
    retry: 1,
  });

  const { data: pricing } = useQuery({
    queryKey: ["providers", "pricing"],
    queryFn: api.providers.pricing,
    staleTime: 3600_000,
  });

  const pricingMap = new Map<string, ModelPricing>();
  if (pricing) {
    for (const p of pricing) {
      pricingMap.set(p.model_id, p);
    }
  }

  if (isLoading) {
    return (
      <div className="max-w-2xl mx-auto space-y-6">
        <h1 className="text-2xl font-bold">Settings</h1>
        <div className="space-y-4">
          {Array.from({ length: 5 }, (_, i) => (
            <FieldSkeleton key={i} />
          ))}
        </div>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="max-w-2xl mx-auto space-y-6">
        <h1 className="text-2xl font-bold">Settings</h1>
        <ErrorState message="Failed to load settings" onRetry={() => refetch()} />
      </div>
    );
  }

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Settings</h1>

      <div className="space-y-4" data-testid="settings-fields">
        <ModelSelector
          value={config?.agent.model}
          models={liveModels ?? []}
          fallbackModels={FALLBACK_MODELS}
          pricingMap={pricingMap}
        />
        <EditableField
          label="Max Turns"
          value={config?.agent.max_turns?.toString()}
          type="number"
          fieldKey="max_turns"
        />
        <EditableField
          label="Default Workspace"
          value={config?.agent.default_workspace}
          type="text"
          fieldKey="default_workspace"
        />
        <ToolProfileSelector value={config?.agent.default_tool_profile} />
        <Field label="Data Directory" value={config?.data_dir} />
        <Field label="AgentGateway URL" value={config?.mcp.agentgateway_url} />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Model Selector (live models from Anthropic API + pricing)
// ---------------------------------------------------------------------------

function ModelSelector({
  value,
  models,
  fallbackModels,
  pricingMap,
}: {
  value?: string;
  models: AnthropicModel[];
  fallbackModels: string[];
  pricingMap: Map<string, ModelPricing>;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const queryClient = useQueryClient();

  const options = models.length > 0 ? models.map((m) => m.id) : fallbackModels;
  const displayNameMap = new Map<string, string>();
  for (const m of models) {
    displayNameMap.set(m.id, m.display_name);
  }

  const mutation = useMutation({
    mutationFn: (val: string) => api.config.update({ model: val }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["config"] });
      setEditing(false);
      setToast({ msg: "Model updated", ok: true });
      setTimeout(() => setToast(null), 2500);
    },
    onError: () => {
      setEditing(false);
      setToast({ msg: "Failed to update model", ok: false });
      setTimeout(() => setToast(null), 2500);
    },
  });

  const startEdit = () => {
    setDraft(value || options[0] || "");
    setEditing(true);
  };

  const selectedPricing = pricingMap.get(draft || value || "");

  return (
    <div className="p-4 rounded-xl bg-card border border-border relative" data-testid="model-card">
      <div className="flex items-center justify-between mb-1">
        <label className="text-xs text-muted-foreground">Model</label>
        {!editing && (
          <button
            aria-label="Edit Model"
            onClick={startEdit}
            className="p-1 rounded hover:bg-muted transition-colors"
          >
            <Pencil className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        )}
      </div>

      {!editing ? (
        <div>
          <p className="text-sm font-mono">{value || "\u2014"}</p>
          {value && pricingMap.has(value) && (
            <p className="text-xs text-muted-foreground mt-1">
              ${pricingMap.get(value)!.input_price_per_mtok}/MTok in &middot; ${pricingMap.get(value)!.output_price_per_mtok}/MTok out
            </p>
          )}
        </div>
      ) : (
        <div className="space-y-2 mt-1">
          <div className="flex items-center gap-2">
            <select
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              className="flex-1 text-sm font-mono bg-background border border-border rounded px-2 py-1 focus:outline-none focus:ring-2 focus:ring-primary/30"
              autoFocus
            >
              {options.map((id) => {
                const name = displayNameMap.get(id);
                const price = pricingMap.get(id);
                const label = name
                  ? `${name}${price ? ` — $${price.input_price_per_mtok}/$${price.output_price_per_mtok} per MTok` : ""}`
                  : id;
                return (
                  <option key={id} value={id}>{label}</option>
                );
              })}
            </select>
            <button
              aria-label="Save"
              onClick={() => mutation.mutate(draft)}
              disabled={mutation.isPending}
              className="p-1 rounded hover:bg-muted text-emerald-500 disabled:opacity-50"
            >
              <Check className="w-4 h-4" />
            </button>
            <button
              aria-label="Cancel"
              onClick={() => setEditing(false)}
              className="p-1 rounded hover:bg-muted text-muted-foreground"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
          {selectedPricing && (
            <p className="text-xs text-muted-foreground">
              Input: ${selectedPricing.input_price_per_mtok}/MTok &middot; Output: ${selectedPricing.output_price_per_mtok}/MTok
            </p>
          )}
        </div>
      )}

      {toast && (
        <p className={`text-xs mt-2 ${toast.ok ? "text-emerald-500" : "text-destructive"}`}>
          {toast.msg}
        </p>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Existing field components
// ---------------------------------------------------------------------------

const TOOL_PROFILES = [
  { value: "Minimal", label: "Minimal - memory tools only" },
  { value: "Coding", label: "Coding - filesystem, git, shell" },
  { value: "Research", label: "Research - web, fetch, browser" },
  { value: "Full", label: "Full - all available tools" },
];

function ToolProfileSelector({ value }: { value?: string }) {
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const queryClient = useQueryClient();
  const mutation = useMutation({
    mutationFn: (profile: string) => api.config.update({ default_tool_profile: profile }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["config"] });
      setToast({ msg: "Saved", ok: true });
      setTimeout(() => setToast(null), 2000);
    },
    onError: () => setToast({ msg: "Failed to save", ok: false }),
  });

  return (
    <div className="p-4 rounded-xl bg-card border border-border">
      <label className="text-xs text-muted-foreground">Default Tool Profile</label>
      <select
        value={value || "Coding"}
        onChange={(e) => mutation.mutate(e.target.value)}
        className="w-full text-sm font-mono mt-1 bg-background border border-border rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-primary/30"
        aria-label="Default Tool Profile"
      >
        {TOOL_PROFILES.map((p) => (
          <option key={p.value} value={p.value}>{p.label}</option>
        ))}
      </select>
      {toast && (
        <p className={`text-xs mt-1 ${toast.ok ? "text-emerald-500" : "text-destructive"}`}>
          {toast.msg}
        </p>
      )}
    </div>
  );
}

function Field({ label, value }: { label: string; value?: string }) {
  return (
    <div className="p-4 rounded-xl bg-card border border-border">
      <label className="text-xs text-muted-foreground">{label}</label>
      <p className="text-sm font-mono mt-1">{value || "\u2014"}</p>
    </div>
  );
}

interface EditableFieldProps {
  label: string;
  value?: string;
  type: "text" | "number" | "select";
  options?: string[];
  fieldKey: "model" | "max_turns" | "default_workspace";
}

function EditableField({ label, value, type, options, fieldKey }: EditableFieldProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const queryClient = useQueryClient();

  const mutation = useMutation({
    mutationFn: (val: string) => {
      if (fieldKey === "max_turns") {
        return api.config.update({ max_turns: parseInt(val, 10) });
      } else if (fieldKey === "model") {
        return api.config.update({ model: val });
      } else {
        return api.config.update({ default_workspace: val });
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["config"] });
      setEditing(false);
      setToast({ msg: `${label} updated`, ok: true });
      setTimeout(() => setToast(null), 2500);
    },
    onError: () => {
      setEditing(false);
      setToast({ msg: `Failed to update ${label}`, ok: false });
      setTimeout(() => setToast(null), 2500);
    },
  });

  const startEdit = () => {
    setDraft(value || "");
    setEditing(true);
  };

  const save = () => {
    if (fieldKey === "max_turns") {
      const n = parseInt(draft, 10);
      if (!n || n < 1 || n > 100) {
        setToast({ msg: "Max Turns must be between 1 and 100", ok: false });
        setTimeout(() => setToast(null), 2500);
        return;
      }
    }
    if (!draft.trim()) {
      setToast({ msg: `${label} cannot be empty`, ok: false });
      setTimeout(() => setToast(null), 2500);
      return;
    }
    mutation.mutate(draft);
  };

  return (
    <div className="p-4 rounded-xl bg-card border border-border relative">
      <div className="flex items-center justify-between mb-1">
        <label className="text-xs text-muted-foreground">{label}</label>
        {!editing && (
          <button
            aria-label={`Edit ${label}`}
            onClick={startEdit}
            className="p-1 rounded hover:bg-muted transition-colors"
          >
            <Pencil className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        )}
      </div>

      {!editing ? (
        <p className="text-sm font-mono">{value || "\u2014"}</p>
      ) : (
        <div className="flex items-center gap-2 mt-1">
          {type === "select" ? (
            <select
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              className="flex-1 text-sm font-mono bg-background border border-border rounded px-2 py-1 focus:outline-none focus:ring-2 focus:ring-primary/30"
              autoFocus
            >
              {(options || []).map((o) => (
                <option key={o} value={o}>{o}</option>
              ))}
            </select>
          ) : (
            <input
              type={type}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              min={type === "number" ? 1 : undefined}
              max={type === "number" ? 100 : undefined}
              onKeyDown={(e) => { if (e.key === "Enter") save(); if (e.key === "Escape") setEditing(false); }}
              className="flex-1 text-sm font-mono bg-background border border-border rounded px-2 py-1 focus:outline-none focus:ring-2 focus:ring-primary/30"
              autoFocus
            />
          )}
          <button
            aria-label="Save"
            onClick={save}
            disabled={mutation.isPending}
            className="p-1 rounded hover:bg-muted text-emerald-500 disabled:opacity-50"
          >
            <Check className="w-4 h-4" />
          </button>
          <button
            aria-label="Cancel"
            onClick={() => setEditing(false)}
            className="p-1 rounded hover:bg-muted text-muted-foreground"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      )}

      {toast && (
        <p className={`text-xs mt-2 ${toast.ok ? "text-emerald-500" : "text-destructive"}`}>
          {toast.msg}
        </p>
      )}
    </div>
  );
}
