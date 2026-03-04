import { useState, useRef, useEffect } from "react";
import { useParams, useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, StopCircle, Send, Loader2 } from "lucide-react";
import { api } from "../api/client";
import { useTaskStream } from "../hooks/useTaskStream";
import { StreamEntry } from "../components/StreamEntry";

export function TaskDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { events, statusMessage, connected, done, reconnect } = useTaskStream(id);
  const [followUp, setFollowUp] = useState("");
  const [sending, setSending] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  const { data: task } = useQuery({
    queryKey: ["task", id],
    queryFn: () => api.tasks.get(id!),
    enabled: !!id,
  });

  // Auto-scroll to bottom on new events or status changes
  const lastEventContent = events[events.length - 1]?.content;
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [events.length, lastEventContent, statusMessage]);

  const isRunning = connected && !done;

  const handleSendFollowUp = async () => {
    if (!id || !followUp.trim() || sending) return;
    const message = followUp.trim();
    setSending(true);
    try {
      await api.tasks.sendMessage(id, message);
      setFollowUp("");
      reconnect(message);
    } catch (err) {
      console.error("Failed to send follow-up:", err);
    } finally {
      setSending(false);
    }
  };

  return (
    <div className="max-w-4xl mx-auto flex flex-col h-full">
      <div className="flex items-center justify-between py-4">
        <div className="flex items-center gap-4">
          <button
            onClick={() => navigate("/")}
            className="p-2 rounded-lg hover:bg-muted transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h1 className="text-xl font-bold tracking-tight">{task?.prompt || "Task"}</h1>
            <p className="text-sm text-muted-foreground">
              {id?.slice(0, 8)} &middot;{" "}
              {connected ? (
                <span className="text-emerald-400">Connected</span>
              ) : done ? (
                "Complete"
              ) : (
                "Connecting..."
              )}
            </p>
          </div>
        </div>

        {isRunning && (
          <button
            onClick={() => id && api.tasks.cancel(id)}
            className="flex items-center gap-2 px-4 py-2 rounded-lg border border-destructive/30 text-destructive hover:bg-destructive/10 transition-colors text-sm"
          >
            <StopCircle className="w-4 h-4" />
            Cancel
          </button>
        )}
      </div>

      <div className="flex-1 overflow-y-auto space-y-3 pb-4">
        {events.length === 0 && !done && !statusMessage && (
          <div className="flex items-center justify-center py-16">
            <div className="flex items-center gap-3 text-muted-foreground">
              <Loader2 className="w-5 h-5 animate-spin text-primary" />
              Waiting for agent...
            </div>
          </div>
        )}

        {events.map((event, i) => (
          <StreamEntry key={i} event={event} />
        ))}

        {statusMessage && (
          <div className="flex items-center gap-3 px-4 py-3 text-muted-foreground">
            <Loader2 className="w-4 h-4 animate-spin text-primary" />
            <span className="text-sm">{statusMessage}</span>
          </div>
        )}

        <div ref={bottomRef} />
      </div>

      {/* Follow-up input — visible when task is done */}
      {done && (
        <div className="flex items-center gap-3 py-3 border-t border-border">
          <input
            type="text"
            value={followUp}
            onChange={(e) => setFollowUp(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && handleSendFollowUp()}
            placeholder="Send a follow-up message..."
            className="flex-1 bg-muted rounded-lg px-4 py-2.5 text-sm text-foreground placeholder:text-muted-foreground outline-none focus:ring-2 focus:ring-primary/50"
            disabled={sending}
          />
          <button
            onClick={handleSendFollowUp}
            disabled={!followUp.trim() || sending}
            className="p-2.5 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {sending ? <Loader2 className="w-4 h-4 animate-spin" /> : <Send className="w-4 h-4" />}
          </button>
        </div>
      )}
    </div>
  );
}
