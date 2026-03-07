import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Pencil, Check, X, AlertTriangle, Info } from "lucide-react";
import { api } from "../api/client";
import { FieldSkeleton } from "../components/LoadingSkeleton";
import { ErrorState } from "../components/ErrorState";
import type { DetailedUsageSummary, DailyUsage, BudgetInfo, AnthropicModel, ModelPricing, CreditsResponse } from "../api/types";

const FALLBACK_MODELS = ["claude-sonnet-4-6-20250514", "claude-opus-4-6-20250514", "claude-haiku-4-5-20251001"];

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`;
  return n.toString();
}

function formatCost(usd: number): string {
  return `$${usd.toFixed(2)}`;
}

function budgetColor(spent: number, limit: number | null): string {
  if (!limit || limit <= 0) return "bg-emerald-500";
  const pct = (spent / limit) * 100;
  if (pct >= 80) return "bg-red-500";
  if (pct >= 50) return "bg-amber-500";
  return "bg-emerald-500";
}

function budgetPct(spent: number, limit: number | null): number {
  if (!limit || limit <= 0) return 0;
  return Math.min((spent / limit) * 100, 100);
}

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
    staleTime: 3600_000, // 1 hour
    retry: 1,
  });

  const { data: pricing } = useQuery({
    queryKey: ["providers", "pricing"],
    queryFn: api.providers.pricing,
    staleTime: 3600_000,
  });

  // Build pricing lookup for display
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
        <Field label="Data Directory" value={config?.data_dir} />
        <Field label="AgentGateway URL" value={config?.mcp.agentgateway_url} />
      </div>

      <SpendingDashboard />
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
// Mic Provider Selector
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Spending Dashboard
// ---------------------------------------------------------------------------

type FilterPeriod = "day" | "week" | "month" | "year" | "all";

const FILTER_LABELS: { key: FilterPeriod; label: string }[] = [
  { key: "day", label: "Day" },
  { key: "week", label: "Week" },
  { key: "month", label: "Month" },
  { key: "year", label: "Year" },
  { key: "all", label: "All" },
];

function filterDays(period: FilterPeriod): number {
  if (period === "day") return 1;
  if (period === "week") return 7;
  if (period === "month") return 30;
  if (period === "year") return 365;
  return Infinity;
}

function filterHistory(history: DailyUsage[], period: FilterPeriod): DailyUsage[] {
  if (period === "all") return history;
  const days = filterDays(period);
  const cutoff = new Date();
  cutoff.setDate(cutoff.getDate() - days + 1);
  // Use local date to match backend YYYY-MM-DD format (toISOString uses UTC, can shift dates)
  const cutoffStr = `${cutoff.getFullYear()}-${String(cutoff.getMonth() + 1).padStart(2, "0")}-${String(cutoff.getDate()).padStart(2, "0")}`;
  return history.filter((d) => d.date >= cutoffStr);
}

function filteredSpend(history: DailyUsage[], period: FilterPeriod): number {
  return filterHistory(history, period).reduce((sum, d) => sum + d.cost_usd, 0);
}

function periodGranularity(period: FilterPeriod): string {
  if (period === "year" || period === "all") return "monthly";
  return "daily";
}

function granularityLabel(granularity: string): string {
  return granularity === "monthly" ? "Monthly Spend" : "Daily Spend";
}

function SpendingDashboard() {
  const [period, setPeriod] = useState<FilterPeriod>("month");
  const granularity = periodGranularity(period);

  const { data: usage } = useQuery<DetailedUsageSummary>({
    queryKey: ["providers", "usage", granularity],
    queryFn: () => api.providers.usage(granularity),
    refetchInterval: 5_000,
  });

  const { data: budget } = useQuery<BudgetInfo>({
    queryKey: ["providers", "budget"],
    queryFn: api.providers.budgetInfo,
    refetchInterval: 5_000,
  });

  const { data: credits } = useQuery<CreditsResponse>({
    queryKey: ["providers", "credits"],
    queryFn: api.providers.credits,
    refetchInterval: 15_000,
    retry: 1,
  });

  const history = usage?.daily_history ?? [];
  const filteredHistory = filterHistory(history, period);
  const periodSpend = filteredSpend(history, period);

  // Determine spend values per card based on selected period
  const todaySpend = budget?.daily_spent_usd ?? 0;
  const monthSpend = budget?.monthly_spent_usd ?? 0;
  const displaySpend = period === "day" ? todaySpend : period === "month" ? monthSpend : periodSpend;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">API Usage & Budget</h2>
        {/* Period filter pills */}
        <div className="flex gap-1">
          {FILTER_LABELS.map(({ key, label }) => (
            <button
              key={key}
              onClick={() => setPeriod(key)}
              className={`px-3 py-1 rounded-full text-xs font-medium transition-colors ${
                period === key
                  ? "bg-primary text-primary-foreground"
                  : "bg-muted text-muted-foreground hover:text-foreground"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {/* Alerts */}
      {budget?.alerts && budget.alerts.length > 0 && (
        <div className="space-y-2">
          {budget.alerts.map((alert, i) => (
            <div
              key={i}
              className="flex items-center gap-2 p-3 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-400 text-sm"
            >
              <AlertTriangle className="w-4 h-4 shrink-0" />
              {alert}
            </div>
          ))}
        </div>
      )}

      {/* Spend overview cards */}
      <div className="grid grid-cols-2 gap-4">
        <CreditsCard credits={credits} period={period} periodSpend={displaySpend} />
        <SpendCard
          label={`Usage (${FILTER_LABELS.find(f => f.key === period)?.label})`}
          spent={displaySpend}
          limit={period === "day" ? (budget?.daily_limit_usd ?? null) : period === "month" ? (budget?.monthly_limit_usd ?? null) : null}
        />
      </div>

      {/* Bar chart */}
      <UsageBarChart history={filteredHistory} period={period} granularity={granularity} />

      {/* By Model table */}
      {usage && usage.by_model.length > 0 && (
        <div className="rounded-xl bg-card border border-border overflow-hidden">
          <div className="px-4 py-3 border-b border-border">
            <h3 className="text-sm font-medium text-muted-foreground">By Model</h3>
          </div>
          <table className="w-full text-sm">
            <thead>
              <tr className="text-xs text-muted-foreground border-b border-border">
                <th className="text-left px-4 py-2 font-medium">Model</th>
                <th className="text-right px-4 py-2 font-medium">Requests</th>
                <th className="text-right px-4 py-2 font-medium">Tokens</th>
                <th className="text-right px-4 py-2 font-medium">Cost</th>
                <th className="text-right px-4 py-2 font-medium">%</th>
              </tr>
            </thead>
            <tbody>
              {usage.by_model.map((m) => {
                const pct =
                  usage.total.estimated_cost_usd > 0
                    ? ((m.estimated_cost_usd / usage.total.estimated_cost_usd) * 100).toFixed(0)
                    : "0";
                return (
                  <tr key={m.model} className="border-b border-border/50 last:border-0">
                    <td className="px-4 py-2 font-mono text-xs">{m.model}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{m.request_count}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{formatTokens(m.total_tokens)}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{formatCost(m.estimated_cost_usd)}</td>
                    <td className="px-4 py-2 text-right tabular-nums text-muted-foreground">{pct}%</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* By Task table */}
      {usage && usage.by_task.length > 0 && (
        <div className="rounded-xl bg-card border border-border overflow-hidden">
          <div className="px-4 py-3 border-b border-border">
            <h3 className="text-sm font-medium text-muted-foreground">By Task (Recent)</h3>
          </div>
          <table className="w-full text-sm">
            <thead>
              <tr className="text-xs text-muted-foreground border-b border-border">
                <th className="text-left px-4 py-2 font-medium">Task</th>
                <th className="text-left px-4 py-2 font-medium">Model</th>
                <th className="text-right px-4 py-2 font-medium">Cost</th>
              </tr>
            </thead>
            <tbody>
              {usage.by_task.slice(0, 10).map((t) => (
                <tr key={t.task_id} className="border-b border-border/50 last:border-0">
                  <td className="px-4 py-2 max-w-[200px] truncate" title={t.prompt_preview}>
                    {t.prompt_preview || t.task_id.slice(0, 8)}
                  </td>
                  <td className="px-4 py-2 font-mono text-xs">{shortModel(t.model)}</td>
                  <td className="px-4 py-2 text-right tabular-nums">{formatCost(t.estimated_cost_usd)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Empty state */}
      {(!usage || (usage.by_model.length === 0 && usage.by_task.length === 0)) && (
        <div className="p-6 rounded-xl bg-card border border-border text-center text-sm text-muted-foreground">
          No usage data yet. Run a task to see spending here.
        </div>
      )}

      {/* Budget limits */}
      <BudgetEditor budget={budget} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Usage bar chart (pure CSS/SVG, no external library)
// ---------------------------------------------------------------------------

const MONTH_NAMES = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** Always generate the full date range for the period, overlaying any existing usage data */
function fillDateGaps(history: DailyUsage[], period: FilterPeriod, granularity: string): DailyUsage[] {
  const zero: Omit<DailyUsage, "date"> = { cost_usd: 0, tokens: 0 };
  const now = new Date();

  if (granularity === "monthly") {
    const lookup = new Map(history.map((d) => [d.date, d]));
    const result: DailyUsage[] = [];
    let startMonth: Date;
    if (period === "year") {
      startMonth = new Date(now.getFullYear(), 0, 1);
    } else if (history.length > 0 && history[0]) {
      const earliest = history[0].date;
      startMonth = new Date(parseInt(earliest.slice(0, 4)), parseInt(earliest.slice(5, 7)) - 1, 1);
    } else {
      // No history — show current year
      startMonth = new Date(now.getFullYear(), 0, 1);
    }
    const cursor = new Date(startMonth);
    while (cursor <= now) {
      const key = `${cursor.getFullYear()}-${String(cursor.getMonth() + 1).padStart(2, "0")}`;
      result.push(lookup.get(key) ?? { date: key, ...zero });
      cursor.setMonth(cursor.getMonth() + 1);
    }
    return result;
  }

  // Daily: always generate the full date range for the period
  const days = period === "day" ? 1 : period === "week" ? 7 : period === "month" ? 30 : 365;
  const start = new Date();
  start.setDate(now.getDate() - days + 1);
  start.setHours(0, 0, 0, 0);

  const lookup = new Map(history.map((d) => [d.date, d]));
  const result: DailyUsage[] = [];
  const cursor = new Date(start);
  while (cursor <= now) {
    // Use local date (YYYY-MM-DD) to match backend format — toISOString() uses UTC which can shift dates
    const key = `${cursor.getFullYear()}-${String(cursor.getMonth() + 1).padStart(2, "0")}-${String(cursor.getDate()).padStart(2, "0")}`;
    result.push(lookup.get(key) ?? { date: key, ...zero });
    cursor.setDate(cursor.getDate() + 1);
  }
  return result;
}

function UsageBarChart({ history, period, granularity }: { history: DailyUsage[]; period: FilterPeriod; granularity: string }) {
  const [tooltip, setTooltip] = useState<{ date: string; cost: number; x: number } | null>(null);

  // Fill in missing dates so chart always shows continuous timeline
  const filledHistory = fillDateGaps(history, period, granularity);
  if (filledHistory.length === 0 && history.length === 0) {
    return (
      <div className="rounded-xl bg-card border border-border p-4">
        <h3 className="text-sm font-medium text-muted-foreground mb-3">{granularityLabel(granularity)}</h3>
        <p className="text-xs text-muted-foreground text-center py-6">No usage data yet</p>
      </div>
    );
  }

  const maxCost = Math.max(...filledHistory.map((d) => d.cost_usd), 0.000001);
  // Show at most 30 bars; if more, show every Nth
  const maxBars = 30;
  const step = Math.ceil(filledHistory.length / maxBars);
  const visible = filledHistory.filter((_, i) => i % step === 0 || i === filledHistory.length - 1);

  // Label frequency: show a date label every ~5 bars
  const labelEvery = Math.max(1, Math.floor(visible.length / 6));

  function shortDate(date: string): string {
    if (granularity === "monthly") {
      // date is "YYYY-MM"
      const monthIdx = parseInt(date.slice(5, 7), 10) - 1;
      return MONTH_NAMES[monthIdx] ?? date.slice(5);
    }
    const parts = date.split("-");
    const m = parts[1] ?? "";
    const d = parts[2] ?? "";
    if (period === "day") return date.slice(8);
    return `${parseInt(m)}/${parseInt(d)}`;
  }

  function tooltipLabel(date: string): string {
    if (granularity === "monthly") {
      const monthIdx = parseInt(date.slice(5, 7), 10) - 1;
      return `${MONTH_NAMES[monthIdx] ?? ""} ${date.slice(0, 4)}`;
    }
    return date;
  }

  return (
    <div className="rounded-xl bg-card border border-border p-4">
      <h3 className="text-sm font-medium text-muted-foreground mb-3">{granularityLabel(granularity)}</h3>
      <div className="relative">
        {/* Horizontal grid lines */}
        <div className="absolute inset-0 flex flex-col justify-between pointer-events-none" style={{ bottom: 20 }}>
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="border-t border-border/30 w-full" />
          ))}
        </div>

        {/* Bars container */}
        <div
          className="relative flex items-end gap-[2px] overflow-hidden"
          style={{ height: 120 }}
          onMouseLeave={() => setTooltip(null)}
        >
          {visible.map((day) => {
            const heightPct = maxCost > 0 ? (day.cost_usd / maxCost) * 100 : 0;
            const barH = Math.max(heightPct, day.cost_usd > 0 ? 2 : 0);
            return (
              <div
                key={day.date}
                className="relative flex-1 flex flex-col items-center justify-end group cursor-default"
                style={{ height: "100%" }}
                onMouseEnter={(e) => {
                  const rect = e.currentTarget.getBoundingClientRect();
                  const parent = e.currentTarget.parentElement!.getBoundingClientRect();
                  setTooltip({ date: day.date, cost: day.cost_usd, x: rect.left - parent.left + rect.width / 2 });
                }}
              >
                <div
                  className="w-full rounded-t-sm bg-primary/80 group-hover:bg-primary transition-colors"
                  style={{ height: `${barH}%` }}
                />
              </div>
            );
          })}
        </div>

        {/* Tooltip */}
        {tooltip && (
          <div
            className="absolute bottom-6 z-10 px-2 py-1 rounded bg-popover border border-border text-xs shadow-md pointer-events-none -translate-x-1/2"
            style={{ left: tooltip.x }}
          >
            <span className="text-muted-foreground">{tooltipLabel(tooltip.date)}</span>
            <span className="ml-2 font-semibold tabular-nums">{formatCost(tooltip.cost)}</span>
          </div>
        )}

        {/* X-axis date labels */}
        <div className="flex gap-[2px] mt-1" style={{ height: 20 }}>
          {visible.map((day, i) => (
            <div key={day.date} className="flex-1 text-center overflow-hidden">
              {i % labelEvery === 0 && (
                <span className="text-[9px] text-muted-foreground/60 leading-none">{shortDate(day.date)}</span>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function shortModel(model: string): string {
  if (model.includes("opus")) return "opus";
  if (model.includes("sonnet")) return "sonnet";
  if (model.includes("haiku")) return "haiku";
  return model.split("-").slice(0, 2).join("-");
}

function SpendCard({
  label,
  spent,
  limit,
}: {
  label: string;
  spent: number;
  limit: number | null;
}) {
  const pct = budgetPct(spent, limit);
  const color = budgetColor(spent, limit);

  return (
    <div className="p-4 rounded-xl bg-card border border-border">
      <p className="text-xs text-muted-foreground mb-1">{label}</p>
      <p className="text-lg font-semibold tabular-nums">
        {formatCost(spent)}
        {limit != null && (
          <span className="text-xs text-muted-foreground font-normal ml-1">
            / ${limit.toFixed(2)}
          </span>
        )}
      </p>
      {limit != null && (
        <div className="mt-2 h-1.5 rounded-full bg-muted overflow-hidden">
          <div
            className={`h-full rounded-full transition-all ${color}`}
            style={{ width: `${pct}%` }}
          />
        </div>
      )}
    </div>
  );
}

function computeProjection(
  actualSpend: number,
  period: FilterPeriod,
): { projected: number; elapsedPct: number } | null {
  if (period === "all") return null;
  const now = new Date();
  let elapsed: number;
  let total: number;

  if (period === "day") {
    elapsed = now.getHours() + now.getMinutes() / 60;
    total = 24;
  } else if (period === "week") {
    // getDay() returns 0=Sun..6=Sat; treat week as Mon-Sun so shift by 1
    const dayOfWeek = (now.getDay() + 6) % 7; // 0=Mon..6=Sun
    elapsed = dayOfWeek + now.getHours() / 24;
    total = 7;
  } else if (period === "month") {
    elapsed = now.getDate() - 1 + now.getHours() / 24;
    total = new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
  } else {
    // year
    const startOfYear = new Date(now.getFullYear(), 0, 1);
    elapsed = (now.getTime() - startOfYear.getTime()) / (1000 * 60 * 60 * 24);
    const isLeap = now.getFullYear() % 4 === 0 && (now.getFullYear() % 100 !== 0 || now.getFullYear() % 400 === 0);
    total = isLeap ? 366 : 365;
  }

  if (elapsed <= 0) return null;
  const elapsedPct = Math.min((elapsed / total) * 100, 100);
  const projected = (actualSpend / elapsed) * total;
  return { projected, elapsedPct };
}

function CreditsCard({ credits, period, periodSpend }: { credits?: CreditsResponse; period: FilterPeriod; periodSpend: number }) {
  const isAdmin = credits?.source === "admin_api";
  const periodLabel = FILTER_LABELS.find(f => f.key === period)?.label ?? "All";
  const label = `Estimated Usage (${periodLabel})`;
  if (credits?.error) {
    console.warn("[CreditsCard] Admin API issue:", credits.error);
  }

  // Use period-appropriate spend for projection (not always monthly)
  const actualSpend = periodSpend;
  const projection = computeProjection(actualSpend, period);

  return (
    <div className="p-4 rounded-xl bg-card border border-border">
      <div className="flex items-center gap-1.5 mb-1">
        <p className="text-xs text-muted-foreground">{label}</p>
        <div className="relative group">
          <Info className="w-3 h-3 text-muted-foreground/50 cursor-help" />
          <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-1 px-2 py-1 rounded bg-popover border border-border text-[10px] text-muted-foreground w-48 text-center shadow-md opacity-0 group-hover:opacity-100 pointer-events-none transition-opacity z-20">
            {isAdmin
              ? "Usage data from Anthropic Admin API."
              : "Estimated from local usage tracking."}
          </div>
        </div>
      </div>
      {projection ? (
        <>
          <p className="text-lg font-semibold tabular-nums">
            ~{formatCost(projection.projected)}
            <span className="text-xs font-normal text-muted-foreground ml-1.5">projected</span>
          </p>
          <p className="text-sm text-muted-foreground mt-0.5 tabular-nums">
            {formatCost(actualSpend)}{" "}
            <span className="text-xs">spent so far</span>
          </p>
          <div className="mt-2">
            <div className="flex justify-between text-[10px] text-muted-foreground/70 mb-0.5">
              <span>{Math.round(projection.elapsedPct)}% of {periodLabel.toLowerCase()} elapsed</span>
              <span>{formatCost(actualSpend)} / ~{formatCost(projection.projected)}</span>
            </div>
            <div className="h-1 rounded-full bg-muted overflow-hidden">
              <div
                className="h-full rounded-full bg-primary/60 transition-all"
                style={{ width: `${projection.elapsedPct}%` }}
              />
            </div>
          </div>
        </>
      ) : (
        <p className="text-lg font-semibold tabular-nums">
          {credits ? formatCost(actualSpend) : "$0.00"}
        </p>
      )}
      <p className="text-[10px] text-muted-foreground mt-1">
        {isAdmin ? "via Anthropic Admin API" : "estimated from local tracking"}
      </p>
      {!credits?.available && (
        <p className="text-[10px] text-muted-foreground/60 mt-1">
          Add ANTHROPIC_ADMIN_KEY in Secrets for real cost data
        </p>
      )}
    </div>
  );
}

function BudgetEditor({ budget }: { budget?: BudgetInfo }) {
  const [dailyDraft, setDailyDraft] = useState("");
  const [monthlyDraft, setMonthlyDraft] = useState("");
  const [editingBudget, setEditingBudget] = useState(false);
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const queryClient = useQueryClient();

  const mutation = useMutation({
    mutationFn: (vals: { daily: string; monthly: string }) => {
      const daily = vals.daily.trim() ? parseFloat(vals.daily) : null;
      const monthly = vals.monthly.trim() ? parseFloat(vals.monthly) : null;
      return api.providers.setBudget({
        daily_limit_usd: daily,
        monthly_limit_usd: monthly,
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["providers", "budget"] });
      setEditingBudget(false);
      setToast({ msg: "Budget limits updated", ok: true });
      setTimeout(() => setToast(null), 2500);
    },
    onError: () => {
      setToast({ msg: "Failed to update budget", ok: false });
      setTimeout(() => setToast(null), 2500);
    },
  });

  const startEdit = () => {
    setDailyDraft(budget?.daily_limit_usd?.toString() ?? "");
    setMonthlyDraft(budget?.monthly_limit_usd?.toString() ?? "");
    setEditingBudget(true);
  };

  return (
    <div className="p-4 rounded-xl bg-card border border-border">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-muted-foreground">Budget Limits</h3>
        {!editingBudget && (
          <button
            aria-label="Edit budget"
            onClick={startEdit}
            className="p-1 rounded hover:bg-muted transition-colors"
          >
            <Pencil className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        )}
      </div>

      {!editingBudget ? (
        <div className="space-y-2 text-sm">
          <div className="flex justify-between">
            <span className="text-muted-foreground">Daily limit</span>
            <span className="font-mono tabular-nums">
              {budget?.daily_limit_usd != null ? `$${budget.daily_limit_usd.toFixed(2)}` : "No limit"}
            </span>
          </div>
          <div className="flex justify-between">
            <span className="text-muted-foreground">Monthly limit</span>
            <span className="font-mono tabular-nums">
              {budget?.monthly_limit_usd != null ? `$${budget.monthly_limit_usd.toFixed(2)}` : "No limit"}
            </span>
          </div>
        </div>
      ) : (
        <div className="space-y-3">
          <div className="flex items-center gap-2">
            <label className="text-sm text-muted-foreground w-20">Daily</label>
            <div className="flex items-center gap-1 flex-1">
              <span className="text-sm text-muted-foreground">$</span>
              <input
                type="number"
                min="0"
                step="0.01"
                value={dailyDraft}
                onChange={(e) => setDailyDraft(e.target.value)}
                placeholder="No limit"
                className="flex-1 text-sm font-mono bg-background border border-border rounded px-2 py-1 focus:outline-none focus:ring-2 focus:ring-primary/30"
                autoFocus
              />
            </div>
          </div>
          <div className="flex items-center gap-2">
            <label className="text-sm text-muted-foreground w-20">Monthly</label>
            <div className="flex items-center gap-1 flex-1">
              <span className="text-sm text-muted-foreground">$</span>
              <input
                type="number"
                min="0"
                step="0.01"
                value={monthlyDraft}
                onChange={(e) => setMonthlyDraft(e.target.value)}
                placeholder="No limit"
                className="flex-1 text-sm font-mono bg-background border border-border rounded px-2 py-1 focus:outline-none focus:ring-2 focus:ring-primary/30"
              />
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={() => mutation.mutate({ daily: dailyDraft, monthly: monthlyDraft })}
              disabled={mutation.isPending}
              className="px-3 py-1 rounded bg-primary text-primary-foreground text-sm hover:bg-primary/90 disabled:opacity-50"
            >
              Save
            </button>
            <button
              onClick={() => setEditingBudget(false)}
              className="px-3 py-1 rounded bg-muted text-muted-foreground text-sm hover:bg-muted/80"
            >
              Cancel
            </button>
          </div>
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
// Existing field components (unchanged)
// ---------------------------------------------------------------------------

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
