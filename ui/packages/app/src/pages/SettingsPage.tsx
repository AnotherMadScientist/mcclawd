import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";

export function SettingsPage() {
  const { data: config } = useQuery({
    queryKey: ["config"],
    queryFn: api.config.get,
  });

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Settings</h1>

      <div className="space-y-4">
        <Field label="Model" value={config?.agent.model} />
        <Field label="Max Turns" value={config?.agent.max_turns?.toString()} />
        <Field label="Default Workspace" value={config?.agent.default_workspace} />
        <Field label="Data Directory" value={config?.data_dir} />
        <Field label="AgentGateway URL" value={config?.mcp.agentgateway_url} />
      </div>
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
