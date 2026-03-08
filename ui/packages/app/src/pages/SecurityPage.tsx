import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Shield, Activity, BookOpen } from "lucide-react";
import { api } from "../api/client";
import { SecurityEventsPage } from "./SecurityEventsPage";
import { SecurityRulesPage } from "./SecurityRulesPage";

const TABS = [
  { id: "events", label: "Audit Log", icon: Activity },
  { id: "rules", label: "Detection & Rules", icon: BookOpen },
] as const;

type TabId = (typeof TABS)[number]["id"];

export function SecurityPage() {
  const [tab, setTab] = useState<TabId>("events");

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
    <div className="flex flex-col gap-6 p-6 max-w-7xl mx-auto w-full">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold text-zinc-100 flex items-center gap-2">
          <Shield className="w-6 h-6 text-primary" />
          Security
        </h1>
        <p className="text-zinc-400 text-sm mt-1">
          Agent security pipeline monitoring &amp; DLP configuration
        </p>
      </div>

      {/* Status Bar */}
      <div className="bg-zinc-900 border border-zinc-800 rounded-lg px-4 py-3 flex items-center gap-6 text-sm flex-wrap">
        <div className="flex items-center gap-2">
          <Activity className="w-4 h-4 text-zinc-400" />
          <span className="text-zinc-400">Pipeline hooks:</span>
          <span className="text-zinc-100 font-medium">
            {status ? status.pipeline_hooks : "\u2014"}
          </span>
        </div>
        <div className="w-px h-4 bg-zinc-700" />
        <div className="flex items-center gap-2">
          <Shield className="w-4 h-4 text-zinc-400" />
          <span className="text-zinc-400">Detection Patterns:</span>
          <span className="text-zinc-100 font-medium">
            {status ? status.dlp_pattern_count : "\u2014"}
          </span>
          <span
            className="text-zinc-500 text-xs"
            title="Built-in regex rules that detect sensitive data like API keys, PII, and injection attempts"
          >
            (?)
          </span>
        </div>
        <div className="w-px h-4 bg-zinc-700" />
        <div className="flex items-center gap-2">
          <Shield className="w-4 h-4 text-zinc-400" />
          <span className="text-zinc-400">Response Rules:</span>
          <span className="text-zinc-100 font-medium">{policies.length}</span>
          <span
            className="text-zinc-500 text-xs"
            title="User-configurable policies that determine what action (allow/warn/block) to take when patterns match"
          >
            (?)
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
            {status == null ? "\u2014" : status.sidecar_healthy ? "Healthy" : "Down"}
          </span>
          {status && (
            <span className="text-zinc-500 font-mono text-xs">{status.sidecar_url}</span>
          )}
        </div>
      </div>

      {/* Tab Navigation */}
      <div className="flex items-center gap-1 border-b border-zinc-800">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className={`flex items-center gap-2 px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
              tab === t.id
                ? "border-primary text-zinc-100"
                : "border-transparent text-zinc-500 hover:text-zinc-300"
            }`}
          >
            <t.icon className="w-4 h-4" />
            {t.label}
          </button>
        ))}
      </div>

      {/* Tab Content */}
      {tab === "events" && <SecurityEventsPage />}
      {tab === "rules" && <SecurityRulesPage />}
    </div>
  );
}
