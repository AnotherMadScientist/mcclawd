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
} from "lucide-react";
import { api } from "../api/client";
import { ResourceCard } from "../components/ResourceCard";

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
  const { files, addFiles, removeFile, clear: clearFiles } = useFileAttachments();
  const navigate = useNavigate();

  const { data: config } = useQuery({
    queryKey: ["config"],
    queryFn: api.config.get,
  });

  const { data: mcpServers = [] } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: api.mcp.servers,
  });

  const createTask = useMutation({
    mutationFn: async () => {
      const task = await api.tasks.create(prompt);
      if (files.length > 0) {
        await api.tasks.uploadAttachments(task.id, files.map((f) => f.file));
      }
      return task;
    },
    onSuccess: (task) => {
      clearFiles();
      navigate(`/tasks/${task.id}`);
    },
  });

  // Track what was typed before mic started so we can append cleanly
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
            title={config?.agent.model || "claude-sonnet-4-5"}
            description="AI model powering the agent"
            color="text-violet-400"
            status="active"
          />
          <ResourceCard
            icon={FileText}
            title={`Workspace: ${config?.agent.default_workspace || "default"}`}
            description="Agent personality, skills, and user preferences"
            items={["SOUL.md", "AGENTS.md", "USER.md"]}
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
