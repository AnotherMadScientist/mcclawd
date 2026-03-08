import { useState, useRef, useEffect } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { AlertCircle, Download, FileText, User, RotateCcw, Pencil, Check, X, Shield, Wrench } from "lucide-react";
import { SpeechButton } from "./SpeechButton";
import { cn } from "../lib/utils";
import type { StreamEvent } from "../hooks/useTaskStream";
import type { SecurityEvent } from "../api/types";
import { MermaidBlock } from "./MermaidBlock";
import { CodeBlock } from "./CodeBlock";

interface StreamEntryProps {
  event: StreamEvent;
  onRetry?: (message: string) => void;
  onEditRetry?: (message: string) => void;
  securityEvents?: SecurityEvent[];
}

function getToolSecurityBadge(toolName: string, securityEvents?: SecurityEvent[]) {
  if (!securityEvents?.length) return null;
  const matches = securityEvents.filter((e) => e.tool_name === toolName);
  if (matches.length === 0) return null;

  // Use the worst threat level
  const levels = ["critical", "dangerous", "suspicious", "safe"];
  const worst = levels.find((l) => matches.some((e) => e.threat_level === l));
  const blocked = matches.some((e) => e.action_taken === "blocked");

  if (blocked || worst === "critical" || worst === "dangerous") {
    return { color: "text-red-400", bg: "bg-red-500/10", label: "Blocked" };
  }
  if (worst === "suspicious") {
    return { color: "text-amber-400", bg: "bg-amber-500/10", label: "Warning" };
  }
  return { color: "text-emerald-400", bg: "bg-emerald-500/10", label: "Clean" };
}

export function StreamEntry({ event, onRetry, onEditRetry, securityEvents }: StreamEntryProps) {
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState(event.content);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (editing && textareaRef.current) {
      textareaRef.current.focus();
      const len = textareaRef.current.value.length;
      textareaRef.current.setSelectionRange(len, len);
      // Auto-resize
      textareaRef.current.style.height = "auto";
      textareaRef.current.style.height = textareaRef.current.scrollHeight + "px";
    }
  }, [editing]);

  const handleEditSubmit = () => {
    const trimmed = editValue.trim();
    if (trimmed && onEditRetry) {
      onEditRetry(trimmed);
    }
    setEditing(false);
  };

  const handleEditCancel = () => {
    setEditValue(event.content);
    setEditing(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleEditSubmit();
    }
    if (e.key === "Escape") {
      handleEditCancel();
    }
  };

  if (event.type === "user") {
    return (
      <div className="group flex justify-end">
        <div className="flex items-start gap-3 max-w-[80%]">
          {/* Action buttons — visible on group hover, hidden during edit */}
          {!editing && (onRetry || onEditRetry) && (
            <div className="flex flex-col gap-1 self-center opacity-0 group-hover:opacity-100 transition-opacity duration-150">
              {onRetry && (
                <button
                  onClick={() => onRetry(event.content)}
                  title="Retry"
                  className="p-1 rounded text-zinc-500 hover:text-zinc-200 hover:bg-zinc-700/60 transition-colors"
                >
                  <RotateCcw className="w-3.5 h-3.5" />
                </button>
              )}
              {onEditRetry && (
                <button
                  onClick={() => {
                    setEditValue(event.content);
                    setEditing(true);
                  }}
                  title="Edit and retry"
                  className="p-1 rounded text-zinc-500 hover:text-zinc-200 hover:bg-zinc-700/60 transition-colors"
                >
                  <Pencil className="w-3.5 h-3.5" />
                </button>
              )}
            </div>
          )}

          {/* Message bubble */}
          <div className="rounded-2xl rounded-tr-sm bg-primary/15 border border-primary/20 px-4 py-2.5">
            {editing ? (
              <div className="flex flex-col gap-2">
                <textarea
                  ref={textareaRef}
                  value={editValue}
                  onChange={(e) => {
                    setEditValue(e.target.value);
                    e.target.style.height = "auto";
                    e.target.style.height = e.target.scrollHeight + "px";
                  }}
                  onKeyDown={handleKeyDown}
                  rows={1}
                  className="w-full resize-none bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground min-w-[200px]"
                />
                <div className="flex items-center gap-1.5 justify-end">
                  <button
                    onClick={handleEditCancel}
                    title="Cancel"
                    className="flex items-center gap-1 px-2 py-0.5 rounded text-xs text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700/60 transition-colors"
                  >
                    <X className="w-3 h-3" />
                    Cancel
                  </button>
                  <button
                    onClick={handleEditSubmit}
                    title="Send"
                    disabled={!editValue.trim()}
                    className="flex items-center gap-1 px-2 py-0.5 rounded text-xs text-primary hover:text-primary/80 hover:bg-primary/10 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                  >
                    <Check className="w-3 h-3" />
                    Send
                  </button>
                </div>
              </div>
            ) : (
              <p className="text-sm text-foreground whitespace-pre-wrap">{event.content}</p>
            )}
          </div>

          <div className="w-8 h-8 rounded-full bg-primary/20 flex items-center justify-center shrink-0">
            <User className="w-4 h-4 text-primary" />
          </div>
        </div>
      </div>
    );
  }

  if (event.type === "attachments" && event.attachments) {
    const token = localStorage.getItem("mcclawd_token") || "";
    const authUrl = (url: string) => `${url}${url.includes("?") ? "&" : "?"}token=${encodeURIComponent(token)}`;
    return (
      <div className="flex justify-end">
        <div className="flex items-start gap-3 max-w-[80%]">
          <div className="flex flex-wrap gap-2 rounded-2xl rounded-tr-sm bg-primary/10 border border-primary/20 px-3 py-2">
            {event.attachments.map((att, i) => {
              const isImage = att.content_type.startsWith("image/");
              const sizeLabel =
                att.size < 1024
                  ? att.size + "B"
                  : att.size < 1048576
                    ? (att.size / 1024).toFixed(1) + "KB"
                    : (att.size / 1048576).toFixed(1) + "MB";
              return (
                <a
                  key={i}
                  href={authUrl(att.url)}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="group relative flex h-14 w-14 items-center justify-center rounded-md border border-border bg-muted/50 hover:border-primary/50 transition-colors overflow-hidden"
                  title={`${att.name} (${sizeLabel})`}
                >
                  {isImage ? (
                    <img
                      src={authUrl(att.url)}
                      alt={att.name}
                      className="h-full w-full object-cover"
                    />
                  ) : (
                    <FileText className="h-5 w-5 text-muted-foreground" />
                  )}
                  <span className="absolute bottom-0 left-0 right-0 truncate bg-black/60 px-1 text-[9px] text-white">
                    {att.name}
                  </span>
                </a>
              );
            })}
          </div>
          <div className="w-8 h-8 rounded-full bg-primary/20 flex items-center justify-center shrink-0">
            <User className="w-4 h-4 text-primary" />
          </div>
        </div>
      </div>
    );
  }

  if (event.type === "generated_files" && event.attachments) {
    const token = localStorage.getItem("mcclawd_token") || "";
    const authUrl = (url: string) =>
      `${url}${url.includes("?") ? "&" : "?"}token=${encodeURIComponent(token)}`;
    return (
      <div className="flex items-start gap-3 rounded-lg bg-emerald-500/10 border border-emerald-500/20 p-4">
        <Download className="w-5 h-5 text-emerald-400 shrink-0 mt-0.5" />
        <div className="flex flex-col gap-2">
          <p className="text-sm font-medium text-emerald-300">Generated Files</p>
          <div className="flex flex-wrap gap-2">
            {event.attachments.map((file, i) => {
              const sizeLabel =
                file.size < 1024
                  ? file.size + "B"
                  : file.size < 1048576
                    ? (file.size / 1024).toFixed(1) + "KB"
                    : (file.size / 1048576).toFixed(1) + "MB";
              return (
                <a
                  key={i}
                  href={authUrl(file.url)}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center gap-2 px-2 py-1 rounded bg-emerald-500/10 border border-emerald-500/20 text-emerald-300 hover:bg-emerald-500/20 transition-colors text-sm"
                >
                  <FileText className="w-4 h-4 shrink-0" />
                  <span className="truncate max-w-[200px]">{file.name}</span>
                  <span className="text-xs text-emerald-400/60">{sizeLabel}</span>
                </a>
              );
            })}
          </div>
        </div>
      </div>
    );
  }

  if (event.type === "tool_start" && event.toolName) {
    const badge = getToolSecurityBadge(event.toolName, securityEvents);
    return (
      <div className="flex items-center gap-2 px-3 py-1.5 text-xs text-muted-foreground">
        <Wrench className="w-3 h-3 shrink-0" />
        <span className="font-mono">{event.toolName}</span>
        {badge && (
          <span
            className={cn(
              "inline-flex items-center gap-1 px-1.5 py-0.5 rounded-full text-[10px] font-medium",
              badge.bg,
              badge.color,
            )}
          >
            <Shield className="w-2.5 h-2.5" />
            {badge.label}
          </span>
        )}
      </div>
    );
  }

  if (event.type === "tool_end") {
    if (!event.content) return null;
    return (
      <div className="flex items-center gap-2 px-3 py-1 text-xs text-muted-foreground/70 ml-5 border-l-2 border-border pl-3">
        <span className="truncate">{event.content}</span>
      </div>
    );
  }

  if (event.type === "error") {
    return (
      <div className="flex items-start gap-3 rounded-lg bg-red-500/10 border border-red-500/20 p-4">
        <AlertCircle className="w-5 h-5 text-red-400 shrink-0 mt-0.5" />
        <p className="text-sm text-red-300 whitespace-pre-wrap">{event.content}</p>
      </div>
    );
  }

  // text response — rendered as markdown
  return (
    <div className={cn("group/response rounded-lg bg-card border border-border p-4 relative", "agent-response")}>
      <div className="prose-response text-sm text-foreground">
        <Markdown
          remarkPlugins={[remarkGfm]}
          rehypePlugins={[rehypeHighlight]}
          components={{
            code({ className, children, ...props }) {
              const match = /language-(\w+)/.exec(className || "");
              const language = match ? match[1] : "";
              const code = String(children).replace(/\n$/, "");
              if (language === "mermaid") {
                return <MermaidBlock code={code} />;
              }
              if (language || code.includes("\n")) {
                return <CodeBlock language={language} code={code} />;
              }
              // Inline code
              return (
                <code className={className} {...props}>
                  {children}
                </code>
              );
            },
            pre({ children }) {
              // Suppress default <pre> wrapper — CodeBlock provides its own
              return <>{children}</>;
            },
            table({ children }) {
              return (
                <div className="overflow-x-auto my-3">
                  <table className="min-w-full">{children}</table>
                </div>
              );
            },
          }}
        >
          {event.content}
        </Markdown>
      </div>
      {/* Read aloud button — appears on hover */}
      <div className="absolute top-2 right-2 opacity-0 group-hover/response:opacity-100 transition-opacity">
        <SpeechButton text={event.content} size="sm" />
      </div>
    </div>
  );
}
