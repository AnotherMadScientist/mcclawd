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
  Puzzle,
  X,
  Plus,
} from "lucide-react";
import { api } from "../api/client";
import { getToken } from "../api/client";
import type { InstalledSkill, ClawHubSkillMeta } from "../api/types";

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
// Browse card (catalog grid)
// ---------------------------------------------------------------------------

function BrowseCard({
  skill,
  isInstalled,
  isSelected,
  onClick,
  onInstall,
  installPending,
}: {
  skill: ClawHubSkillMeta;
  isInstalled: boolean;
  isSelected: boolean;
  onClick: () => void;
  onInstall: () => void;
  installPending: boolean;
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
            <p className="text-sm font-medium truncate">{skill.name}</p>
            {isInstalled && (
              <Check className="w-3 h-3 text-emerald-400 shrink-0" />
            )}
          </div>
          <p className="text-xs text-muted-foreground truncate">
            {skill.author ? `${skill.author} \u00b7 ` : ""}v{skill.version}
          </p>
          {skill.description && (
            <p className="text-xs text-muted-foreground/70 mt-1 line-clamp-2">
              {skill.description}
            </p>
          )}
        </div>
        {!isInstalled && (
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
        )}
      </div>
    </button>
  );
}

// ---------------------------------------------------------------------------
// Installed skill row (compact sidebar item)
// ---------------------------------------------------------------------------

function InstalledRow({
  skill,
  isSelected,
  onClick,
  onUninstall,
  uninstallPending,
}: {
  skill: InstalledSkill;
  isSelected: boolean;
  onClick: () => void;
  onUninstall: () => void;
  uninstallPending: boolean;
}) {
  return (
    <div
      onClick={onClick}
      className={`group flex items-center gap-2 px-2.5 py-2 rounded-lg cursor-pointer transition-colors ${
        isSelected
          ? "bg-primary/10 border border-primary/30"
          : "hover:bg-muted/50 border border-transparent"
      }`}
    >
      <Check className="w-3 h-3 text-emerald-400 shrink-0" />
      <div className="min-w-0 flex-1">
        <p className="text-xs font-medium truncate">{skill.name}</p>
        <p className="text-[10px] text-muted-foreground">v{skill.version}</p>
      </div>
      <button
        onClick={(e) => {
          e.stopPropagation();
          onUninstall();
        }}
        disabled={uninstallPending}
        className="p-0.5 rounded opacity-0 group-hover:opacity-100 hover:bg-red-500/10 transition-all disabled:opacity-50 shrink-0"
        title="Uninstall"
      >
        {uninstallPending ? (
          <Loader2 className="w-3 h-3 animate-spin text-muted-foreground" />
        ) : (
          <X className="w-3 h-3 text-muted-foreground hover:text-red-400" />
        )}
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// SKILL.md template for "Create Skill"
// ---------------------------------------------------------------------------

const SKILL_TEMPLATE = `---
name: my-awesome-skill
version: 1.0.0
author: your-name
description: A brief one-liner describing what this skill does
tags:
  - productivity
  - automation
---

# My Awesome Skill

## Purpose
Describe the high-level goal of this skill. What problem does it solve?
When should an agent use it?

## Instructions
Step-by-step instructions for how the agent should behave when this
skill is active. Be specific and actionable.

1. First, analyze the user's request to determine if this skill applies
2. Gather any required context (files, URLs, data)
3. Execute the core task using the tools available
4. Verify the output meets quality standards
5. Present results clearly to the user

## Tools Required
- \`filesystem\` — read/write files in the workspace
- \`web_search\` — look up documentation or references

## Examples

### Example 1: Basic usage
**User:** "Run my-awesome-skill on this project"
**Agent:** Analyzes the project structure, applies the skill logic,
and reports findings.

### Example 2: With options
**User:** "Run my-awesome-skill with verbose output"
**Agent:** Same as above but includes detailed step-by-step reporting.

## Configuration
Optional settings that can be customized:
- \`verbosity\`: "normal" | "verbose" | "quiet" (default: "normal")
- \`max_files\`: Maximum files to process (default: 50)
`;

// ---------------------------------------------------------------------------
// Create Skill Dialog — editable with real-time section annotations
// ---------------------------------------------------------------------------

function CreateSkillDialog({ onClose }: { onClose: () => void }) {
  const [text, setText] = useState(SKILL_TEMPLATE);
  const sections = parseSkillSections(text);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-6" onClick={onClose}>
      <div className="bg-card border border-border rounded-2xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col" onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-border shrink-0">
          <div>
            <h2 className="text-lg font-semibold">Create a Skill</h2>
            <p className="text-xs text-muted-foreground mt-0.5">Edit the template — colored bars show SKILL.md sections</p>
          </div>
          <button onClick={onClose} className="p-1.5 rounded-lg hover:bg-muted transition-colors">
            <X className="w-5 h-5 text-muted-foreground" />
          </button>
        </div>

        {/* Body — textarea with overlaid section bars, both sharing exact line-height */}
        <div className="flex-1 overflow-y-auto">
          <div className="flex" style={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace", fontSize: "13px", lineHeight: "20px" }}>
            <textarea
              value={text}
              onChange={(e) => setText(e.target.value)}
              className="flex-1 bg-transparent text-foreground/90 resize-none focus:outline-none"
              style={{ padding: "16px 24px", fontFamily: "inherit", fontSize: "inherit", lineHeight: "inherit" }}
              spellCheck={false}
              rows={text.split("\n").length + 2}
            />
            {/* Section bars — pixel-matched to textarea lines */}
            <div className="w-28 shrink-0 border-l border-border/10" style={{ paddingTop: "16px" }}>
              {sections.map((section, i) => (
                <div
                  key={`${section.key}-${i}`}
                  className="flex items-stretch"
                  style={{ height: `${section.lines.length * 20}px` }}
                >
                  <div className={`w-1 ${section.color.bg} shrink-0`} />
                  <div className="flex items-start px-2 pt-0.5">
                    <span className={`text-[10px] font-semibold uppercase tracking-wider ${section.color.text} whitespace-nowrap`} style={{ fontFamily: "system-ui, sans-serif" }}>
                      {section.label}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-3 border-t border-border shrink-0">
          <p className="text-xs text-muted-foreground">
            Save as <code className="px-1 py-0.5 bg-muted rounded">SKILL.md</code> in your skill directory
          </p>
          <div className="flex gap-2">
            <button
              onClick={() => navigator.clipboard.writeText(text)}
              className="px-3 py-1.5 rounded-lg bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition-opacity"
            >
              Copy to Clipboard
            </button>
            <button onClick={onClose} className="px-3 py-1.5 rounded-lg bg-muted text-xs font-medium hover:bg-muted/80 transition-colors">
              Close
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// SKILL.md section color map (maps ## header names to colors)
// ---------------------------------------------------------------------------

const SECTION_COLORS: Record<string, { bg: string; text: string }> = {
  frontmatter: { bg: "bg-orange-400", text: "text-orange-400" },
  title: { bg: "bg-blue-400", text: "text-blue-400" },
  purpose: { bg: "bg-emerald-400", text: "text-emerald-400" },
  description: { bg: "bg-emerald-400", text: "text-emerald-400" },
  instructions: { bg: "bg-violet-400", text: "text-violet-400" },
  context: { bg: "bg-violet-400", text: "text-violet-400" },
  tools: { bg: "bg-yellow-400", text: "text-yellow-400" },
  "tools required": { bg: "bg-yellow-400", text: "text-yellow-400" },
  "mcp tools": { bg: "bg-yellow-400", text: "text-yellow-400" },
  examples: { bg: "bg-rose-400", text: "text-rose-400" },
  configuration: { bg: "bg-cyan-400", text: "text-cyan-400" },
  config: { bg: "bg-cyan-400", text: "text-cyan-400" },
  install: { bg: "bg-amber-400", text: "text-amber-400" },
  reference: { bg: "bg-indigo-400", text: "text-indigo-400" },
};

const DEFAULT_SECTION_COLOR = { bg: "bg-slate-400", text: "text-slate-400" };

type SectionColor = { bg: string; text: string };

function sectionColor(key: string): SectionColor {
  return SECTION_COLORS[key] ?? DEFAULT_SECTION_COLOR;
}

/** Parse SKILL.md text into annotated sections for rendering. */
function parseSkillSections(content: string) {
  const lines = content.split("\n");
  const sections: { label: string; key: string; lines: string[]; color: SectionColor }[] = [];

  let inFrontmatter = false;
  let frontmatterDone = false;
  let currentSection: (typeof sections)[0] | null = null;

  for (const line of lines) {
    // YAML frontmatter detection
    if (!frontmatterDone && line.trimEnd() === "---") {
      if (!inFrontmatter) {
        inFrontmatter = true;
        currentSection = { label: "Frontmatter", key: "frontmatter", lines: [line], color: sectionColor("frontmatter") };
        sections.push(currentSection);
        continue;
      } else {
        currentSection!.lines.push(line);
        inFrontmatter = false;
        frontmatterDone = true;
        currentSection = null;
        continue;
      }
    }

    if (inFrontmatter) {
      currentSection!.lines.push(line);
      continue;
    }

    // # Title (h1)
    if (line.startsWith("# ") && !line.startsWith("## ")) {
      currentSection = { label: "Title", key: "title", lines: [line], color: sectionColor("title") };
      sections.push(currentSection);
      continue;
    }

    // ## Section headers
    if (line.startsWith("## ")) {
      const headerText = line.replace(/^#+\s*/, "").trim();
      const key = headerText.toLowerCase();
      currentSection = { label: headerText, key, lines: [line], color: sectionColor(key) };
      sections.push(currentSection);
      continue;
    }

    // ### Sub-section headers stay within parent section
    if (currentSection) {
      currentSection.lines.push(line);
    } else {
      // Lines before any section (between frontmatter and first heading)
      if (line.trim()) {
        currentSection = { label: "Preamble", key: "preamble", lines: [line], color: sectionColor("title") };
        sections.push(currentSection);
      }
    }
  }

  return sections;
}

/** Generate a stub SKILL.md from metadata when real content isn't available. */
function generateStubSkillMd(skill: ClawHubSkillMeta): string {
  const tagsLine = skill.tags.length > 0 ? `tags:\n${skill.tags.map((t) => `  - ${t}`).join("\n")}\n` : "";
  return `---
name: ${skill.name}
version: ${skill.version}
author: ${skill.author || "unknown"}
description: ${skill.description}
${tagsLine}---

# ${skill.name}

## Description
${skill.description || "No description available."}

## Instructions
Install this skill to see the full instructions.

## Tools Required
Available after installation.

## Examples
Available after installation.
`;
}

// ---------------------------------------------------------------------------
// Skill Detail Dialog (large modal with full SKILL.md preview)
// ---------------------------------------------------------------------------

function SkillDetailDialog({
  name,
  onClose,
  onNotify,
}: {
  name: string;
  onClose: () => void;
  onNotify: (type: "success" | "error", message: string) => void;
}) {
  const queryClient = useQueryClient();

  const { data: installed = [] } = useQuery({
    queryKey: ["skills"],
    queryFn: api.skills.list,
  });
  const installedInfo = installed.find((s) => s.name === name);

  const { data: skill, isLoading: metaLoading } = useQuery({
    queryKey: ["skill-detail", name],
    queryFn: () => api.skills.detail(name).catch(() => null),
  });

  // Fetch full SKILL.md content (only succeeds for installed skills)
  const { data: skillContent } = useQuery({
    queryKey: ["skill-content", name],
    queryFn: () => api.skills.content(name).then((r) => r.content).catch(() => null),
  });

  const install = useMutation({
    mutationFn: () => api.skills.install(name, skill?.version),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      queryClient.invalidateQueries({ queryKey: ["skill-content", name] });
      onNotify("success", `Installed "${name}"`);
    },
    onError: (err: Error) => onNotify("error", `Install failed: ${err.message}`),
  });

  const uninstall = useMutation({
    mutationFn: () => api.skills.uninstall(name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      onNotify("success", `Uninstalled "${name}"`);
      onClose();
    },
    onError: (err: Error) => onNotify("error", `Uninstall failed: ${err.message}`),
  });

  // Parse the SKILL.md content (real or generated stub)
  const mdText = skillContent ?? (skill ? generateStubSkillMd(skill) : null);
  const sections = mdText ? parseSkillSections(mdText) : [];
  const isStub = !skillContent;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-6" onClick={onClose}>
      <div
        className="bg-card border border-border rounded-2xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-border shrink-0">
          <div className="flex items-center gap-3 min-w-0">
            <Package className="w-5 h-5 text-violet-400 shrink-0" />
            <div className="min-w-0">
              <h2 className="text-lg font-semibold truncate">{name}</h2>
              <div className="flex items-center gap-3 text-xs text-muted-foreground">
                {skill && (
                  <>
                    <span>v{skill.version}</span>
                    {skill.author && <span>by {skill.author}</span>}
                    <span>{skill.downloads.toLocaleString()} downloads</span>
                    <span>
                      Updated{" "}
                      {new Date(skill.updated_at).toLocaleDateString(undefined, {
                        year: "numeric",
                        month: "short",
                        day: "numeric",
                      })}
                    </span>
                  </>
                )}
                {installedInfo && (
                  <span className="text-emerald-400 font-medium flex items-center gap-1">
                    <Check className="w-3 h-3" />
                    Installed
                  </span>
                )}
              </div>
            </div>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            {skill?.tags && skill.tags.length > 0 && (
              <div className="hidden sm:flex items-center gap-1 mr-2">
                {skill.tags.slice(0, 3).map((tag) => (
                  <span
                    key={tag}
                    className="px-2 py-0.5 rounded-full bg-violet-400/10 text-violet-300 border border-violet-400/20 text-[10px]"
                  >
                    {tag}
                  </span>
                ))}
                {skill.tags.length > 3 && (
                  <span className="text-[10px] text-muted-foreground">+{skill.tags.length - 3}</span>
                )}
              </div>
            )}
            {installedInfo ? (
              <button
                onClick={() => uninstall.mutate()}
                disabled={uninstall.isPending}
                className="px-3 py-1.5 rounded-lg bg-red-500/10 text-red-400 text-xs font-medium hover:bg-red-500/20 transition-colors disabled:opacity-50 flex items-center gap-1.5"
              >
                {uninstall.isPending ? <Loader2 className="w-3 h-3 animate-spin" /> : <Trash2 className="w-3 h-3" />}
                Uninstall
              </button>
            ) : (
              <button
                onClick={() => install.mutate()}
                disabled={install.isPending}
                className="px-3 py-1.5 rounded-lg bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition-opacity disabled:opacity-50 flex items-center gap-1.5"
              >
                {install.isPending ? <Loader2 className="w-3 h-3 animate-spin" /> : <Download className="w-3 h-3" />}
                Install
              </button>
            )}
            <button onClick={onClose} className="p-1.5 rounded-lg hover:bg-muted transition-colors">
              <X className="w-5 h-5 text-muted-foreground" />
            </button>
          </div>
        </div>

        {/* Body — full SKILL.md with section annotations */}
        <div className="flex-1 overflow-y-auto">
          {metaLoading && (
            <div className="flex justify-center py-16">
              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
            </div>
          )}

          {!metaLoading && !skill && (
            <p className="text-muted-foreground text-sm text-center py-12">
              Skill details not available. Try refreshing the catalog.
            </p>
          )}

          {!metaLoading && skill && sections.length > 0 && (
            <div className="font-mono text-[13px] leading-relaxed">
              {isStub && (
                <div className="px-6 py-2 bg-amber-500/5 border-b border-amber-500/20 text-amber-400 text-xs font-sans">
                  Preview from catalog metadata — install to see full skill content
                </div>
              )}
              {sections.map((section, i) => (
                <div
                  key={`${section.key}-${i}`}
                  className={`flex border-b border-border/10 ${i % 2 === 0 ? "" : "bg-muted/20"}`}
                >
                  {/* Skill text */}
                  <pre className="flex-1 px-6 py-3 whitespace-pre-wrap text-foreground/85 overflow-x-auto">
                    {section.lines.join("\n")}
                  </pre>
                  {/* Section annotation bar (RHS) */}
                  <div className="w-28 shrink-0 flex items-stretch border-l border-border/10">
                    <div className={`w-1 ${section.color.bg} shrink-0`} />
                    <div className="flex items-start py-3 px-2">
                      <span className={`text-[10px] font-sans font-semibold uppercase tracking-wider ${section.color.text} whitespace-nowrap`}>
                        {section.label}
                      </span>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-3 border-t border-border shrink-0">
          <p className="text-xs text-muted-foreground">
            {skillContent
              ? `${sections.length} sections · ${skillContent.split("\n").length} lines`
              : "Catalog preview"}
          </p>
          {skillContent && (
            <button
              onClick={() => navigator.clipboard.writeText(skillContent)}
              className="px-3 py-1.5 rounded-lg bg-muted text-xs font-medium hover:bg-muted/80 transition-colors"
            >
              Copy SKILL.md
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// SkillsPage (main export) — unified single-page layout
// ---------------------------------------------------------------------------

export function SkillsPage() {
  const queryClient = useQueryClient();

  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [notification, setNotification] = useState<{
    type: "success" | "error";
    message: string;
  } | null>(null);
  const [installingSkill, setInstallingSkill] = useState<string | null>(null);
  const [uninstallingSkill, setUninstallingSkill] = useState<string | null>(null);
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [syncProgress, setSyncProgress] = useState<number | null>(null);

  const autoRefreshed = useRef(false);

  const notify = useCallback((type: "success" | "error", message: string) => {
    setNotification({ type, message });
    setTimeout(() => setNotification(null), 3000);
  }, []);

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(searchQuery), 400);
    return () => clearTimeout(timer);
  }, [searchQuery]);

  // ---- Queries ----

  const { data: installed = [], isLoading: installedLoading } = useQuery({
    queryKey: ["skills"],
    queryFn: api.skills.list,
  });

  const installedNames = new Set(installed.map((s) => s.name));

  const { data: catalog, isLoading: catalogLoading } = useQuery({
    queryKey: ["catalog", debouncedQuery],
    queryFn: () => api.skills.catalog(debouncedQuery, 0, 50),
    refetchInterval: syncProgress !== null ? 2000 : false,
  });

  // ---- Mutations ----

  const quickInstall = useMutation({
    mutationFn: ({ name, version }: { name: string; version?: string }) =>
      api.skills.install(name, version),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      notify("success", `Installed "${variables.name}"`);
      setInstallingSkill(null);
    },
    onError: (err: Error) => {
      notify("error", `Install failed: ${err.message}`);
      setInstallingSkill(null);
    },
  });

  const quickUninstall = useMutation({
    mutationFn: (name: string) => api.skills.uninstall(name),
    onSuccess: (_data, name) => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      notify("success", `Uninstalled "${name}"`);
      setUninstallingSkill(null);
      if (selectedSkill === name) setSelectedSkill(null);
    },
    onError: (err: Error) => {
      notify("error", `Uninstall failed: ${err.message}`);
      setUninstallingSkill(null);
    },
  });

  // ---- SSE Refresh ----

  const startSync = useCallback(() => {
    setSyncProgress(0);
    const token = getToken();
    const url = `/api/skills/refresh-stream${token ? `?token=${token}` : ""}`;
    const es = new EventSource(url);
    es.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        if (data.fetched) {
          setSyncProgress(data.fetched);
          queryClient.invalidateQueries({ queryKey: ["catalog"] });
        }
        if (data.done) {
          setSyncProgress(null);
          queryClient.invalidateQueries({ queryKey: ["catalog"] });
          notify("success", `Synced ${data.total.toLocaleString()} skills from ClawHub`);
          es.close();
        }
      } catch {
        // ignore parse errors
      }
    };
    es.onerror = () => {
      setSyncProgress(null);
      notify("error", "Sync connection lost");
      es.close();
    };
  }, [queryClient, notify]);

  // Auto-refresh catalog on first load if empty
  useEffect(() => {
    if (catalog && !catalog.cached && catalog.total === 0 && !autoRefreshed.current && syncProgress === null) {
      autoRefreshed.current = true;
      startSync();
    }
  }, [catalog, syncProgress, startSync]);

  // ---- Derived ----

  const skills = catalog?.skills ?? [];
  const lastRefreshed = catalog?.last_refreshed ?? null;
  const catalogTotal = catalog?.total ?? 0;

  // ---- Render ----

  return (
    <div className="max-w-6xl mx-auto space-y-4">
      {/* Header row */}
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Skills</h1>
        <div className="flex items-center gap-2">
          {syncProgress !== null ? (
            <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-primary/10 border border-primary/20 text-xs">
              <Loader2 className="w-3.5 h-3.5 animate-spin text-primary" />
              <span className="text-primary font-medium">
                Syncing... {syncProgress.toLocaleString()} skills
              </span>
            </div>
          ) : (
            <>
              <span className="text-xs text-muted-foreground">
                {formatTimeAgo(lastRefreshed)}
              </span>
              <button
                onClick={startSync}
                className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-card border border-border text-xs hover:bg-muted transition-colors"
                title="Sync catalog from ClawHub"
              >
                <RefreshCw className="w-3.5 h-3.5" />
                Sync
              </button>
            </>
          )}
          <button
            onClick={() => setShowCreateDialog(true)}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition-opacity"
          >
            <Plus className="w-3.5 h-3.5" />
            Create
          </button>
        </div>
      </div>

      {notification && <NotificationBanner notification={notification} />}

      {/* Main layout: browse left, installed right */}
      <div className="flex gap-4">
        {/* Left: Browse catalog */}
        <div className="flex-1 min-w-0 space-y-3">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search skills..."
              className="w-full pl-10 pr-4 py-2 rounded-lg bg-card border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          </div>

          {!catalogLoading && skills.length > 0 && (
            <p className="text-xs text-muted-foreground">
              {debouncedQuery ? `${skills.length} of ${catalogTotal} skills` : `${catalogTotal.toLocaleString()} skills`}
            </p>
          )}

          {catalogLoading && (
            <div className="flex justify-center py-16">
              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
            </div>
          )}

          {!catalogLoading && skills.length === 0 && (
            <div className="rounded-xl border border-border bg-card p-8">
              <div className="flex flex-col items-center justify-center py-12">
                <RefreshCw className="w-10 h-10 text-muted-foreground mb-3" />
                <p className="text-muted-foreground text-sm">No skills in catalog</p>
                <p className="text-xs text-muted-foreground mt-1">
                  Click <strong>Sync</strong> to fetch from ClawHub
                </p>
              </div>
            </div>
          )}

          {!catalogLoading && skills.length > 0 && (
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-2.5">
              {skills.map((skill: ClawHubSkillMeta) => (
                <BrowseCard
                  key={skill.name}
                  skill={skill}
                  isInstalled={installedNames.has(skill.name)}
                  isSelected={selectedSkill === skill.name}
                  onClick={() => setSelectedSkill(skill.name)}
                  onInstall={() => {
                    setInstallingSkill(skill.name);
                    quickInstall.mutate({ name: skill.name, version: skill.version });
                  }}
                  installPending={installingSkill === skill.name}
                />
              ))}
            </div>
          )}
        </div>

        {/* Right: Installed sidebar */}
        <div className="w-56 shrink-0">
          <div className="rounded-xl border border-border bg-card p-3 sticky top-4">
            <div className="flex items-center justify-between mb-2">
              <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                Installed
              </h2>
              <span className="text-[10px] tabular-nums text-muted-foreground bg-muted px-1.5 py-0.5 rounded-full">
                {installed.length}
              </span>
            </div>

            {installedLoading && (
              <div className="flex justify-center py-4">
                <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" />
              </div>
            )}

            {!installedLoading && installed.length === 0 && (
              <div className="flex flex-col items-center py-6 text-center">
                <Puzzle className="w-8 h-8 text-muted-foreground/40 mb-2" />
                <p className="text-xs text-muted-foreground">None yet</p>
              </div>
            )}

            {!installedLoading && installed.length > 0 && (
              <div className="space-y-0.5 max-h-[60vh] overflow-y-auto">
                {installed.map((skill: InstalledSkill) => (
                  <InstalledRow
                    key={skill.name}
                    skill={skill}
                    isSelected={selectedSkill === skill.name}
                    onClick={() => setSelectedSkill(skill.name)}
                    onUninstall={() => {
                      setUninstallingSkill(skill.name);
                      quickUninstall.mutate(skill.name);
                    }}
                    uninstallPending={uninstallingSkill === skill.name}
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Dialogs */}
      {selectedSkill && (
        <SkillDetailDialog
          name={selectedSkill}
          onClose={() => setSelectedSkill(null)}
          onNotify={notify}
        />
      )}
      {showCreateDialog && (
        <CreateSkillDialog onClose={() => setShowCreateDialog(false)} />
      )}
    </div>
  );
}
