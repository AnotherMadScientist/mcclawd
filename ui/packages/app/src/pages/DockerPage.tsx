import { useState, useEffect, useRef } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Container,
  RefreshCw,
  CheckCircle,
  XCircle,
  Loader2,
  ChevronDown,
  ChevronRight,
  Box,
  HardDrive,
  Wrench,
  FileText,
  ImageIcon,
  Puzzle,
} from "lucide-react";
import { api } from "../api/client";
import type { DockerBuildStatus, ContainerInfo } from "../api/types";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatBytes(bytes: number | null): string {
  if (bytes === null) return "—";
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
  if (bytes >= 1_024) return `${(bytes / 1_024).toFixed(1)} KB`;
  return `${bytes} B`;
}

function formatTimeAgo(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function maskSecretValue(key: string, value: string): string {
  const secretPatterns = /key|token|secret|password|pass|pwd|auth|credential/i;
  if (secretPatterns.test(key)) return "••••••••";
  return value;
}

// ---------------------------------------------------------------------------
// Status badge
// ---------------------------------------------------------------------------

type BadgeVariant = "green" | "yellow" | "red" | "blue" | "gray";

function statusBadge(status: DockerBuildStatus["status"]): {
  variant: BadgeVariant;
  label: string;
} {
  switch (status) {
    case "image_ready":
    case "complete":
      return { variant: "green", label: status === "image_ready" ? "Ready" : "Complete" };
    case "building":
      return { variant: "blue", label: "Building" };
    case "failed":
      return { variant: "red", label: "Failed" };
    case "checking":
    default:
      return { variant: "gray", label: "Checking" };
  }
}

function containerBadge(state: string): { variant: BadgeVariant; label: string } {
  switch (state.toLowerCase()) {
    case "running":
      return { variant: "green", label: "Running" };
    case "created":
      return { variant: "blue", label: "Created" };
    case "exited":
    case "dead":
      return { variant: "red", label: state.charAt(0).toUpperCase() + state.slice(1) };
    case "paused":
    case "restarting":
      return { variant: "yellow", label: state.charAt(0).toUpperCase() + state.slice(1) };
    default:
      return { variant: "gray", label: state };
  }
}

const BADGE_CLASSES: Record<BadgeVariant, string> = {
  green: "bg-emerald-500/15 text-emerald-400 border border-emerald-500/25",
  yellow: "bg-yellow-500/15 text-yellow-400 border border-yellow-500/25",
  red: "bg-red-500/15 text-red-400 border border-red-500/25",
  blue: "bg-blue-500/15 text-blue-400 border border-blue-500/25",
  gray: "bg-zinc-500/15 text-zinc-400 border border-zinc-500/25",
};

function Badge({ variant, label }: { variant: BadgeVariant; label: string }) {
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${BADGE_CLASSES[variant]}`}>
      {label}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Runner Image Card
// ---------------------------------------------------------------------------

function RunnerImageCard() {
  const queryClient = useQueryClient();
  const [logsOpen, setLogsOpen] = useState(false);
  const logsEndRef = useRef<HTMLDivElement>(null);

  const isBuilding = (status: DockerBuildStatus["status"]) =>
    status === "building" || status === "checking";

  const { data: buildStatus } = useQuery({
    queryKey: ["docker", "build-status"],
    queryFn: api.docker.buildStatus,
    refetchInterval: (query) => {
      const status = query.state.data?.status;
      return status && isBuilding(status) ? 2000 : 10000;
    },
  });

  const buildMutation = useMutation({
    mutationFn: api.docker.triggerBuild,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["docker", "build-status"] });
    },
  });

  // Auto-scroll logs when open
  useEffect(() => {
    if (logsOpen && logsEndRef.current) {
      logsEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [buildStatus?.logs, logsOpen]);

  const badge = buildStatus ? statusBadge(buildStatus.status) : { variant: "gray" as BadgeVariant, label: "Loading" };
  const active = buildStatus ? isBuilding(buildStatus.status) : false;

  return (
    <div className="bg-card border border-border rounded-xl p-6 space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <HardDrive className="w-5 h-5 text-muted-foreground" />
          <h2 className="text-base font-semibold">Runner Image</h2>
          {buildStatus && <Badge variant={badge.variant} label={badge.label} />}
        </div>
        <button
          onClick={() => buildMutation.mutate()}
          disabled={active || buildMutation.isPending}
          className="flex items-center gap-2 px-3 py-1.5 rounded-md text-sm font-medium bg-primary/10 text-primary hover:bg-primary/20 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {active || buildMutation.isPending ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <RefreshCw className="w-4 h-4" />
          )}
          Rebuild Image
        </button>
      </div>

      {/* Progress bar */}
      {buildStatus && (
        <div className="space-y-1.5">
          <div className="flex items-center justify-between text-xs text-muted-foreground">
            <span>
              {buildStatus.status === "failed" && buildStatus.error
                ? buildStatus.error
                : buildStatus.status === "image_ready" || buildStatus.status === "complete"
                  ? "Image is ready"
                  : buildStatus.status === "building"
                    ? "Building…"
                    : buildStatus.status === "checking"
                      ? "Checking Docker daemon…"
                      : ""}
            </span>
            <span>{buildStatus.progress_pct}%</span>
          </div>
          <div className="w-full h-1.5 rounded-full bg-muted overflow-hidden">
            <div
              className={`h-full rounded-full transition-all duration-500 ${
                buildStatus.status === "failed"
                  ? "bg-red-500"
                  : buildStatus.status === "image_ready" || buildStatus.status === "complete"
                    ? "bg-emerald-500"
                    : "bg-blue-500"
              } ${active ? "animate-pulse" : ""}`}
              style={{ width: `${buildStatus.progress_pct}%` }}
            />
          </div>
        </div>
      )}

      {/* Image metadata */}
      {buildStatus?.image_available && (
        <div className="flex items-center gap-6 text-xs text-muted-foreground">
          {buildStatus.image_id && (
            <span className="flex items-center gap-1.5">
              <CheckCircle className="w-3.5 h-3.5 text-emerald-400" />
              <span className="font-mono">{buildStatus.image_id.slice(0, 19)}</span>
            </span>
          )}
          {buildStatus.image_size !== null && (
            <span>{formatBytes(buildStatus.image_size)}</span>
          )}
          {buildStatus.build_duration_secs != null && (
            <span>Build: {buildStatus.build_duration_secs.toFixed(1)}s</span>
          )}
          {buildStatus.agent_startup_secs != null && (
            <span>Agent startup: {buildStatus.agent_startup_secs.toFixed(1)}s</span>
          )}
        </div>
      )}

      {/* Error state */}
      {buildStatus?.status === "failed" && buildStatus.error && (
        <div className="flex items-start gap-2 p-3 rounded-lg bg-red-500/10 border border-red-500/20 text-sm text-red-400">
          <XCircle className="w-4 h-4 mt-0.5 shrink-0" />
          <span>{buildStatus.error}</span>
        </div>
      )}

      {/* Logs collapsible */}
      {buildStatus && buildStatus.logs.length > 0 && (
        <div>
          <button
            onClick={() => setLogsOpen(!logsOpen)}
            className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            {logsOpen ? (
              <ChevronDown className="w-3.5 h-3.5" />
            ) : (
              <ChevronRight className="w-3.5 h-3.5" />
            )}
            Build logs ({buildStatus.logs.length} lines)
          </button>
          {logsOpen && (
            <div className="mt-2 rounded-lg bg-zinc-950 border border-zinc-800 p-3 max-h-64 overflow-y-auto">
              <pre className="text-xs font-mono text-zinc-300 whitespace-pre-wrap leading-5">
                {buildStatus.logs.join("\n")}
              </pre>
              <div ref={logsEndRef} />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Container row (expandable)
// ---------------------------------------------------------------------------

function ContainerRow({ container, onDelete }: { container: ContainerInfo; onDelete: (id: string) => void }) {
  const [expanded, setExpanded] = useState(false);
  const badge = containerBadge(container.state);
  const agentType = container.labels?.agent_type ?? "task";
  const isExited = ["exited", "dead", "removed"].includes(container.state.toLowerCase());

  const envEntries = Object.entries(container.env_vars);
  const labelEntries = Object.entries(container.labels).filter(
    ([k]) => !k.startsWith("com.docker") && !k.startsWith("org.opencontainers"),
  );

  return (
    <>
      <tr
        className={`border-b border-border cursor-pointer hover:bg-muted/40 transition-colors ${isExited ? "opacity-50" : ""}`}
        onClick={() => setExpanded(!expanded)}
      >
        <td className="py-3 px-4">
          <div className="flex items-center gap-2">
            {expanded ? (
              <ChevronDown className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
            ) : (
              <ChevronRight className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
            )}
            <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${
              agentType === "system"
                ? "bg-violet-500/20 text-violet-400"
                : "bg-blue-500/20 text-blue-400"
            }`}>
              {agentType === "system" ? "System" : "Task"}
            </span>
          </div>
        </td>
        <td className="py-3 px-4 text-sm">
          {agentType === "system" ? (
            <span className="text-violet-400 font-medium">System Agent</span>
          ) : container.task_id ? (
            <a
              href={`/tasks/${container.task_id}`}
              onClick={(e) => e.stopPropagation()}
              className="text-primary hover:underline"
            >
              <span className="font-mono text-xs text-muted-foreground">{container.task_id.slice(0, 8)}</span>
            </a>
          ) : (
            <span className="text-muted-foreground">—</span>
          )}
        </td>
        <td className="py-3 px-4">
          <Badge variant={badge.variant} label={badge.label} />
        </td>
        <td className="py-3 px-4 text-sm text-muted-foreground">
          <div className="flex flex-wrap gap-1">
            {/* Mounts */ container.mounts.length === 0 ? (
              <span>—</span>
            ) : (
              container.mounts.map((m, i) => (
                <span key={i} className="inline-flex items-center px-1.5 py-0.5 rounded bg-zinc-800 text-xs font-mono">
                  {m.destination}
                </span>
              ))
            )}
          </div>
        </td>
        <td className="py-3 px-4">
          <div className="flex flex-wrap gap-1">
            {(container.mcp_tools ?? []).map((tool) => (
              <span key={tool} className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-blue-500/15 text-blue-400 text-xs font-medium border border-blue-500/25">
                <Wrench className="w-3 h-3" />
                {tool}
              </span>
            ))}
            {(container.skills ?? []).map((skill) => (
              <span key={skill} className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-emerald-500/15 text-emerald-400 text-xs font-medium border border-emerald-500/25">
                <Puzzle className="w-3 h-3" />
                {skill}
              </span>
            ))}
            {!(container.mcp_tools ?? []).length && !(container.skills ?? []).length && (
              <span className="text-muted-foreground text-xs">—</span>
            )}
          </div>
        </td>
        <td className="py-3 px-4 text-sm text-muted-foreground">
          {container.created ? formatTimeAgo(container.created) : "—"}
        </td>
        <td className="py-3 px-4">
          <button
            onClick={(e) => { e.stopPropagation(); onDelete(container.id); }}
            className="p-1.5 rounded-md text-zinc-500 hover:text-red-400 hover:bg-red-500/10 transition-colors"
            title="Delete container and task"
          >
            <XCircle className="w-4 h-4" />
          </button>
        </td>
      </tr>

      {expanded && (
        <tr className="border-b border-border bg-muted/20">
          <td colSpan={7} className="px-4 py-4">
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4 text-xs">
              {/* Ports */}
              {container.ports.length > 0 && (
                <div>
                  <p className="font-medium text-muted-foreground mb-1.5">Ports</p>
                  <div className="space-y-1">
                    {container.ports.map((p) => (
                      <div key={p} className="font-mono text-foreground bg-zinc-900 px-2 py-1 rounded">
                        {p}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Mounts */}
              {container.mounts.length > 0 && (
                <div>
                  <p className="font-medium text-muted-foreground mb-1.5">Mounts</p>
                  <div className="space-y-1">
                    {container.mounts.map((m, i) => (
                      <div key={i} className="font-mono text-foreground bg-zinc-900 px-2 py-1 rounded">
                        <span className="text-zinc-400">{m.source}</span>
                        <span className="text-zinc-600 mx-1">→</span>
                        <span>{m.destination}</span>
                        <span className="text-zinc-500 ml-1">({m.mode})</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Attachments */}
              {container.attachments && container.attachments.length > 0 && (
                <div>
                  <p className="font-medium text-muted-foreground mb-1.5">Attachments ({container.attachments.length})</p>
                  <div className="flex flex-wrap gap-2">
                    {container.attachments.map((att) => (
                      <div key={att.name} className="flex items-center gap-1.5 bg-zinc-900 px-2 py-1 rounded text-xs">
                        {att.is_image ? <ImageIcon className="w-3.5 h-3.5 text-violet-400" /> : <FileText className="w-3.5 h-3.5 text-zinc-400" />}
                        <span className="text-zinc-300">{att.name}</span>
                        <span className="text-zinc-500">({formatBytes(att.size)})</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Gateway URL */}
              {container.gateway_url && (
                <div>
                  <p className="font-medium text-muted-foreground mb-1.5">Gateway URL</p>
                  <div className="font-mono text-foreground bg-zinc-900 px-2 py-1 rounded text-xs">
                    {container.gateway_url}
                  </div>
                </div>
              )}

              {/* Env vars */}
              {envEntries.length > 0 && (
                <div className="md:col-span-2">
                  <p className="font-medium text-muted-foreground mb-1.5">
                    Environment ({envEntries.length})
                  </p>
                  <div className="grid grid-cols-2 gap-1">
                    {envEntries.map(([k, v]) => (
                      <div key={k} className="flex gap-2 font-mono bg-zinc-900 px-2 py-1 rounded">
                        <span className="text-blue-400 shrink-0">{k}</span>
                        <span className="text-zinc-400">=</span>
                        <span className="text-zinc-300 truncate">{maskSecretValue(k, v)}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Labels */}
              {labelEntries.length > 0 && (
                <div className="md:col-span-2">
                  <p className="font-medium text-muted-foreground mb-1.5">
                    Labels ({labelEntries.length})
                  </p>
                  <div className="grid grid-cols-2 gap-1">
                    {labelEntries.map(([k, v]) => (
                      <div key={k} className="flex gap-2 font-mono bg-zinc-900 px-2 py-1 rounded">
                        <span className="text-violet-400 shrink-0 truncate">{k}</span>
                        <span className="text-zinc-400">=</span>
                        <span className="text-zinc-300 truncate">{v}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// Containers Card
// ---------------------------------------------------------------------------

function ContainersCard() {
  const queryClient = useQueryClient();
  const { data: containers, isLoading, isError, refetch, isFetching } = useQuery({
    queryKey: ["docker", "containers"],
    queryFn: api.docker.containers,
    refetchInterval: 2000,
  });

  const deleteMutation = useMutation({
    mutationFn: api.docker.deleteContainer,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["docker", "containers"] });
      queryClient.invalidateQueries({ queryKey: ["tasks"] });
    },
  });

  return (
    <div className="bg-card border border-border rounded-xl p-6 space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Container className="w-5 h-5 text-muted-foreground" />
          <h2 className="text-base font-semibold">Agent Containers</h2>
          {containers && (
            <span className="text-xs text-muted-foreground">
              {containers.length} {containers.length === 1 ? "container" : "containers"}
            </span>
          )}
        </div>
        <button
          onClick={() => refetch()}
          disabled={isFetching}
          className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs text-muted-foreground hover:text-foreground hover:bg-muted transition-colors disabled:opacity-50"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${isFetching ? "animate-spin" : ""}`} />
          Refresh
        </button>
      </div>

      {isLoading && (
        <div className="flex items-center justify-center py-12 text-muted-foreground text-sm gap-2">
          <Loader2 className="w-4 h-4 animate-spin" />
          Loading containers…
        </div>
      )}

      {isError && (
        <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 border border-red-500/20 text-sm text-red-400">
          <XCircle className="w-4 h-4 shrink-0" />
          Failed to load containers. Is Docker running?
        </div>
      )}

      {!isLoading && !isError && containers && containers.length === 0 && (
        <div className="flex flex-col items-center justify-center py-12 gap-3 text-muted-foreground">
          <Box className="w-10 h-10 opacity-30" />
          <p className="text-sm">No agent containers running</p>
        </div>
      )}

      {!isLoading && !isError && containers && containers.length > 0 && (
        <div className="overflow-x-auto rounded-lg border border-border">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/30">
                <th className="py-2.5 px-4 text-left text-xs font-medium text-muted-foreground">Type</th>
                <th className="py-2.5 px-4 text-left text-xs font-medium text-muted-foreground">Task</th>
                <th className="py-2.5 px-4 text-left text-xs font-medium text-muted-foreground">Status</th>
                <th className="py-2.5 px-4 text-left text-xs font-medium text-muted-foreground">Mounts</th>
                <th className="py-2.5 px-4 text-left text-xs font-medium text-muted-foreground">Tools</th>
                <th className="py-2.5 px-4 text-left text-xs font-medium text-muted-foreground">Created</th>
                <th className="py-2.5 px-4 text-left text-xs font-medium text-muted-foreground w-12"></th>
              </tr>
            </thead>
            <tbody>
              {[...containers]
                .sort((a, b) => {
                  const aExited = ["exited", "dead", "removed"].includes(a.state.toLowerCase()) ? 1 : 0;
                  const bExited = ["exited", "dead", "removed"].includes(b.state.toLowerCase()) ? 1 : 0;
                  return aExited - bExited || b.created - a.created;
                })
                .map((c) => (
                  <ContainerRow key={c.id} container={c} onDelete={(id) => deleteMutation.mutate(id)} />
                ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export function DockerPage() {
  return (
    <div className="max-w-4xl mx-auto space-y-6 p-6">
      <div className="flex items-center gap-3">
        <Container className="w-6 h-6 text-muted-foreground" />
        <h1 className="text-xl font-semibold">Docker Management</h1>
      </div>

      <RunnerImageCard />
      <ContainersCard />
    </div>
  );
}
