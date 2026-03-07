import { useState, useEffect, useRef, useCallback } from "react";
import type { StreamChunk } from "../api/types";

export interface StreamEvent {
  type: "text" | "error" | "user" | "attachments" | "generated_files";
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
  // Tracks whether we have seen a Processing indicator in this session.
  // Used as a race-condition fallback: if the first TextDelta arrives before Processing
  // (e.g. WS connected after the broadcast was sent), auto-enter streaming mode.
  const seenProcessingRef = useRef(false);
  // Prevent auto-reconnect after intentional close (unmount, user-initiated reconnect).
  const intentionalCloseRef = useRef(false);
  const doneRef = useRef(false);
  const retryCountRef = useRef(0);

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
      retryCountRef.current = 0; // reset on successful connection
    };
    ws.onclose = () => {
      setConnected(false);
      // Auto-reconnect on unexpected close (server restart) — up to 5 retries with backoff
      if (!intentionalCloseRef.current && !doneRef.current && retryCountRef.current < 5) {
        const delay = Math.min(1000 * 2 ** retryCountRef.current, 8000);
        retryCountRef.current += 1;
        setTimeout(() => {
          if (!intentionalCloseRef.current && !doneRef.current) {
            // Clear events before reconnect — history will be replayed fresh, preventing duplication
            setEvents([]);
            streamingRef.current = false;
            seenProcessingRef.current = false;
            newBlockRef.current = true;
            connect();
          }
        }, delay);
      }
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
          doneRef.current = true;
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
        } else if ("GeneratedFiles" in chunk) {
          setEvents((prev) => [
            ...prev,
            {
              type: "generated_files" as const,
              content: "",
              timestamp,
              attachments: chunk.GeneratedFiles as StreamEvent["attachments"],
            },
          ]);
        } else if ("TextDelta" in chunk) {
          // Race-condition fallback: if Processing was dropped (WS connected after broadcast),
          // auto-enter streaming mode on first LLM TextDelta. Pre-Processing status messages
          // ("Starting agent...", "Workspace loaded", etc.) arrive before any LLM content and
          // keep streamingRef false, so they correctly show as transient spinners. Once the LLM
          // actually starts responding, we must be in streaming mode.
          if (!streamingRef.current && seenProcessingRef.current) {
            streamingRef.current = true;
            newBlockRef.current = true;
            setStatusMessage(null);
          }
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
            seenProcessingRef.current = true;
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
      intentionalCloseRef.current = true;
      ws.close();
    };
  }, [taskId]);

  useEffect(() => {
    intentionalCloseRef.current = false;
    doneRef.current = false;
    retryCountRef.current = 0;
    // Clear events on fresh mount / taskId change so history replay starts clean.
    // Prevents duplication from React StrictMode double-mount and taskId transitions.
    setEvents([]);
    streamingRef.current = false;
    seenProcessingRef.current = false;
    newBlockRef.current = true;
    const cleanup = connect();
    return () => {
      intentionalCloseRef.current = true;
      cleanup?.();
    };
  }, [connect]);

  /** Reconnect for follow-up — show user message immediately, server persists it.
   *  If truncateToIndex is set, discard events from that index onward (edit/retry). */
  const reconnect = useCallback(
    (userMessage?: string, truncateToIndex?: number) => {
      intentionalCloseRef.current = true; // don't auto-reconnect the old WS
      if (wsRef.current) {
        wsRef.current.close();
      }
      if (truncateToIndex !== undefined) {
        // Edit/retry: discard everything from the edited message onward
        setEvents((prev) => {
          const truncated = prev.slice(0, truncateToIndex);
          if (userMessage) {
            return [...truncated, { type: "user" as const, content: userMessage, timestamp: new Date() }];
          }
          return truncated;
        });
      } else if (userMessage) {
        setEvents((prev) => [
          ...prev,
          { type: "user", content: userMessage, timestamp: new Date() },
        ]);
      }
      setDone(false);
      setConnected(false);
      setStatusMessage("Starting agent...");
      streamingRef.current = false;
      seenProcessingRef.current = false;
      newBlockRef.current = true;
      skipHistoryRef.current = true;
      retryCountRef.current = 0;
      intentionalCloseRef.current = false; // allow auto-reconnect for new WS
      doneRef.current = false;
      const timer = setTimeout(() => {
        connect();
      }, 300);
      return () => clearTimeout(timer);
    },
    [connect],
  );

  return { events, statusMessage, connected, done, reconnect };
}
