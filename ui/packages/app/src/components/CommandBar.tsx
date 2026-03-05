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

type CommandBarState = "idle" | "listening" | "processing" | "responding";

interface SystemAction {
  action: string;
  [key: string]: unknown;
}

const HOLD_THRESHOLD_MS = 300;

export function CommandBar() {
  const navigate = useNavigate();
  const [input, setInput] = useState("");
  const [state, setState] = useState<CommandBarState>("idle");
  const [response, setResponse] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isListening, setIsListening] = useState(false);
  const [responseVisible, setResponseVisible] = useState(false);
  const [micLevel, setMicLevel] = useState(0);
  const [previewFile, setPreviewFile] = useState<AttachedFile | null>(null);
  const { files: attachedFiles, addFiles, removeFile, clear: clearFiles } = useFileAttachments();
  const inputRef = useRef<HTMLInputElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const recognitionRef = useRef<SpeechRecognition | null>(null);
  const responseRef = useRef<HTMLDivElement>(null);
  const mouseDownTimeRef = useRef<number>(0);
  const isHoldingRef = useRef(false);
  const audioContextRef = useRef<AudioContext | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const animFrameRef = useRef<number>(0);
  const mediaStreamRef = useRef<MediaStream | null>(null);

  // Cmd+K shortcut to focus input
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        inputRef.current?.focus();
      }
      if (e.key === "Escape" && responseVisible) {
        setResponseVisible(false);
        setResponse("");
        setError(null);
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
        // Not JSON — regular text response
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

  // Send message to system agent
  const sendMessage = useCallback(
    async (message: string) => {
      if (!message.trim()) return;

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
    [connectStream],
  );

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    sendMessage(input);
  };

  // --- Audio level metering ---
  const startAudioMeter = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      mediaStreamRef.current = stream;
      const ctx = new AudioContext();
      audioContextRef.current = ctx;
      const source = ctx.createMediaStreamSource(stream);
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 256;
      analyser.smoothingTimeConstant = 0.5;
      source.connect(analyser);
      analyserRef.current = analyser;

      const dataArray = new Uint8Array(analyser.frequencyBinCount);
      const tick = () => {
        analyser.getByteFrequencyData(dataArray);
        let sum = 0;
        for (let i = 0; i < dataArray.length; i++) {
          sum += dataArray[i] ?? 0;
        }
        const avg = sum / dataArray.length / 255;
        setMicLevel(Math.min(1, avg * 2.5));
        animFrameRef.current = requestAnimationFrame(tick);
      };
      tick();
    } catch {
      // Microphone not available — metering silently skipped
    }
  }, []);

  const stopAudioMeter = useCallback(() => {
    if (animFrameRef.current) {
      cancelAnimationFrame(animFrameRef.current);
      animFrameRef.current = 0;
    }
    if (audioContextRef.current) {
      audioContextRef.current.close();
      audioContextRef.current = null;
    }
    if (mediaStreamRef.current) {
      mediaStreamRef.current.getTracks().forEach((t) => t.stop());
      mediaStreamRef.current = null;
    }
    analyserRef.current = null;
    setMicLevel(0);
  }, []);

  // --- Speech recognition ---
  const startListening = useCallback(() => {
    const SpeechRecognition =
      window.SpeechRecognition || window.webkitSpeechRecognition;
    if (!SpeechRecognition) {
      setError("Speech recognition not supported in this browser");
      return;
    }

    const recognition = new SpeechRecognition();
    recognition.continuous = false;
    recognition.interimResults = true;
    recognition.lang = "en-US";

    recognition.onresult = (event: SpeechRecognitionEvent) => {
      const transcript = Array.from(event.results)
        .map((result) => result[0]?.transcript ?? "")
        .join("");
      setInput(transcript);

      const lastResult = event.results[event.results.length - 1];
      if (lastResult?.isFinal) {
        setIsListening(false);
        stopAudioMeter();
        if (transcript.trim()) {
          sendMessage(transcript);
        }
      }
    };

    recognition.onerror = () => {
      setIsListening(false);
      stopAudioMeter();
    };

    recognition.onend = () => {
      setIsListening(false);
      stopAudioMeter();
    };

    recognitionRef.current = recognition;
    recognition.start();
    setIsListening(true);
    startAudioMeter();
  }, [sendMessage, startAudioMeter, stopAudioMeter]);

  const stopListening = useCallback(() => {
    if (recognitionRef.current) {
      recognitionRef.current.stop();
    }
    setIsListening(false);
    stopAudioMeter();
  }, [stopAudioMeter]);

  // --- Mic button: hold-to-record OR click-to-toggle ---
  // mousedown on idle mic → start recording, mark as holding
  // mouseup after short press (<300ms) → keep recording (toggle mode — click again to stop)
  // mouseup after long press (>=300ms) → stop recording (hold mode)
  // mousedown on active mic → stop recording (second click to toggle off)
  const handleMicMouseDown = useCallback(() => {
    if (isListening) {
      // Already recording — any mousedown stops it (toggle off)
      stopListening();
      return;
    }
    mouseDownTimeRef.current = Date.now();
    isHoldingRef.current = true;
    startListening();
  }, [isListening, startListening, stopListening]);

  const handleMicMouseUp = useCallback(() => {
    if (!isHoldingRef.current) return;
    const elapsed = Date.now() - mouseDownTimeRef.current;
    if (elapsed >= HOLD_THRESHOLD_MS) {
      // Long press — stop on release
      stopListening();
    }
    // Short press — keep recording (toggle mode)
    isHoldingRef.current = false;
  }, [stopListening]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      stopAudioMeter();
      if (recognitionRef.current) {
        recognitionRef.current.stop();
      }
    };
  }, [stopAudioMeter]);

  // Auto-scroll response
  useEffect(() => {
    if (responseRef.current) {
      responseRef.current.scrollTop = responseRef.current.scrollHeight;
    }
  }, [response]);

  // Check if response contains an action
  useEffect(() => {
    if (state === "idle" && response) {
      const actionMatch = response.match(/\{[^{}]*"action"\s*:/);
      if (actionMatch) {
        try {
          const jsonStart = response.indexOf(actionMatch[0]);
          let depth = 0;
          let jsonEnd = jsonStart;
          for (let i = jsonStart; i < response.length; i++) {
            if (response[i] === "{") depth++;
            if (response[i] === "}") depth--;
            if (depth === 0) {
              jsonEnd = i + 1;
              break;
            }
          }
          const jsonStr = response.slice(jsonStart, jsonEnd);
          executeAction(jsonStr);
        } catch {
          // Not valid JSON, ignore
        }
      }
    }
  }, [state, response, executeAction]);

  // Mic level fill height (0-100%)
  const fillHeight = Math.round(micLevel * 100);

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
            <button
              onClick={() => {
                setResponseVisible(false);
                setResponse("");
                setError(null);
              }}
              className="mt-1 text-xs text-muted-foreground hover:text-foreground"
            >
              Dismiss (Esc)
            </button>
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
            placeholder={
              isListening
                ? "Listening..."
                : "Ask the system agent... (Cmd+K)"
            }
            disabled={state === "processing" || state === "responding"}
            className="w-full rounded-md border border-border bg-background px-3 py-2 pr-8 text-sm text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary disabled:opacity-50"
          />
          {(state === "processing" || state === "responding") && (
            <div className="absolute right-2 top-1/2 -translate-y-1/2">
              <div className="h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
            </div>
          )}
        </div>

        {/* Mic button — hold to record or click to toggle */}
        <button
          type="button"
          onMouseDown={(e) => {
            e.preventDefault();
            handleMicMouseDown();
          }}
          onMouseUp={handleMicMouseUp}
          onMouseLeave={() => {
            if (isHoldingRef.current && isListening) {
              stopListening();
            }
          }}
          disabled={state === "processing" || state === "responding"}
          className={`relative flex h-9 w-9 items-center justify-center rounded-md border transition-colors ${
            isListening
              ? "border-destructive bg-destructive/10 text-destructive"
              : "border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground"
          } disabled:opacity-50`}
          title={isListening ? "Click to stop / Release to stop" : "Click or hold to record"}
        >
          {/* Mic SVG with volume fill overlay */}
          <div className="relative h-4 w-4">
            {/* Volume fill — clips from bottom up */}
            {isListening && fillHeight > 0 && (
              <svg
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 24 24"
                fill="currentColor"
                stroke="none"
                className="absolute inset-0 h-4 w-4 text-destructive transition-[clip-path] duration-75"
                style={{
                  clipPath: `inset(${100 - fillHeight}% 0 0 0)`,
                }}
              >
                <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                <line x1="12" x2="12" y1="19" y2="22" strokeWidth="2" stroke="currentColor" />
              </svg>
            )}
            {/* Mic outline (always visible) */}
            <svg
              xmlns="http://www.w3.org/2000/svg"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              className="absolute inset-0 h-4 w-4"
            >
              <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
              <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
              <line x1="12" x2="12" y1="19" y2="22" />
            </svg>
          </div>
          {/* Recording pulse ring */}
          {isListening && (
            <span className="absolute inset-0 animate-ping rounded-md border border-destructive/30" />
          )}
        </button>

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
