import { useNavigate } from "react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Activity, CheckCircle2, XCircle, Trash2 } from "lucide-react";
import { cn } from "../lib/utils";
import { api } from "../api/client";
import type { Task } from "../api/types";

function statusInfo(status: Task["status"]) {
  if (status === "Running")
    return { icon: Activity, label: "Running", color: "text-blue-400", pulse: true };
  if (status === "Completed")
    return { icon: CheckCircle2, label: "Completed", color: "text-emerald-400", pulse: false };
  return { icon: XCircle, label: "Failed", color: "text-red-400", pulse: false };
}

export function TaskCard({ task }: { task: Task }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { icon: Icon, label, color, pulse } = statusInfo(task.status);

  const remove = useMutation({
    mutationFn: () => api.tasks.cancel(task.id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["tasks"] }),
  });

  return (
    <div className="flex items-center gap-2">
      <button
        onClick={() => navigate(`/tasks/${task.id}`)}
        className="flex-1 text-left p-5 rounded-xl bg-card border border-border hover:border-primary/30 hover:bg-card/80 transition-all group"
      >
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium text-foreground line-clamp-2 break-words group-hover:text-primary transition-colors">
              {task.prompt}
            </p>
            <p className="text-xs text-muted-foreground mt-1">ID: {task.id.slice(0, 8)}</p>
            {task.tags && task.tags.length > 0 && (
              <div className="flex flex-wrap gap-1 mt-1.5">
                {task.tags.map((tag) => (
                  <span
                    key={tag}
                    className="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-medium bg-gray-200 text-gray-700 dark:bg-gray-700 dark:text-gray-300"
                  >
                    {tag}
                  </span>
                ))}
              </div>
            )}
          </div>
          <div className={cn("flex items-center gap-1.5 text-xs font-medium", color)}>
            <Icon className={cn("w-4 h-4", pulse && "animate-pulse")} />
            {label}
          </div>
        </div>
      </button>
      <button
        onClick={(e) => {
          e.stopPropagation();
          remove.mutate();
        }}
        aria-label="Delete task"
        className="p-2 rounded-lg text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors shrink-0"
        title="Delete task"
      >
        <Trash2 className="w-4 h-4" />
      </button>
    </div>
  );
}
