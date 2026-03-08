import { useState, useCallback, useRef } from "react";
import { useNavigate } from "react-router";
import { useQuery, useMutation } from "@tanstack/react-query";
import {
  Brain,
  Server,
  Puzzle,
  HardDrive,
  FileText,
  Sparkles,
  ArrowRight,
  ChevronDown,
  ChevronUp,
} from "lucide-react";
import { api } from "../api/client";
import { ResourceCard } from "../components/ResourceCard";

const FALLBACK_MODELS = ["claude-sonnet-4-6-20250514", "claude-opus-4-6-20250514", "claude-haiku-4-5-20251001"];

function SkillsResourceCard() {
  const { data: skills = [] } = useQuery({
    queryKey: ["installed-skills"],
    queryFn: api.skills.list,
  });
  if (skills.length === 0) {
    return (
      <ResourceCard
        icon={Puzzle}
        title="Skills"
        description="No skills installed yet"
        color="text-zinc-500"
        status="inactive"
      />
    );
  }
  return (
    <ResourceCard
      icon={Puzzle}
      title={`${skills.length} Skill${skills.length !== 1 ? "s" : ""} Installed`}
      description="Skills available to the agent"
      items={skills.map((s) => s.name)}
      color="text-purple-400"
      status="active"
    />
  );
}
import {
  useFileAttachments,
  DropZone,
  AttachButton,
  FileThumbnails,
  FilePreviewDialog,
} from "../components/FileAttachments";
import type { AttachedFile } from "../components/FileAttachments";
import { MicButton } from "../components/MicButton";

export function NewTaskPage() {
  const [prompt, setPrompt] = useState("");
  const [previewFile, setPreviewFile] = useState<AttachedFile | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [selectedModel, setSelectedModel] = useState<string>("");
  const [selectedWorkspace, setSelectedWorkspace] = useState<string>("");
  const [selectedSkills, setSelectedSkills] = useState<string[]>([]);
  const [selectedToolProfile, setSelectedToolProfile] = useState<string>("");
  const [tagsInput, setTagsInput] = useState("");
  const { files, addFiles, removeFile, clear: clearFiles } = useFileAttachments();
  const navigate = useNavigate();

  const { data: config } = useQuery({
    queryKey: ["config"],
    queryFn: api.config.get,
  });

  const { data: liveModels } = useQuery({
    queryKey: ["providers", "models"],
    queryFn: api.providers.models,
    staleTime: 3600_000,
    retry: 1,
  });

  const modelOptions = liveModels && liveModels.length > 0
    ? liveModels.map((m) => m.id)
    : FALLBACK_MODELS;

  const { data: mcpServers = [] } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: api.mcp.servers,
  });

  const { data: llmHealth } = useQuery({
    queryKey: ["llm-health"],
    queryFn: () =>
      api.health.llm().catch(() => ({ ok: false, error: "Checking LLM connection..." } as { ok: boolean; error?: string })),
    refetchInterval: 30_000,
    retry: false,
  });

  const { data: installedSkills = [] } = useQuery({
    queryKey: ["installed-skills"],
    queryFn: api.skills.list,
  });

  // Resolve effective model/workspace (from selectors, falling back to config)
  const effectiveModel = selectedModel || config?.agent.model;
  const effectiveWorkspace = selectedWorkspace || config?.agent.default_workspace || "default";

  const createTask = useMutation({
    mutationFn: async () => {
      const hasFiles = files.length > 0;
      const parsedTags = tagsInput.split(",").map((t) => t.trim()).filter(Boolean);
      const task = await api.tasks.create(
        prompt,
        effectiveWorkspace !== (config?.agent.default_workspace || "default") ? effectiveWorkspace : undefined,
        selectedModel || undefined,
        hasFiles,
        parsedTags.length > 0 ? parsedTags : undefined,
        selectedSkills.length > 0 ? selectedSkills : undefined,
        selectedToolProfile || undefined,
      );
      if (hasFiles) {
        // Retry upload + sendMessage up to 3 times (handles transient 503 from server restarts)
        let lastErr: unknown;
        for (let attempt = 0; attempt < 3; attempt++) {
          try {
            await api.tasks.uploadAttachments(task.id, files.map((f) => f.file));
            await api.tasks.sendMessage(task.id, prompt);
            return task;
          } catch (err) {
            lastErr = err;
            if (attempt < 2) await new Promise((r) => setTimeout(r, 1500 * (attempt + 1)));
          }
        }
        throw lastErr;
      }
      return task;
    },
    onSuccess: (task) => {
      clearFiles();
      navigate(`/tasks/${task.id}`);
    },
  });

  const promptBeforeMicRef = useRef(prompt);

  const handleInterim = useCallback((text: string) => {
    const base = promptBeforeMicRef.current;
    setPrompt(base ? base + " " + text : text);
  }, []);

  const handleTranscript = useCallback((text: string) => {
    const base = promptBeforeMicRef.current;
    const final_ = base ? base + " " + text : text;
    setPrompt(final_);
    promptBeforeMicRef.current = final_;
  }, []);

  const toggleSkill = (name: string) => {
    setSelectedSkills((prev) =>
      prev.includes(name) ? prev.filter((s) => s !== name) : [...prev, name],
    );
  };

  return (
    <div className="max-w-4xl mx-auto space-y-8">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">New Task</h1>
        <p className="text-muted-foreground mt-1">
          Describe what you'd like the agent to do
        </p>
      </div>

      <DropZone onDrop={addFiles}>
        <div className="relative">
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="What would you like me to do? (drag files here to attach)"
            rows={4}
            autoFocus
            className="w-full p-5 rounded-xl bg-card border border-border text-foreground placeholder:text-muted-foreground resize-none focus:outline-none focus:ring-2 focus:ring-primary/30 focus:border-primary/50 transition-all text-base"
          />
          <div className="absolute bottom-4 right-4 flex items-center gap-2">
            <MicButton
              onTranscript={handleTranscript}
              onInterim={handleInterim}
              onError={(msg) => {
                console.warn("[Mic]", msg);
                handleInterim(`[Mic error: ${msg}]`);
              }}
              disabled={createTask.isPending}
            />
            <AttachButton onFiles={addFiles} compact />
            <button
              onClick={() => createTask.mutate()}
              disabled={!prompt.trim() || createTask.isPending}
              className="flex items-center gap-2 px-4 py-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40 transition-all text-sm font-medium"
            >
              {createTask.isPending ? "Starting..." : "Run Task"}
              <ArrowRight className="w-4 h-4" />
            </button>
          </div>
        </div>
        {/* Tags input */}
        <div className="mt-2">
          <input
            data-testid="task-tags-input"
            value={tagsInput}
            onChange={(e) => setTagsInput(e.target.value)}
            placeholder="Tags (comma-separated, e.g. deploy, urgent)"
            className="w-full px-4 py-2 rounded-lg bg-card border border-border text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/30"
          />
        </div>
        {files.length > 0 && (
          <div className="px-2">
            <FileThumbnails
              files={files}
              onRemove={removeFile}
              onPreview={setPreviewFile}
            />
          </div>
        )}
      </DropZone>
      <FilePreviewDialog file={previewFile} onClose={() => setPreviewFile(null)} />

      {/* Advanced Options */}
      <div className="rounded-xl border border-border bg-card overflow-hidden">
        <button
          onClick={() => setShowAdvanced((v) => !v)}
          className="w-full flex items-center justify-between px-4 py-3 text-sm font-medium hover:bg-muted/50 transition-colors"
          aria-expanded={showAdvanced}
        >
          <span>Advanced Options</span>
          {showAdvanced ? (
            <ChevronUp className="w-4 h-4 text-muted-foreground" />
          ) : (
            <ChevronDown className="w-4 h-4 text-muted-foreground" />
          )}
        </button>

        {showAdvanced && (
          <div className="px-4 pb-4 space-y-4 border-t border-border pt-4">
            {/* Model selector */}
            <div>
              <label className="text-xs text-muted-foreground mb-1 block">Model</label>
              <select
                value={selectedModel || effectiveModel || ""}
                onChange={(e) => setSelectedModel(e.target.value)}
                className="w-full text-sm font-mono bg-background border border-border rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-primary/30"
                aria-label="Model"
              >
                {modelOptions.map((m) => (
                  <option key={m} value={m}>{m}</option>
                ))}
              </select>
            </div>

            {/* Workspace selector */}
            <div>
              <label className="text-xs text-muted-foreground mb-1 block">Workspace</label>
              <select
                value={selectedWorkspace || effectiveWorkspace}
                onChange={(e) => setSelectedWorkspace(e.target.value)}
                className="w-full text-sm font-mono bg-background border border-border rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-primary/30"
                aria-label="Workspace"
              >
                <option value="default">default</option>
                {config?.agent.default_workspace && config.agent.default_workspace !== "default" && (
                  <option value={config.agent.default_workspace}>{config.agent.default_workspace}</option>
                )}
              </select>
            </div>

            {/* Tool profile selector */}
            <div>
              <label className="text-xs text-muted-foreground mb-1 block">Tool Profile</label>
              <select
                value={selectedToolProfile || config?.agent.default_tool_profile || "Coding"}
                onChange={(e) => setSelectedToolProfile(e.target.value)}
                className="w-full text-sm font-mono bg-background border border-border rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-primary/30"
                aria-label="Tool Profile"
              >
                <option value="Minimal">Minimal - memory tools only</option>
                <option value="Coding">Coding - filesystem, git, shell</option>
                <option value="Research">Research - web, fetch, browser</option>
                <option value="Full">Full - all available tools</option>
              </select>
            </div>

            {/* Skills multi-select */}
            {installedSkills.length > 0 && (
              <div>
                <label className="text-xs text-muted-foreground mb-2 block">
                  Skills (deselect to exclude)
                </label>
                <div className="space-y-1 max-h-40 overflow-y-auto">
                  {installedSkills.map((skill) => (
                    <label
                      key={skill.name}
                      className="flex items-center gap-2 text-sm cursor-pointer hover:bg-muted/50 px-2 py-1 rounded"
                    >
                      <input
                        type="checkbox"
                        checked={
                          selectedSkills.length === 0
                            ? true
                            : selectedSkills.includes(skill.name)
                        }
                        onChange={() => {
                          if (selectedSkills.length === 0) {
                            // Start with all selected, then deselect this one
                            const all = installedSkills.map((s) => s.name).filter((n) => n !== skill.name);
                            setSelectedSkills(all);
                          } else {
                            toggleSkill(skill.name);
                          }
                        }}
                        className="rounded"
                      />
                      <span className="font-mono">{skill.name}</span>
                    </label>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      <div>
        <h2 className="text-lg font-semibold mb-4 flex items-center gap-2">
          <Sparkles className="w-5 h-5 text-primary" />
          Available Resources
        </h2>
        <p className="text-sm text-muted-foreground mb-4">
          The agent has access to these tools and capabilities for your task
        </p>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          <ResourceCard
            icon={Brain}
            title={effectiveModel || "claude-sonnet-4-5"}
            description={llmHealth?.ok ? "AI model powering the agent" : llmHealth?.error || "Checking LLM connection..."}
            color="text-violet-400"
            status={llmHealth?.ok ? "active" : "inactive"}
          />
          <ResourceCard
            icon={FileText}
            title={`Workspace: ${effectiveWorkspace}`}
            description="Agent personality, skills, and user preferences"
            items={["SOUL.md", "AGENTS.md", "USER.md", "IDENTITY.md", "TOOLS.md", "HEARTBEAT.md"]}
            color="text-amber-400"
            status="active"
          />
          <ResourceCard
            icon={HardDrive}
            title="Builtin Tools"
            description="Core tools available to every agent"
            items={["memory.store", "memory.recall"]}
            color="text-cyan-400"
            status="active"
          />
          {mcpServers.map((server) => (
            <ResourceCard
              key={server.name}
              icon={Server}
              title={server.name}
              description={`MCP server (port ${server.port})`}
              color="text-emerald-400"
              status="active"
            />
          ))}
          <SkillsResourceCard />
        </div>
      </div>
    </div>
  );
}
