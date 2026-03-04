import { useParams, useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, StopCircle } from "lucide-react";
import { api } from "../api/client";
import { useTaskStream } from "../hooks/useTaskStream";
import { StreamEntry } from "../components/StreamEntry";

export function TaskDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { events, connected, done } = useTaskStream(id);

  const { data: task } = useQuery({
    queryKey: ["task", id],
    queryFn: () => api.tasks.get(id!),
    enabled: !!id,
  });

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <button
            onClick={() => navigate("/")}
            className="p-2 rounded-lg hover:bg-muted transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h1 className="text-xl font-bold tracking-tight">
              {task?.prompt || "Task"}
            </h1>
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

        {!done && (
          <button
            onClick={() => id && api.tasks.cancel(id)}
            className="flex items-center gap-2 px-4 py-2 rounded-lg border border-destructive/30 text-destructive hover:bg-destructive/10 transition-colors text-sm"
          >
            <StopCircle className="w-4 h-4" />
            Cancel
          </button>
        )}
      </div>

      <div className="space-y-2">
        {events.length === 0 && !done && (
          <div className="flex items-center justify-center py-16">
            <div className="flex items-center gap-3 text-muted-foreground">
              <div className="w-2 h-2 rounded-full bg-primary animate-pulse" />
              Waiting for agent...
            </div>
          </div>
        )}
        {events.map((event, i) => (
          <StreamEntry key={i} event={event} />
        ))}
      </div>
    </div>
  );
}
