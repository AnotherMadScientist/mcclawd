import { useState, useEffect, useRef, useCallback } from "react";
import { useNavigate } from "react-router";
import { api, getToken } from "../api/client";
import type { StreamChunk } from "../api/types";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  useFileAttachments,
  DropZone,
  AttachButton,
  FileThumbnails,
  FilePreviewDialog,
} from "./FileAttachments";
import type { AttachedFile } from "./FileAttachments";
import { MicButton } from "./MicButton";
import { SpeechButton } from "./SpeechButton";

type CommandBarState = "idle" | "listening" | "processing" | "responding";

interface SystemAction {
  action: string;
  [key: string]: unknown;
}

export function CommandBar() {
  const navigate = useNavigate();
  const [input, setInput] = useState("");
  const [state, setState] = useState<CommandBarState>("idle");
  const [response, setResponse] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [responseVisible, setResponseVisible] = useState(false);
  const [previewFile, setPreviewFile] = useState<AttachedFile | null>(null);
  const { files: attachedFiles, addFiles, removeFile, clear: clearFiles } = useFileAttachments();
  const inputRef = useRef<HTMLInputElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const responseRef = useRef<HTMLDivElement>(null);

  // Cmd+K shortcut to focus input
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        inputRef.current?.focus();
      }
      if (e.key === "Escape") {
        if (responseVisible) {
          setResponseVisible(false);
          setResponse("");
          setError(null);
        }
        inputRef.current?.blur();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [responseVisible]);

  // Parse tool call results for UI actions
  const executeAction = useCallback(
    (text: string) => {
      try {
        const parsed: SystemAction = JSON.parse(text);
        switch (parsed.action) {
          case "navigate":
            if (typeof parsed.path === "string") {
              navigate(parsed.path);
              return true;
            }
            break;
          case "create_task":
            if (typeof parsed.prompt === "string") {
              api.tasks.create(parsed.prompt as string).then((task) => {
                navigate(`/tasks/${task.id}`);
              });
              return true;
            }
            break;
          case "install_skill":
            if (typeof parsed.name === "string") {
              api.skills
                .install(
                  parsed.name as string,
                  parsed.version as string | undefined,
                )
                .then(() => {
                  navigate("/config/skills");
                });
              return true;
            }
            break;
          case "uninstall_skill":
            if (typeof parsed.name === "string") {
              api.skills.uninstall(parsed.name as string).then(() => {
                navigate("/config/skills");
              });
              return true;
            }
            break;
          case "list_skills":
            navigate("/config/skills");
            return true;
          case "manage_secret":
            navigate("/config/secrets");
            return true;
          case "read_workspace":
          case "update_workspace":
            navigate("/workspace");
            return true;
        }
      } catch {
        // Not JSON — try natural language navigation patterns
        const navMatch = text.match(
          /(?:navigat(?:ed?|ing)\s+(?:you\s+)?to|go(?:ing)?\s+to|taking\s+you\s+to|opening)\s+(\/[\w/-]+)/i,
        );
        if (navMatch && navMatch[1]) {
          navigate(navMatch[1]);
          return true;
        }
      }
      return false;
    },
    [navigate],
  );

  // Connect to system agent WebSocket stream
  const connectStream = useCallback(() => {
    if (wsRef.current) {
      wsRef.current.close();
    }

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const token = getToken() || "";
    const wsUrl = `${protocol}//${window.location.host}/api/tasks/__system__/stream?token=${encodeURIComponent(token)}&skip_history=1`;
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => {
      setState("responding");
    };

    ws.onmessage = (event) => {
      try {
        const chunk: StreamChunk = JSON.parse(event.data);

        if (chunk === "Done") {
          setState("idle");
          return;
        }

        if ("TextDelta" in chunk) {
          setResponse((prev) => prev + chunk.TextDelta);
        } else if ("TextBlock" in chunk) {
          setResponse((prev) => prev + chunk.TextBlock);
        } else if ("ToolStart" in chunk) {
          setState("processing");
        } else if ("ToolEnd" in chunk && chunk.ToolEnd.summary) {
          try {
            const action = JSON.parse(chunk.ToolEnd.summary);
            if (action.action) {
              executeAction(chunk.ToolEnd.summary);
            }
          } catch {
            /* not action JSON */
          }
        } else if ("StatusIndicator" in chunk) {
          if (chunk.StatusIndicator === "Processing") {
            setState("responding");
          }
        } else if ("Error" in chunk) {
          setError(chunk.Error);
          setState("idle");
        }
      } catch {
        // ignore parse errors
      }
    };

    ws.onclose = () => {
      setState((prev) => (prev === "responding" ? "idle" : prev));
    };
  }, []);

  // Direct navigation shortcuts — bypass LLM for common nav commands
  const ROUTE_KEYWORDS: [RegExp, string][] = [
    [/^\/?(settings|config)$/i, "/config"],
    [/^\/?(tasks?)$/i, "/tasks"],
    [/^\/?(new\s*task|create\s*task)$/i, "/tasks/new"],
    [/^\/?(skills?)$/i, "/config/skills"],
    [/^\/?(secrets?)$/i, "/config/secrets"],
    [/^\/?(mcp|servers?)$/i, "/config/mcp"],
    [/^\/?(workspace)$/i, "/workspace"],
    [/^\/?(home|dashboard)$/i, "/"],
    [/^go\s*(?:to\s+)?\/?(settings|config)$/i, "/config"],
    [/^go\s*(?:to\s+)?\/?(tasks?)$/i, "/tasks"],
    [/^go\s*(?:to\s+)?\/?(skills?)$/i, "/config/skills"],
    [/^go\s*(?:to\s+)?\/?(secrets?)$/i, "/config/secrets"],
    [/^go\s*(?:to\s+)?\/?(mcp|servers?)$/i, "/config/mcp"],
    [/^go\s*(?:to\s+)?\/?(workspace)$/i, "/workspace"],
    [/^go\s*(?:to\s+)?\/?(home|dashboard)$/i, "/"],
    [/^show\s+\/?(tasks?)$/i, "/tasks"],
    [/^show\s+\/?(skills?)$/i, "/config/skills"],
    [/^show\s+\/?(secrets?)$/i, "/config/secrets"],
    [/^show\s+\/?(settings|config)$/i, "/config"],
  ];

  // Send message to system agent
  const sendMessage = useCallback(
    async (message: string) => {
      if (!message.trim()) return;

      // Try direct navigation first (no LLM needed)
      const trimMsg = message.trim();
      for (const [pattern, route] of ROUTE_KEYWORDS) {
        if (pattern.test(trimMsg)) {
          setInput("");
          navigate(route);
          return;
        }
      }

      setInput("");
      setResponse("");
      setError(null);
      setResponseVisible(true);
      setState("processing");
      clearFiles();

      try {
        await api.systemAgent.chat(message);
        connectStream();
      } catch (e) {
        setError(e instanceof Error ? e.message : "Failed to send message");
        setState("idle");
      }
    },
    [connectStream, navigate],
  );

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    sendMessage(input);
  };

  // Auto-scroll response
  useEffect(() => {
    if (responseRef.current) {
      responseRef.current.scrollTop = responseRef.current.scrollHeight;
    }
  }, [response]);

  // Check if response contains an action — JSON or natural language navigation
  useEffect(() => {
    if (state === "idle" && response) {
      const trimmed = response.trim();
      // Try JSON action first
      if (trimmed.startsWith('{"action":')) {
        try {
          executeAction(trimmed);
        } catch {
          // Malformed JSON — will render as text
        }
      } else {
        // Try natural language navigation detection on the full response
        executeAction(trimmed);
      }
    }
  }, [state, response, executeAction]);

  return (
    <div className="border-t border-border bg-card/50 backdrop-blur-sm">
      {/* Response area */}
      {responseVisible && (response || error) && (
        <div
          ref={responseRef}
          className="max-h-48 overflow-y-auto border-b border-border px-4 py-3"
        >
          {error ? (
            <p className="text-sm text-destructive">{error}</p>
          ) : (
            <div className="prose-response text-sm text-foreground">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {response}
              </ReactMarkdown>
            </div>
          )}
          {state === "idle" && (
            <div className="mt-1 flex items-center gap-2">
              <button
                onClick={() => {
                  setResponseVisible(false);
                  setResponse("");
                  setError(null);
                }}
                className="text-xs text-muted-foreground hover:text-foreground"
              >
                Dismiss (Esc)
              </button>
              {response && <SpeechButton text={response} size="sm" />}
            </div>
          )}
        </div>
      )}

      {/* Attached files thumbnails */}
      {attachedFiles.length > 0 && (
        <div className="border-b border-border px-4">
          <FileThumbnails
            files={attachedFiles}
            onRemove={removeFile}
            onPreview={setPreviewFile}
            disabled={state === "processing" || state === "responding"}
          />
        </div>
      )}

      {/* Input bar — wrapped in drop zone */}
      <DropZone onDrop={addFiles} disabled={state === "processing" || state === "responding"}>
        <form onSubmit={handleSubmit} className="flex items-center gap-2 px-4 py-2">
          {/* Attach button */}
          <AttachButton
            onFiles={addFiles}
            disabled={state === "processing" || state === "responding"}
            compact
          />

          <div className="relative flex-1">
          <input
            ref={inputRef}
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="Ask the system agent... (Cmd+K)"
            disabled={state === "processing" || state === "responding"}
            className="w-full rounded-md border border-border bg-background px-3 py-2 pr-8 text-sm text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary disabled:opacity-50"
          />
          {(state === "processing" || state === "responding") && (
            <div className="absolute right-2 top-1/2 -translate-y-1/2">
              <div className="h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
            </div>
          )}
        </div>

        {/* Mic button — uses shared MicButton with Whisper fallback */}
        <MicButton
          onTranscript={(text) => sendMessage(text)}
          onInterim={(text) => setInput(text)}
          onError={(msg) => setError(msg)}
          disabled={state === "processing" || state === "responding"}
          size="sm"
        />

        {/* Send button */}
        <button
          type="submit"
          disabled={
            !input.trim() || state === "processing" || state === "responding"
          }
          className="flex h-9 w-9 items-center justify-center rounded-md bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            className="h-4 w-4"
          >
            <path d="m22 2-7 20-4-9-9-4Z" />
            <path d="M22 2 11 13" />
          </svg>
        </button>
        </form>
      </DropZone>

      {/* File preview dialog */}
      <FilePreviewDialog file={previewFile} onClose={() => setPreviewFile(null)} />
    </div>
  );
}
