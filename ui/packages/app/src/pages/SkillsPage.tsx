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
  Sparkles,
  Wand2,
  Shield,
  ShieldAlert,
  ShieldCheck,
  ShieldQuestion,
} from "lucide-react";
import { api } from "../api/client";
import { getToken } from "../api/client";
import type { InstalledSkill, ClawHubSkillMeta, ScanResult } from "../api/types";
import { TiptapSkillEditor } from "../components/TiptapSkillEditor";

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

// Tag-based color palette for the top border accent
const TAG_COLORS: Record<string, string> = {
  ai: "from-violet-500/60",
  llm: "from-violet-500/60",
  ml: "from-blue-500/60",
  data: "from-cyan-500/60",
  devops: "from-orange-500/60",
  security: "from-red-500/60",
  web: "from-emerald-500/60",
  api: "from-sky-500/60",
  database: "from-amber-500/60",
  default: "from-primary/40",
};

function getTagColor(tags: string[]): string {
  const fallback = TAG_COLORS["default"] || "from-primary/40";
  if (!tags || tags.length === 0) return fallback;
  for (const t of tags) {
    const color = TAG_COLORS[t.toLowerCase()];
    if (color) return color;
  }
  return fallback;
}

// ---------------------------------------------------------------------------
// Security status badge
// ---------------------------------------------------------------------------

function SecurityBadge({ scanResult }: { scanResult?: ScanResult | null }) {
  if (!scanResult || scanResult.status === "NotScanned") {
    return (
      <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full bg-muted text-[10px] text-muted-foreground leading-none">
        <ShieldQuestion className="w-2.5 h-2.5" />
        Not Scanned
      </span>
    );
  }

  if (scanResult.status === "Pass") {
    return (
      <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full bg-emerald-500/10 text-[10px] text-emerald-400 border border-emerald-500/20 leading-none">
        <ShieldCheck className="w-2.5 h-2.5" />
        Safe
      </span>
    );
  }

  if (scanResult.status === "Warning") {
    return (
      <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full bg-yellow-500/10 text-[10px] text-yellow-400 border border-yellow-500/20 leading-none">
        <Shield className="w-2.5 h-2.5" />
        Warning
      </span>
    );
  }

  // Critical
  return (
    <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full bg-red-500/10 text-[10px] text-red-400 border border-red-500/20 leading-none">
      <ShieldAlert className="w-2.5 h-2.5" />
      Critical
    </span>
  );
}

function BrowseCard({
  skill,
  isInstalled,
  isSelected,
  onClick,
  onInstall,
  installPending,
  scanResult,
  onScan,
  scanPending,
}: {
  skill: ClawHubSkillMeta;
  isInstalled: boolean;
  isSelected: boolean;
  onClick: () => void;
  onInstall: () => void;
  installPending: boolean;
  scanResult?: ScanResult | null;
  onScan: () => void;
  scanPending: boolean;
}) {
  const visibleTags = skill.tags ? skill.tags.slice(0, 3) : [];
  const accentColor = getTagColor(skill.tags);

  return (
    <button
      onClick={onClick}
      className={`relative rounded-xl bg-card border transition-colors text-left w-full overflow-hidden flex flex-col ${
        isSelected
          ? "border-primary ring-1 ring-primary/30"
          : "border-border hover:border-primary/40"
      }`}
    >
      {/* Top gradient accent bar */}
      <div
        className={`h-0.5 w-full bg-gradient-to-r ${accentColor} to-transparent`}
      />

      <div className="p-3 flex flex-col gap-2 flex-1">
        {/* Header row: icon + name + installed check + install button */}
        <div className="flex items-start gap-2.5">
          <div className="w-8 h-8 rounded-full bg-violet-500/15 flex items-center justify-center shrink-0">
            <Package className="w-4 h-4 text-violet-400" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              <p className="text-sm font-semibold truncate leading-tight">
                {skill.name}
              </p>
              {isInstalled && (
                <Check className="w-3 h-3 text-emerald-400 shrink-0" />
              )}
            </div>
            <p className="text-[11px] text-muted-foreground truncate mt-0.5">
              {skill.author ? `${skill.author} · ` : ""}v{skill.version}
            </p>
          </div>
          <div className="flex items-center gap-0.5 shrink-0">
            {/* Scan button */}
            {!scanResult && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onScan();
                }}
                disabled={scanPending}
                className="p-1 rounded-md hover:bg-muted transition-colors disabled:opacity-50"
                title="Scan for security issues"
              >
                {scanPending ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin text-muted-foreground" />
                ) : (
                  <Shield className="w-3.5 h-3.5 text-muted-foreground/50 hover:text-muted-foreground" />
                )}
              </button>
            )}
            {/* Install button */}
            {!isInstalled && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onInstall();
                }}
                disabled={installPending}
                className="p-1 rounded-md hover:bg-muted transition-colors disabled:opacity-50"
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
        </div>

        {/* Description */}
        {skill.description && (
          <p className="text-xs text-muted-foreground/70 line-clamp-2 leading-relaxed">
            {skill.description}
          </p>
        )}

        {/* Footer: tags + security badge + download count */}
        <div className="flex items-center justify-between gap-2 mt-auto pt-1">
          <div className="flex flex-wrap gap-1 min-w-0 items-center">
            {visibleTags.map((tag) => (
              <span
                key={tag}
                className="bg-muted text-[10px] px-1.5 py-0.5 rounded-full text-muted-foreground leading-none"
              >
                {tag}
              </span>
            ))}
            {scanResult && <SecurityBadge scanResult={scanResult} />}
          </div>
          {skill.downloads > 0 && (
            <span className="flex items-center gap-0.5 text-[10px] text-muted-foreground/60 shrink-0">
              <Download className="w-2.5 h-2.5" />
              {skill.downloads >= 1000
                ? `${(skill.downloads / 1000).toFixed(1)}k`
                : skill.downloads}
            </span>
          )}
        </div>
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
  scanResult,
}: {
  skill: InstalledSkill;
  isSelected: boolean;
  onClick: () => void;
  onUninstall: () => void;
  uninstallPending: boolean;
  scanResult?: ScanResult | null;
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
        <div className="flex items-center gap-1">
          <p className="text-xs font-medium truncate">{skill.name}</p>
          {scanResult && <SecurityBadge scanResult={scanResult} />}
        </div>
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
// AI skill generation helper
// ---------------------------------------------------------------------------

function slugify(name: string): string {
  return name
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function generateSkillFromDescription(name: string, description: string): string {
  const slug = slugify(name) || "my-skill";
  return `---
name: ${slug}
version: 0.1.0
author: me
description: ${description}
tags:
  - custom
---

# ${slug}

## Description
${description}

## Instructions
[Describe step-by-step how this skill works]

## Tools Required
[List any MCP tools this skill needs]

## Examples
\`\`\`
User: [example prompt]
Assistant: [example response]
\`\`\`

## Configuration
[Any configuration options]
`;
}

function parseSkillName(text: string): string {
  const match = text.match(/^name:\s*(.+)$/m);
  return match && match[1] ? slugify(match[1].trim()) : "";
}

// ---------------------------------------------------------------------------
// Create Skill Dialog — description-first AI flow + editable with section bars
// ---------------------------------------------------------------------------

function CreateSkillDialog({ onClose }: { onClose: () => void }) {
  const [mode, setMode] = useState<"describe" | "edit">("describe");
  const [skillName, setSkillName] = useState("");
  const [description, setDescription] = useState("");
  const [generating, setGenerating] = useState(false);
  const [text, setText] = useState(SKILL_TEMPLATE);

  const folderName = mode === "edit" ? (parseSkillName(text) || slugify(skillName) || "my-skill") : (slugify(skillName) || "my-skill");

  function handleGenerate() {
    if (!skillName.trim() && !description.trim()) return;
    setGenerating(true);
    // Simulate async generation (replace with real API call when System Agent is ready)
    setTimeout(() => {
      const generated = generateSkillFromDescription(skillName, description);
      setText(generated);
      setGenerating(false);
      setMode("edit");
    }, 800);
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      handleGenerate();
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-6" onClick={onClose}>
      <div className="bg-card border border-border rounded-2xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col" onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-border shrink-0">
          <div>
            <h2 className="text-lg font-semibold">Create a Skill</h2>
            <p className="text-xs text-muted-foreground mt-0.5">
              {mode === "describe"
                ? "Describe what your skill does — AI will generate the SKILL.md template"
                : "Edit the template — colored bars show SKILL.md sections"}
            </p>
          </div>
          <div className="flex items-center gap-2">
            {mode === "edit" && (
              <button
                onClick={() => setMode("describe")}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-muted text-xs font-medium hover:bg-muted/80 transition-colors text-muted-foreground"
              >
                <Wand2 className="w-3.5 h-3.5" />
                Regenerate
              </button>
            )}
            <button onClick={onClose} className="p-1.5 rounded-lg hover:bg-muted transition-colors">
              <X className="w-5 h-5 text-muted-foreground" />
            </button>
          </div>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto">
          {mode === "describe" ? (
            /* Describe mode — centered card */
            <div className="flex items-center justify-center min-h-[360px] p-8">
              <div className="w-full max-w-lg space-y-5">
                {/* Sparkle icon */}
                <div className="flex justify-center">
                  <div className="w-12 h-12 rounded-2xl bg-primary/10 border border-primary/20 flex items-center justify-center">
                    <Sparkles className="w-6 h-6 text-primary" />
                  </div>
                </div>

                <div className="text-center">
                  <h3 className="text-base font-semibold">Describe your skill</h3>
                  <p className="text-xs text-muted-foreground mt-1">Give it a name and explain what it does — AI generates the full SKILL.md</p>
                </div>

                {/* Skill name input */}
                <div className="space-y-1.5">
                  <label className="text-xs font-medium text-foreground/70">Skill name</label>
                  <input
                    type="text"
                    value={skillName}
                    onChange={(e) => setSkillName(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder="e.g. web-scraper"
                    className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary/50 font-mono"
                    autoFocus
                  />
                  {skillName && (
                    <p className="text-[10px] text-muted-foreground pl-0.5">
                      Folder: <span className="text-foreground/60 font-mono">{slugify(skillName) || "my-skill"}</span>
                    </p>
                  )}
                </div>

                {/* Description textarea */}
                <div className="space-y-1.5">
                  <label className="text-xs font-medium text-foreground/70">Description</label>
                  <textarea
                    value={description}
                    onChange={(e) => setDescription(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder="e.g. A skill that scrapes web pages and extracts structured data using CSS selectors"
                    className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary/50 resize-none"
                    rows={4}
                  />
                </div>

                {/* Actions */}
                <div className="flex flex-col gap-2">
                  <button
                    onClick={handleGenerate}
                    disabled={generating || (!skillName.trim() && !description.trim())}
                    className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-primary text-primary-foreground text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-40 disabled:cursor-not-allowed"
                  >
                    {generating ? (
                      <>
                        <Loader2 className="w-4 h-4 animate-spin" />
                        Generating SKILL.md...
                      </>
                    ) : (
                      <>
                        <Sparkles className="w-4 h-4" />
                        Generate SKILL.md
                      </>
                    )}
                  </button>
                  <button
                    onClick={() => { setText(SKILL_TEMPLATE); setMode("edit"); }}
                    className="w-full px-4 py-2 rounded-lg bg-transparent text-xs text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                  >
                    Skip — start from blank template
                  </button>
                </div>

                <p className="text-center text-[10px] text-muted-foreground">
                  Tip: Press <kbd className="px-1 py-0.5 rounded bg-muted border border-border font-mono text-[10px]">Cmd+Enter</kbd> to generate
                </p>
              </div>
            </div>
          ) : (
            /* Edit mode — TipTap editor with section bars */
            <TiptapSkillEditor value={text} onChange={setText} />
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-3 border-t border-border shrink-0">
          <p className="text-xs text-muted-foreground font-mono">
            ~/.mcclawd/skills/<span className="text-foreground/60">{folderName}</span>/SKILL.md
          </p>
          {mode === "edit" && (
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
          )}
          {mode === "describe" && (
            <button onClick={onClose} className="px-3 py-1.5 rounded-lg bg-muted text-xs font-medium hover:bg-muted/80 transition-colors">
              Cancel
            </button>
          )}
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
  const [scanning, setScanning] = useState(false);

  const { data: installed = [] } = useQuery({
    queryKey: ["skills"],
    queryFn: api.skills.list,
  });
  const installedInfo = installed.find((s) => s.name === name);

  const { data: skill, isLoading: metaLoading } = useQuery({
    queryKey: ["skill-detail", name],
    queryFn: () => api.skills.detail(name).catch(() => null),
  });

  // Fetch full SKILL.md content (tries disk first, then ClawHub download)
  const { data: skillContent, isLoading: contentLoading } = useQuery({
    queryKey: ["skill-content", name],
    queryFn: () => api.skills.content(name).then((r) => r.content).catch(() => null),
  });

  // Security scan query (only for installed skills)
  const { data: scanResult } = useQuery({
    queryKey: ["skill-scan", name],
    queryFn: () => api.skills.scan(name).catch(() => null),
    enabled: !!installedInfo,
  });

  const handleScan = async () => {
    setScanning(true);
    try {
      const result = await api.skills.scan(name);
      queryClient.setQueryData(["skill-scan", name], result);
      if (result.status === "Pass") {
        onNotify("success", `Scan passed — no issues found`);
      } else if (result.status === "Warning") {
        onNotify("error", `Scan found ${result.issues.length} warning(s)`);
      } else if (result.status === "Critical") {
        onNotify("error", `Scan found ${result.issues.length} critical issue(s)`);
      } else {
        onNotify("error", "Scanner not available (uvx/snyk-agent-scan not installed)");
      }
    } catch {
      onNotify("error", "Scan failed");
    } finally {
      setScanning(false);
    }
  };

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
            {installedInfo && (
              <>
                <SecurityBadge scanResult={scanResult} />
                <button
                  onClick={handleScan}
                  disabled={scanning}
                  className="px-3 py-1.5 rounded-lg bg-muted text-xs font-medium hover:bg-muted/80 transition-colors disabled:opacity-50 flex items-center gap-1.5"
                >
                  {scanning ? <Loader2 className="w-3 h-3 animate-spin" /> : <Shield className="w-3 h-3" />}
                  Scan
                </button>
              </>
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
          {(metaLoading || contentLoading) && (
            <div className="flex flex-col items-center justify-center py-16 gap-2">
              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
              <p className="text-xs text-muted-foreground">
                {contentLoading ? "Fetching SKILL.md content..." : "Loading metadata..."}
              </p>
            </div>
          )}

          {!metaLoading && !contentLoading && !skill && (
            <p className="text-muted-foreground text-sm text-center py-12">
              Skill details not available. Try refreshing the catalog.
            </p>
          )}

          {!metaLoading && !contentLoading && skill && sections.length > 0 && (
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
  const [scanResults, setScanResults] = useState<Record<string, ScanResult>>({});
  const [scanningSkill, setScanningSkill] = useState<string | null>(null);
  const scanLoadedRef = useRef(false);

  const autoRefreshed = useRef(false);

  const notify = useCallback((type: "success" | "error", message: string) => {
    setNotification({ type, message });
    setTimeout(() => setNotification(null), 3000);
  }, []);

  const handleCardScan = useCallback(
    (name: string) => {
      setScanningSkill(name);
      api.skills
        .scan(name)
        .then((result) => {
          setScanResults((prev) => ({ ...prev, [name]: result }));
        })
        .catch(() => {
          /* scan not available */
        })
        .finally(() => setScanningSkill(null));
    },
    [],
  );

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
      // Auto-scan after install
      handleCardScan(variables.name);
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

  // Load cached scan results for installed skills
  useEffect(() => {
    if (scanLoadedRef.current || installed.length === 0) return;
    scanLoadedRef.current = true;
    // Fire-and-forget: load scan results for each installed skill
    for (const skill of installed) {
      api.skills.scan(skill.name).then((result) => {
        if (result && result.status !== "NotScanned") {
          setScanResults((prev) => ({ ...prev, [skill.name]: result }));
        }
      }).catch(() => { /* ignore — scan not available */ });
    }
  }, [installed]);

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
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-2.5">
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
                  scanResult={scanResults[skill.name]}
                  onScan={() => handleCardScan(skill.name)}
                  scanPending={scanningSkill === skill.name}
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
                    scanResult={scanResults[skill.name]}
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
