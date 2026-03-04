import { useQuery } from "@tanstack/react-query";
import { Server } from "lucide-react";
import { api } from "../api/client";

export function McpServersPage() {
  const { data: servers = [] } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: api.mcp.servers,
  });

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">MCP Servers</h1>

      <div className="space-y-3">
        {servers.map((s) => (
          <div
            key={s.name}
            className="flex items-center justify-between p-4 rounded-xl bg-card border border-border"
          >
            <div className="flex items-center gap-3">
              <Server className="w-5 h-5 text-emerald-400" />
              <div>
                <p className="text-sm font-medium">{s.name}</p>
                <p className="text-xs text-muted-foreground font-mono">{s.image}</p>
              </div>
            </div>
            <span className="text-xs text-muted-foreground">:{s.port}</span>
          </div>
        ))}
        {servers.length === 0 && (
          <p className="text-sm text-muted-foreground text-center py-8">No MCP servers configured</p>
        )}
      </div>
    </div>
  );
}
