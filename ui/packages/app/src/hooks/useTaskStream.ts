import { useState, useEffect, useRef, useCallback } from "react";
import type { StreamChunk } from "../api/types";

export interface StreamEvent {
  type: "text" | "error" | "user" | "attachments";
  content: string;
  timestamp: Date;
  attachments?: Array<{
    name: string;
    size: number;
    content_type: string;
    url: string;
  }>;
}

export function useTaskStream(taskId: string | undefined) {
  const [events, setEvents] = useState<StreamEvent[]>([]);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);
  const [done, setDone] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const streamingRef = useRef(false);
  const skipHistoryRef = useRef(false);
  // When true, the next TextDelta creates a new text event instead of appending.
  // Set on every StatusIndicator(Processing) so each LLM turn is a separate block.
  const newBlockRef = useRef(true);

  const connect = useCallback(() => {
    if (!taskId) return;

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const token = localStorage.getItem("mcclawd_token") || "";
    let wsUrl = `${protocol}//${window.location.host}/api/tasks/${taskId}/stream?token=${encodeURIComponent(token)}`;
    if (skipHistoryRef.current) {
      wsUrl += "&skip_history=1";
    }
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => {
      setConnected(true);
      skipHistoryRef.current = false;
    };
    ws.onclose = () => {
      setConnected(false);
    };
    ws.onmessage = (event) => {
      try {
        const chunk: StreamChunk = JSON.parse(event.data);
        const timestamp = new Date();

        if (chunk === "Done") {
          streamingRef.current = false;
          newBlockRef.current = true;
          setStatusMessage(null);
          setDone(true);
          return;
        }

        if ("UserMessage" in chunk) {
          // User message from history replay or live stream — render as user bubble
          // Skip if already added locally by reconnect (dedup)
          setEvents((prev) => {
            const last = prev[prev.length - 1];
            if (last && last.type === "user" && last.content === chunk.UserMessage) {
              return prev; // already added locally
            }
            return [...prev, { type: "user", content: chunk.UserMessage, timestamp }];
          });
          // Reset streaming state for the next agent turn
          streamingRef.current = false;
          newBlockRef.current = true;
        } else if ("Attachments" in chunk) {
          setEvents((prev) => [
            ...prev,
            {
              type: "attachments" as const,
              content: "",
              timestamp,
              attachments: chunk.Attachments,
            },
          ]);
        } else if ("TextDelta" in chunk) {
          if (streamingRef.current) {
            // Streaming LLM response — accumulate into text event
            setStatusMessage(null);
            setEvents((prev) => {
              // If newBlockRef is set (after tool call or start), create a new text event
              if (!newBlockRef.current) {
                const last = prev[prev.length - 1];
                if (last && last.type === "text") {
                  const updated = [...prev];
                  updated[updated.length - 1] = {
                    ...last,
                    content: last.content + chunk.TextDelta,
                  };
                  return updated;
                }
              }
              newBlockRef.current = false;
              return [...prev, { type: "text", content: chunk.TextDelta, timestamp }];
            });
          } else {
            // Before streaming mode: transient status (not added to events)
            setStatusMessage(chunk.TextDelta);
          }
        } else if ("TextBlock" in chunk) {
          setStatusMessage(null);
          setEvents((prev) => [...prev, { type: "text", content: chunk.TextBlock, timestamp }]);
        } else if ("ToolStart" in chunk) {
          // Tool call interrupts streaming — next TextDeltas start a new text block
          newBlockRef.current = true;
          setStatusMessage(`Using ${chunk.ToolStart.name}...`);
        } else if ("ToolEnd" in chunk) {
          setStatusMessage(null);
        } else if ("StatusIndicator" in chunk) {
          if (chunk.StatusIndicator === "Processing") {
            streamingRef.current = true;
            newBlockRef.current = true;
            setStatusMessage("Agent is thinking...");
          } else if (chunk.StatusIndicator === "Done") {
            streamingRef.current = false;
            setStatusMessage(null);
          } else if (chunk.StatusIndicator === "Typing") {
            setStatusMessage("Agent is typing...");
          } else if (chunk.StatusIndicator === "UploadingMedia") {
            setStatusMessage("Uploading media...");
          }
        } else if ("Error" in chunk) {
          setStatusMessage(null);
          setEvents((prev) => [...prev, { type: "error", content: chunk.Error, timestamp }]);
        }
      } catch {
        // ignore parse errors
      }
    };

    return () => {
      ws.close();
    };
  }, [taskId]);

  useEffect(() => {
    const cleanup = connect();
    return cleanup;
  }, [connect]);

  /** Reconnect for follow-up — show user message immediately, server persists it */
  const reconnect = useCallback(
    (userMessage?: string) => {
      if (wsRef.current) {
        wsRef.current.close();
      }
      if (userMessage) {
        setEvents((prev) => [
          ...prev,
          { type: "user", content: userMessage, timestamp: new Date() },
        ]);
      }
      setDone(false);
      setConnected(false);
      setStatusMessage("Starting agent...");
      streamingRef.current = false;
      newBlockRef.current = true;
      skipHistoryRef.current = true;
      const timer = setTimeout(() => {
        connect();
      }, 300);
      return () => clearTimeout(timer);
    },
    [connect],
  );

  return { events, statusMessage, connected, done, reconnect };
}
