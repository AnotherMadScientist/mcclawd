import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useEffect, useRef, useCallback } from "react";
import {
  Package,
  Search,
  Download,
  Trash2,
  Loader2,
  Check,
  RefreshCw,
  ArrowLeft,
  Puzzle,
} from "lucide-react";
import { api } from "../api/client";
import type { InstalledSkill, ClawHubSkillMeta } from "../api/types";

type Tab = "installed" | "browse";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatTimeAgo(dateString: string | null | undefined): string {
  if (!dateString) return "Never synced";
  const date = new Date(dateString);
  const now = Date.now();
  const diffMs = now - date.getTime();
  if (diffMs < 0) return "just now";

  const seconds = Math.floor(diffMs / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

function sourceLabel(source: InstalledSkill["source"]): string {
  return "Local" in source ? "Local" : "Registry";
}

// ---------------------------------------------------------------------------
// Notification banner
// ---------------------------------------------------------------------------

function NotificationBanner({
  notification,
}: {
  notification: { type: "success" | "error"; message: string };
}) {
  return (
    <div
      className={`px-4 py-2.5 rounded-lg text-sm font-medium ${
        notification.type === "success"
          ? "bg-emerald-500/10 text-emerald-400 border border-emerald-500/20"
          : "bg-red-500/10 text-red-400 border border-red-500/20"
      }`}
    >
      {notification.message}
    </div>
  );
}

// ---------------------------------------------------------------------------
// SkillCard  (compact, reusable for both tabs)
// ---------------------------------------------------------------------------

function SkillCard({
  name,
  version,
  subtitle,
  isInstalled,
  isSelected,
  onClick,
  onInstall,
  installPending,
}: {
  name: string;
  version: string;
  subtitle: string;
  isInstalled?: boolean;
  isSelected?: boolean;
  onClick: () => void;
  onInstall?: () => void;
  installPending?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={`p-3 rounded-xl bg-card border transition-colors text-left w-full ${
        isSelected
          ? "border-primary ring-1 ring-primary/30"
          : "border-border hover:border-primary/40"
      }`}
    >
      <div className="flex items-start gap-2.5">
        <Package className="w-4 h-4 text-violet-400 mt-0.5 shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <p className="text-sm font-medium truncate">{name}</p>
            {isInstalled && (
              <Check className="w-3 h-3 text-emerald-400 shrink-0" />
            )}
          </div>
          <p className="text-xs text-muted-foreground truncate">{subtitle}</p>
        </div>
        {onInstall && !isInstalled ? (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onInstall();
            }}
            disabled={installPending}
            className="p-1 rounded-md hover:bg-muted transition-colors disabled:opacity-50 shrink-0"
            title="Install skill"
          >
            {installPending ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin text-muted-foreground" />
            ) : (
              <Download className="w-3.5 h-3.5 text-muted-foreground hover:text-foreground" />
            )}
          </button>
        ) : (
          <span className="text-xs text-muted-foreground shrink-0">
            v{version}
          </span>
        )}
      </div>
    </button>
  );
}

// ---------------------------------------------------------------------------
// SkillDetail  (inline expanded section)
// ---------------------------------------------------------------------------

function SkillDetail({
  name,
  onBack,
  onNotify,
}: {
  name: string;
  onBack: () => void;
  onNotify: (type: "success" | "error", message: string) => void;
}) {
  const queryClient = useQueryClient();

  const { data: installed = [] } = useQuery({
    queryKey: ["skills"],
    queryFn: api.skills.list,
  });
  const installedInfo = installed.find((s) => s.name === name);

  const { data: skill, isLoading } = useQuery({
    queryKey: ["skill-detail", name],
    queryFn: () => api.skills.detail(name).catch(() => null),
  });

  const install = useMutation({
    mutationFn: () => api.skills.install(name, skill?.version),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      onNotify("success", `Installed "${name}" successfully`);
    },
    onError: (err: Error) => {
      onNotify("error", `Failed to install "${name}": ${err.message}`);
    },
  });

  const uninstall = useMutation({
    mutationFn: () => api.skills.uninstall(name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      onNotify("success", `Uninstalled "${name}"`);
      onBack();
    },
    onError: (err: Error) => {
      onNotify("error", `Failed to uninstall "${name}": ${err.message}`);
    },
  });

  if (isLoading) {
    return (
      <div className="rounded-xl border border-border bg-card p-6 mt-4">
        <div className="flex justify-center py-12">
          <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
        </div>
      </div>
    );
  }

  if (!skill) {
    return (
      <div className="rounded-xl border border-border bg-card p-6 mt-4">
        <button
          onClick={onBack}
          className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors mb-4"
        >
          <ArrowLeft className="w-4 h-4" />
          Back
        </button>
        <p className="text-muted-foreground text-sm">
          Skill details not available. Try refreshing the catalog.
        </p>
      </div>
    );
  }

  return (
    <div className="rounded-xl border border-border bg-card p-6 mt-4">
      {/* Header */}
      <div className="flex items-start justify-between mb-4">
        <button
          onClick={onBack}
          className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
        >
          <ArrowLeft className="w-4 h-4" />
          Back
        </button>

        <div className="flex items-center gap-2">
          {installedInfo ? (
            <button
              onClick={() => uninstall.mutate()}
              disabled={uninstall.isPending}
              className="px-3 py-1.5 rounded-lg bg-red-500/10 text-red-400 text-xs font-medium hover:bg-red-500/20 transition-colors disabled:opacity-50 flex items-center gap-1.5"
            >
              {uninstall.isPending ? (
                <Loader2 className="w-3 h-3 animate-spin" />
              ) : (
                <Trash2 className="w-3 h-3" />
              )}
              Uninstall
            </button>
          ) : (
            <button
              onClick={() => install.mutate()}
              disabled={install.isPending}
              className="px-3 py-1.5 rounded-lg bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition-opacity disabled:opacity-50 flex items-center gap-1.5"
            >
              {install.isPending ? (
                <Loader2 className="w-3 h-3 animate-spin" />
              ) : (
                <Download className="w-3 h-3" />
              )}
              Install
            </button>
          )}
        </div>
      </div>

      {/* Skill info */}
      <div className="space-y-3">
        <div className="flex items-center gap-3">
          <h2 className="text-lg font-semibold">{skill.name}</h2>
          <span className="text-sm text-muted-foreground">v{skill.version}</span>
        </div>

        <p className="text-sm text-muted-foreground">by {skill.author}</p>

        <p className="text-sm leading-relaxed">{skill.description}</p>

        {skill.tags.length > 0 && (
          <div className="flex items-center gap-2 flex-wrap">
            {skill.tags.map((tag) => (
              <span
                key={tag}
                className="px-2 py-0.5 rounded-full bg-muted text-xs"
              >
                {tag}
              </span>
            ))}
          </div>
        )}

        <div className="flex items-center gap-4 text-xs text-muted-foreground pt-1">
          <span className="flex items-center gap-1">
            <Download className="w-3 h-3" />
            {skill.downloads.toLocaleString()} downloads
          </span>
          <span>
            Updated{" "}
            {new Date(skill.updated_at).toLocaleDateString(undefined, {
              year: "numeric",
              month: "short",
              day: "numeric",
            })}
          </span>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// SkillsPage  (main export)
// ---------------------------------------------------------------------------

export function SkillsPage() {
  const queryClient = useQueryClient();

  const [tab, setTab] = useState<Tab>("installed");
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [installedSearch, setInstalledSearch] = useState("");
  const [notification, setNotification] = useState<{
    type: "success" | "error";
    message: string;
  } | null>(null);
  const [installingSkill, setInstallingSkill] = useState<string | null>(null);

  // track whether we already auto-refreshed this session
  const autoRefreshed = useRef(false);

  // Notification helper with auto-dismiss
  const notify = useCallback(
    (type: "success" | "error", message: string) => {
      setNotification({ type, message });
      setTimeout(() => setNotification(null), 3000);
    },
    [],
  );

  // Debounce search input
  useEffect(() => {
    const timer = setTimeout(() => {
      setDebouncedQuery(searchQuery);
    }, 400);
    return () => clearTimeout(timer);
  }, [searchQuery]);

  // Clear selection when switching tabs
  useEffect(() => {
    setSelectedSkill(null);
  }, [tab]);

  // ---- Queries ----

  const { data: installed = [], isLoading: installedLoading } = useQuery({
    queryKey: ["skills"],
    queryFn: api.skills.list,
  });

  const installedNames = new Set(installed.map((s) => s.name));

  const { data: catalog, isLoading: catalogLoading } = useQuery({
    queryKey: ["catalog", debouncedQuery],
    queryFn: () => api.skills.catalog(debouncedQuery, 0, 50),
    enabled: tab === "browse",
  });

  // ---- Mutations ----

  const refresh = useMutation({
    mutationFn: () => api.skills.refresh(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["catalog"] });
    },
  });

  const quickInstall = useMutation({
    mutationFn: ({ name, version }: { name: string; version?: string }) =>
      api.skills.install(name, version),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      notify("success", `Installed "${variables.name}" successfully`);
      setInstallingSkill(null);
    },
    onError: (err: Error, variables) => {
      notify("error", `Failed to install "${variables.name}": ${err.message}`);
      setInstallingSkill(null);
    },
  });

  // Auto-refresh when browse tab opened and catalog is empty
  const refreshMutate = refresh.mutate;
  const refreshPending = refresh.isPending;
  useEffect(() => {
    if (
      tab === "browse" &&
      catalog &&
      !catalog.cached &&
      catalog.total === 0 &&
      !autoRefreshed.current &&
      !refreshPending
    ) {
      autoRefreshed.current = true;
      refreshMutate();
    }
  }, [tab, catalog, refreshMutate, refreshPending]);

  // ---- Filtered installed skills ----
  const filteredInstalled = installedSearch
    ? installed.filter((s) =>
        s.name.toLowerCase().includes(installedSearch.toLowerCase()),
      )
    : installed;

  // ---- Render helpers ----

  const renderInstalledGrid = () => {
    if (installedLoading) {
      return (
        <div className="flex justify-center py-16">
          <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
        </div>
      );
    }

    if (installed.length === 0) {
      return (
        <div className="rounded-xl border border-border bg-card p-8">
          <div className="flex flex-col items-center justify-center py-16">
            <Puzzle className="w-12 h-12 text-muted-foreground mb-4" />
            <p className="text-muted-foreground">No skills installed</p>
            <p className="text-sm text-muted-foreground mt-1">
              Browse ClawHub to find and install skills
            </p>
          </div>
        </div>
      );
    }

    return (
      <div className="space-y-4">
        {/* Search installed skills */}
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
          <input
            value={installedSearch}
            onChange={(e) => setInstalledSearch(e.target.value)}
            placeholder="Filter installed skills..."
            className="w-full pl-10 pr-4 py-2 rounded-lg bg-card border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
          />
        </div>

        {filteredInstalled.length === 0 ? (
          <p className="text-sm text-muted-foreground text-center py-8">
            No installed skills matching &ldquo;{installedSearch}&rdquo;
          </p>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {filteredInstalled.map((skill: InstalledSkill) => (
              <SkillCard
                key={skill.name}
                name={skill.name}
                version={skill.version}
                subtitle={`${sourceLabel(skill.source)} · Installed ${new Date(skill.installed_at).toLocaleDateString()}`}
                isInstalled
                isSelected={selectedSkill === skill.name}
                onClick={() =>
                  setSelectedSkill((prev) =>
                    prev === skill.name ? null : skill.name,
                  )
                }
              />
            ))}
          </div>
        )}
      </div>
    );
  };

  const renderBrowseTab = () => {
    const skills = catalog?.skills ?? [];
    const lastRefreshed = catalog?.last_refreshed ?? null;
    const total = catalog?.total ?? 0;

    return (
      <div className="space-y-4">
        {/* Search + Refresh row */}
        <div className="flex items-center gap-3">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Filter skills..."
              className="w-full pl-10 pr-4 py-2 rounded-lg bg-card border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          </div>

          <button
            onClick={() => refresh.mutate()}
            disabled={refresh.isPending}
            className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-card border border-border text-sm hover:bg-muted transition-colors disabled:opacity-50 shrink-0"
            title="Refresh catalog from ClawHub"
          >
            {refresh.isPending ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <RefreshCw className="w-4 h-4" />
            )}
            Refresh
          </button>

          <span className="text-xs text-muted-foreground whitespace-nowrap shrink-0">
            {formatTimeAgo(lastRefreshed)}
          </span>
        </div>

        {/* Catalog count */}
        {debouncedQuery && !catalogLoading && skills.length > 0 && (
          <p className="text-xs text-muted-foreground">
            Showing {skills.length} of {total} skills
          </p>
        )}

        {/* Content */}
        {catalogLoading && (
          <div className="flex justify-center py-16">
            <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
          </div>
        )}

        {!catalogLoading && skills.length === 0 && (
          <div className="rounded-xl border border-border bg-card p-8">
            <div className="flex flex-col items-center justify-center py-16">
              <RefreshCw className="w-12 h-12 text-muted-foreground mb-4" />
              <p className="text-muted-foreground">No skills in catalog</p>
              <p className="text-sm text-muted-foreground mt-1">
                Click <strong>Refresh</strong> to sync from ClawHub
              </p>
            </div>
          </div>
        )}

        {!catalogLoading && skills.length > 0 && (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {skills.map((skill: ClawHubSkillMeta) => (
              <SkillCard
                key={skill.name}
                name={skill.name}
                version={skill.version}
                subtitle={`by ${skill.author} · ${skill.downloads.toLocaleString()} downloads`}
                isInstalled={installedNames.has(skill.name)}
                isSelected={selectedSkill === skill.name}
                onClick={() =>
                  setSelectedSkill((prev) =>
                    prev === skill.name ? null : skill.name,
                  )
                }
                onInstall={() => {
                  setInstallingSkill(skill.name);
                  quickInstall.mutate({
                    name: skill.name,
                    version: skill.version,
                  });
                }}
                installPending={installingSkill === skill.name}
              />
            ))}
          </div>
        )}
      </div>
    );
  };

  // ---- Main render ----

  const catalogTotal = catalog?.total ?? 0;

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Skills</h1>

      {/* Notification banner */}
      {notification && <NotificationBanner notification={notification} />}

      {/* Tab bar */}
      <div className="flex gap-4 border-b border-border">
        <button
          onClick={() => setTab("installed")}
          className={`pb-2 text-sm font-medium border-b-2 transition-colors ${
            tab === "installed"
              ? "border-primary text-foreground"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          Installed ({installed.length})
        </button>
        <button
          onClick={() => setTab("browse")}
          className={`pb-2 text-sm font-medium border-b-2 transition-colors ${
            tab === "browse"
              ? "border-primary text-foreground"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          Browse ClawHub ({catalogTotal})
        </button>
      </div>

      {/* Tab content */}
      {tab === "installed" ? renderInstalledGrid() : renderBrowseTab()}

      {/* Detail panel (shown below grid when a skill is selected) */}
      {selectedSkill && (
        <SkillDetail
          name={selectedSkill}
          onBack={() => setSelectedSkill(null)}
          onNotify={notify}
        />
      )}
    </div>
  );
}
