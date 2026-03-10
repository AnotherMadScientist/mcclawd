import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { api } from "../api/client";
import { RefreshCw } from "lucide-react";

// In dev, iframe must point directly at the container origin so asset paths resolve correctly.
// In production, the reverse proxy serves worldmonitor at /worldmonitor/.
const WORLDMONITOR_URL = import.meta.env.DEV
  ? "http://localhost:3001"
  : "/worldmonitor/";

export function WorldNewsPage() {
  const [iframeError, setIframeError] = useState(false);

  const syncMutation = useMutation({
    mutationFn: () => api.worldmonitor.syncEnv(),
    onSuccess: (data) => {
      alert(`Synced ${data.synced} secrets: ${data.keys.join(", ") || "none found in vault"}`);
      setIframeError(false);
    },
  });

  if (iframeError) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4 text-muted-foreground">
        <p className="text-lg font-medium">WorldMonitor is not running</p>
        <p className="text-sm">
          Start it with: <code className="bg-muted px-2 py-1 rounded">docker compose up -d worldmonitor</code>
        </p>
        <button
          onClick={() => syncMutation.mutate()}
          disabled={syncMutation.isPending}
          className="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 transition-colors disabled:opacity-50"
        >
          <RefreshCw className={`w-4 h-4 ${syncMutation.isPending ? "animate-spin" : ""}`} />
          {syncMutation.isPending ? "Syncing..." : "Sync Vault Secrets & Restart"}
        </button>
      </div>
    );
  }

  return (
    <div className="absolute inset-0 z-10">
      <iframe
        src={WORLDMONITOR_URL}
        className="w-full h-full border-0"
        title="World News"
        onError={() => setIframeError(true)}
      />
      <button
        onClick={() => syncMutation.mutate()}
        disabled={syncMutation.isPending}
        className="absolute top-2 right-2 p-2 bg-zinc-900/80 text-zinc-300 rounded-md hover:bg-zinc-800 transition-colors disabled:opacity-50 z-20"
        title="Sync vault secrets to WorldMonitor"
      >
        <RefreshCw className={`w-4 h-4 ${syncMutation.isPending ? "animate-spin" : ""}`} />
      </button>
    </div>
  );
}
