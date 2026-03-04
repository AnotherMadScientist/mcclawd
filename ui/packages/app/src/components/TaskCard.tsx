import { useNavigate } from "react-router";
import { Activity, CheckCircle2, XCircle } from "lucide-react";
import { cn } from "../lib/utils";
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
  const { icon: Icon, label, color, pulse } = statusInfo(task.status);

  return (
    <button
      onClick={() => navigate(`/tasks/${task.id}`)}
      className="w-full text-left p-5 rounded-xl bg-card border border-border hover:border-primary/30 hover:bg-card/80 transition-all group"
    >
      <div className="flex items-start justify-between gap-4">
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-foreground truncate group-hover:text-primary transition-colors">
            {task.prompt}
          </p>
          <p className="text-xs text-muted-foreground mt-1">ID: {task.id.slice(0, 8)}</p>
        </div>
        <div className={cn("flex items-center gap-1.5 text-xs font-medium", color)}>
          <Icon className={cn("w-4 h-4", pulse && "animate-pulse")} />
          {label}
        </div>
      </div>
    </button>
  );
}
