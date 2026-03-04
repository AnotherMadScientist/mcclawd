import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { Plus, Zap, CheckCircle2, Server } from "lucide-react";
import { api } from "../api/client";
import { TaskCard } from "../components/TaskCard";
import { cn } from "../lib/utils";
import type { Task } from "../api/types";

export function TasksPage() {
  const navigate = useNavigate();
  const { data: tasks = [], isLoading } = useQuery({
    queryKey: ["tasks"],
    queryFn: api.tasks.list,
    refetchInterval: 3000,
  });

  const running = tasks.filter((t: Task) => t.status === "Running");
  const completed = tasks.filter((t: Task) => t.status === "Completed");
  const failed = tasks.filter(
    (t: Task) => typeof t.status === "object" && "Failed" in t.status
  );

  return (
    <div className="max-w-5xl mx-auto space-y-8">
      {/* Hero */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Tasks</h1>
          <p className="text-muted-foreground mt-1">
            Monitor and launch agent tasks
          </p>
        </div>
        <button
          onClick={() => navigate("/tasks/new")}
          className="flex items-center gap-2 px-5 py-2.5 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 transition-colors font-medium text-sm"
        >
          <Plus className="w-4 h-4" />
          New Task
        </button>
      </div>

      {/* Stats row */}
      <div className="grid grid-cols-3 gap-4">
        <StatCard icon={Zap} label="Running" value={running.length} color="text-blue-400" />
        <StatCard icon={CheckCircle2} label="Completed" value={completed.length} color="text-emerald-400" />
        <StatCard icon={Server} label="Failed" value={failed.length} color="text-red-400" />
      </div>

      {/* Running tasks */}
      {running.length > 0 && (
        <section>
          <h2 className="text-lg font-semibold mb-3 flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-blue-400 animate-pulse" />
            Running
          </h2>
          <div className="space-y-3">
            {running.map((t: Task) => (
              <TaskCard key={t.id} task={t} />
            ))}
          </div>
        </section>
      )}

      {/* Recent tasks */}
      <section>
        <h2 className="text-lg font-semibold mb-3">Recent</h2>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading...</p>
        ) : tasks.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <div className="w-16 h-16 rounded-full bg-muted flex items-center justify-center mb-4">
              <Zap className="w-8 h-8 text-muted-foreground" />
            </div>
            <p className="text-muted-foreground">No tasks yet</p>
            <p className="text-sm text-muted-foreground mt-1">
              Create your first task to get started
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {[...completed, ...failed].map((t: Task) => (
              <TaskCard key={t.id} task={t} />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function StatCard({
  icon: Icon,
  label,
  value,
  color,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  value: number;
  color: string;
}) {
  return (
    <div className="p-4 rounded-xl bg-card border border-border">
      <div className="flex items-center gap-3">
        <Icon className={cn("w-5 h-5", color)} />
        <div>
          <p className="text-2xl font-bold">{value}</p>
          <p className="text-xs text-muted-foreground">{label}</p>
        </div>
      </div>
    </div>
  );
}
