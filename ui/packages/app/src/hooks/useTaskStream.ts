import { useState, useEffect, useRef } from "react";
import type { StreamChunk } from "../api/types";

export interface StreamEvent {
  type: "thinking" | "tool-start" | "tool-end" | "text" | "done" | "error";
  content: string;
  toolName?: string;
  timestamp: Date;
}

export function useTaskStream(taskId: string | undefined) {
  const [events, setEvents] = useState<StreamEvent[]>([]);
  const [connected, setConnected] = useState(false);
  const [done, setDone] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!taskId) return;

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const token = localStorage.getItem("mcclawd_token") || "";
    const wsUrl = `${protocol}//${window.location.host}/api/tasks/${taskId}/stream?token=${encodeURIComponent(token)}`;
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => setConnected(true);
    ws.onclose = () => {
      setConnected(false);
      setDone(true);
    };
    ws.onmessage = (event) => {
      try {
        const chunk: StreamChunk = JSON.parse(event.data);
        const timestamp = new Date();

        if (chunk === "Done") {
          setEvents((prev) => [...prev, { type: "done", content: "Task complete", timestamp }]);
          setDone(true);
          return;
        }

        if ("TextDelta" in chunk) {
          setEvents((prev) => [...prev, { type: "thinking", content: chunk.TextDelta, timestamp }]);
        } else if ("TextBlock" in chunk) {
          setEvents((prev) => [...prev, { type: "text", content: chunk.TextBlock, timestamp }]);
        } else if ("ToolStart" in chunk) {
          setEvents((prev) => [
            ...prev,
            { type: "tool-start", content: `Calling ${chunk.ToolStart.name}...`, toolName: chunk.ToolStart.name, timestamp },
          ]);
        } else if ("ToolEnd" in chunk) {
          setEvents((prev) => [
            ...prev,
            {
              type: "tool-end",
              content: chunk.ToolEnd.summary || "Done",
              toolName: chunk.ToolEnd.name,
              timestamp,
            },
          ]);
        } else if ("Error" in chunk) {
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

  return { events, connected, done };
}
