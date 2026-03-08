import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Server, Plus, Trash2, RotateCw, X, Loader2, Wrench } from "lucide-react";
import { api } from "../api/client";
import { ListSkeleton } from "../components/LoadingSkeleton";
import { ErrorState } from "../components/ErrorState";
import type { McpServer, McpToolOverview, ContainerInfo } from "../api/types";

export function McpServersPage() {
  const queryClient = useQueryClient();
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [restartingServer, setRestartingServer] = useState<string | null>(null);
  const [removingServer, setRemovingServer] = useState<string | null>(null);

  const {
    data: servers = [],
    isLoading,
    isError,
    refetch,
  } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: api.mcp.servers,
  });

  const restartMutation = useMutation({
    mutationFn: (name: string) => api.mcp.restartServer(name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["mcp-servers"] });
      setRestartingServer(null);
    },
    onError: () => setRestartingServer(null),
  });

  const removeMutation = useMutation({
    mutationFn: (name: string) => api.mcp.removeServer(name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["mcp-servers"] });
      setRemovingServer(null);
    },
    onError: () => setRemovingServer(null),
  });

  if (isLoading) {
    return (
      <div className="max-w-2xl mx-auto space-y-6">
        <h1 className="text-2xl font-bold">MCP Servers</h1>
        <ListSkeleton count={3} />
      </div>
    );
  }

  if (isError) {
    return (
      <div className="max-w-2xl mx-auto space-y-6">
        <h1 className="text-2xl font-bold">MCP Servers</h1>
        <ErrorState message="Failed to load MCP servers" onRetry={() => refetch()} />
      </div>
    );
  }

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">MCP Servers</h1>
        <button
          onClick={() => setShowAddDialog(true)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition-opacity"
        >
          <Plus className="w-3.5 h-3.5" />
          Add Server
        </button>
      </div>

      <div className="space-y-3">
        {servers.map((s: McpServer) => (
          <div
            key={s.name}
            className="flex items-center justify-between p-4 rounded-xl bg-card border border-border group"
          >
            <div className="flex items-center gap-3">
              {/* Status dot — green = configured/running */}
              <div className="relative">
                <Server className="w-5 h-5 text-emerald-400" />
                <span className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-green-500 border border-card" />
              </div>
              <div>
                <p className="text-sm font-medium">{s.name}</p>
                <p className="text-xs text-muted-foreground font-mono">{s.image}</p>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground">:{s.port}</span>
              <button
                onClick={() => {
                  setRestartingServer(s.name);
                  restartMutation.mutate(s.name);
                }}
                disabled={restartingServer === s.name}
                className="p-1.5 rounded-lg opacity-0 group-hover:opacity-100 hover:bg-muted transition-all disabled:opacity-50"
                title="Restart server"
              >
                {restartingServer === s.name ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin text-muted-foreground" />
                ) : (
                  <RotateCw className="w-3.5 h-3.5 text-muted-foreground" />
                )}
              </button>
              <button
                onClick={() => {
                  setRemovingServer(s.name);
                  removeMutation.mutate(s.name);
                }}
                disabled={removingServer === s.name}
                className="p-1.5 rounded-lg opacity-0 group-hover:opacity-100 hover:bg-red-500/10 transition-all disabled:opacity-50"
                title="Remove server"
              >
                {removingServer === s.name ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin text-muted-foreground" />
                ) : (
                  <Trash2 className="w-3.5 h-3.5 text-muted-foreground hover:text-red-400" />
                )}
              </button>
            </div>
          </div>
        ))}
        {servers.length === 0 && (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <Server className="w-10 h-10 text-muted-foreground/40 mb-3" />
            <p className="text-sm text-muted-foreground">No MCP servers configured</p>
            <p className="text-xs text-muted-foreground mt-1">
              Click <strong>Add Server</strong> to connect one
            </p>
          </div>
        )}
      </div>

      <McpToolsOverview servers={servers} />

      {showAddDialog && (
        <AddServerDialog
          onClose={() => setShowAddDialog(false)}
          onAdded={() => {
            queryClient.invalidateQueries({ queryKey: ["mcp-servers"] });
            setShowAddDialog(false);
          }}
        />
      )}
    </div>
  );
}

function AddServerDialog({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  onAdded: () => void;
}) {
  const [name, setName] = useState("");
  const [image, setImage] = useState("");
  const [port, setPort] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [env, setEnv] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [nameError, setNameError] = useState("");
  const [imageError, setImageError] = useState("");

  const addMutation = useMutation({
    mutationFn: () => {
      const envObj: Record<string, string> = {};
      if (env.trim()) {
        for (const line of env.split("\n")) {
          const eqIdx = line.indexOf("=");
          if (eqIdx > 0) {
            envObj[line.slice(0, eqIdx).trim()] = line.slice(eqIdx + 1).trim();
          }
        }
      }
      return api.mcp.addServer({
        name: name.trim(),
        image: image.trim(),
        port: parseInt(port, 10),
        command: command.trim() || undefined,
        args: args.trim() ? args.split(/\s+/) : undefined,
        env: Object.keys(envObj).length > 0 ? envObj : undefined,
      });
    },
    onSuccess: () => onAdded(),
    onError: (err: Error) => setError(err.message),
  });

  const canSubmit = name.trim() && image.trim() && port.trim() && parseInt(port, 10) > 0;

  const handleSubmit = () => {
    let valid = true;
    if (!name.trim()) { setNameError("Name is required"); valid = false; } else setNameError("");
    if (!image.trim()) { setImageError("Image is required"); valid = false; } else setImageError("");
    if (valid && canSubmit) addMutation.mutate();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-6"
      onClick={onClose}
    >
      <div
        role="dialog"
        data-testid="add-server-dialog"
        className="bg-card border border-border rounded-2xl shadow-2xl w-full max-w-lg flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-6 py-4 border-b border-border">
          <h2 className="text-lg font-semibold">Add MCP Server</h2>
          <button onClick={onClose} className="p-1.5 rounded-lg hover:bg-muted transition-colors">
            <X className="w-5 h-5 text-muted-foreground" />
          </button>
        </div>

        <div className="px-6 py-4 space-y-4">
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-foreground/70">Name *</label>
            <input
              value={name}
              onChange={(e) => { setName(e.target.value); if (e.target.value.trim()) setNameError(""); }}
              placeholder="e.g. my-mcp-server"
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm focus:outline-none focus:ring-1 focus:ring-primary/50 font-mono"
              autoFocus
            />
            {nameError && <p className="text-xs text-destructive">{nameError}</p>}
          </div>
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-foreground/70">Image *</label>
            <input
              value={image}
              onChange={(e) => { setImage(e.target.value); if (e.target.value.trim()) setImageError(""); }}
              placeholder="e.g. mcp-my-server:latest"
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm focus:outline-none focus:ring-1 focus:ring-primary/50 font-mono"
            />
            {imageError && <p className="text-xs text-destructive">{imageError}</p>}
          </div>
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-foreground/70">Port *</label>
            <input
              type="number"
              value={port}
              onChange={(e) => setPort(e.target.value)}
              placeholder="e.g. 8004"
              min={1}
              max={65535}
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm focus:outline-none focus:ring-1 focus:ring-primary/50 font-mono"
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-foreground/70">Command</label>
            <input
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              placeholder="e.g. node"
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm focus:outline-none focus:ring-1 focus:ring-primary/50 font-mono"
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-foreground/70">Args (space-separated)</label>
            <input
              value={args}
              onChange={(e) => setArgs(e.target.value)}
              placeholder="e.g. --stdio --verbose"
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm focus:outline-none focus:ring-1 focus:ring-primary/50 font-mono"
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-foreground/70">Environment (KEY=VALUE per line)</label>
            <textarea
              value={env}
              onChange={(e) => setEnv(e.target.value)}
              placeholder={"API_KEY=secret\nDEBUG=true"}
              rows={3}
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm focus:outline-none focus:ring-1 focus:ring-primary/50 font-mono resize-none"
            />
          </div>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>

        <div className="flex items-center justify-end gap-2 px-6 py-3 border-t border-border">
          <button
            onClick={onClose}
            className="px-3 py-1.5 rounded-lg bg-muted text-xs font-medium hover:bg-muted/80 transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={addMutation.isPending}
            className="px-4 py-1.5 rounded-lg bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition-opacity disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1.5"
          >
            {addMutation.isPending ? (
              <>
                <Loader2 className="w-3 h-3 animate-spin" />
                Adding...
              </>
            ) : (
              "Add Server"
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

function useMcpToolsOverview(servers: McpServer[]): {
  tools: McpToolOverview[];
  isLoading: boolean;
} {
  const { data: containers = [], isLoading } = useQuery({
    queryKey: ["docker-containers"],
    queryFn: api.docker.containers,
    refetchInterval: 5000,
  });

  const tools = useMemo(() => {
    return servers.map((s) => {
      const matched = containers.filter(
        (c: ContainerInfo) => c.mcp_tools?.includes(s.name),
      );
      const active = matched.some(
        (c: ContainerInfo) => c.state === "running",
      );
      return {
        name: s.name,
        image: s.image,
        port: s.port,
        status: active ? ("active" as const) : ("idle" as const),
        containers: matched.map((c: ContainerInfo) => ({
          id: c.id,
          name: c.name,
          task_id: c.task_id,
          state: c.state,
        })),
      };
    });
  }, [servers, containers]);

  return { tools, isLoading };
}

function McpToolsOverview({ servers }: { servers: McpServer[] }) {
  const { tools, isLoading } = useMcpToolsOverview(servers);

  if (isLoading) return null;

  const activeCount = tools.filter((t) => t.status === "active").length;

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <Wrench className="w-4 h-4 text-muted-foreground" />
        <h2 className="text-lg font-semibold">MCP Tools</h2>
        <span className="px-1.5 py-0.5 text-[10px] font-medium rounded-full bg-muted text-muted-foreground">
          {activeCount}/{tools.length}
        </span>
      </div>

      <div className="space-y-2">
        {tools.map((t) => (
          <div
            key={t.name}
            data-testid={`mcp-tool-${t.name}`}
            className="flex items-center justify-between p-3 rounded-xl bg-card border border-border"
          >
            <div className="flex items-center gap-3">
              <span
                className={`w-2 h-2 rounded-full ${t.status === "active" ? "bg-green-500" : "bg-muted-foreground/30"}`}
              />
              <div>
                <p className="text-sm font-medium">{t.name}</p>
                <p className="text-xs text-muted-foreground font-mono">
                  {t.image}
                </p>
              </div>
            </div>
            <div className="flex items-center gap-1.5 flex-wrap justify-end">
              {t.containers.length === 0 && (
                <span className="text-[10px] text-muted-foreground/50 px-2 py-0.5 rounded-full bg-muted">
                  No containers
                </span>
              )}
              {t.containers.map((c) => (
                <span
                  key={c.id}
                  className={`text-[10px] font-medium px-2 py-0.5 rounded-full ${
                    c.task_id === "__system__"
                      ? "bg-violet-500/15 text-violet-400"
                      : "bg-blue-500/15 text-blue-400"
                  }`}
                >
                  {c.task_id === "__system__"
                    ? "system"
                    : c.task_id
                      ? c.task_id.slice(0, 8)
                      : c.name}
                </span>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
