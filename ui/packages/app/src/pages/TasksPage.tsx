import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, useMemo } from "react";
import { useNavigate } from "react-router";
import { Plus, Zap, CheckCircle2, Server, Search, ArrowUpDown, Trash2, Tag, X } from "lucide-react";
import { api } from "../api/client";
import { TaskCard } from "../components/TaskCard";
import { ListSkeleton } from "../components/LoadingSkeleton";
import { ErrorState } from "../components/ErrorState";
import { cn } from "../lib/utils";
import type { Task } from "../api/types";

type StatusFilter = "all" | "Running" | "Completed" | "Failed" | "Pending";
type SortOrder = "newest" | "oldest";

function getStatusLabel(status: Task["status"]): string {
  if (status === "Running") return "Running";
  if (status === "Completed") return "Completed";
  if (typeof status === "object" && "Failed" in status) return "Failed";
  return "Pending";
}

export function TasksPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [sortOrder, setSortOrder] = useState<SortOrder>("newest");
  const [tagFilter, setTagFilter] = useState<string | null>(null);
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [deletingTag, setDeletingTag] = useState(false);

  const {
    data: tasks = [],
    isLoading,
    isError,
    refetch,
  } = useQuery({
    queryKey: ["tasks"],
    queryFn: api.tasks.list,
    refetchInterval: 3000,
  });

  const handleClearAll = async () => {
    setClearing(true);
    try {
      await api.tasks.clearAll();
      await queryClient.invalidateQueries({ queryKey: ["tasks"] });
      setShowClearConfirm(false);
    } catch (err) {
      console.error("Failed to clear tasks:", err);
    } finally {
      setClearing(false);
    }
  };

  const running = tasks.filter((t: Task) => t.status === "Running");
  const completed = tasks.filter((t: Task) => t.status === "Completed");
  const failed = tasks.filter(
    (t: Task) => typeof t.status === "object" && "Failed" in t.status
  );

  // Collect all unique tags across tasks
  const allTags = useMemo(() => {
    const tagSet = new Set<string>();
    for (const t of tasks) {
      for (const tag of t.tags ?? []) tagSet.add(tag);
    }
    return Array.from(tagSet).sort();
  }, [tasks]);

  const handleDeleteByTag = async (tag: string) => {
    setDeletingTag(true);
    try {
      await api.tasks.deleteByTag(tag);
      await queryClient.invalidateQueries({ queryKey: ["tasks"] });
      if (tagFilter === tag) setTagFilter(null);
    } catch (err) {
      console.error("Failed to delete by tag:", err);
    } finally {
      setDeletingTag(false);
    }
  };

  const filteredTasks = useMemo(() => {
    let result = [...tasks] as Task[];

    if (search.trim()) {
      const q = search.toLowerCase();
      result = result.filter((t) => t.prompt.toLowerCase().includes(q));
    }

    if (statusFilter !== "all") {
      result = result.filter((t) => getStatusLabel(t.status) === statusFilter);
    }

    if (tagFilter) {
      result = result.filter((t) => t.tags?.includes(tagFilter));
    }

    if (sortOrder === "oldest") {
      result.reverse();
    }

    return result;
  }, [tasks, search, statusFilter, sortOrder, tagFilter]);

  const filteredRunning = filteredTasks.filter((t) => t.status === "Running");
  const filteredOther = filteredTasks.filter((t) => t.status !== "Running");

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
        <div className="flex items-center gap-2">
          {tasks.length > 0 && (
            <button
              data-testid="clear-all-tasks"
              onClick={() => setShowClearConfirm(true)}
              className="flex items-center gap-2 px-4 py-2.5 rounded-lg border border-border text-muted-foreground hover:bg-destructive/10 hover:text-destructive hover:border-destructive/30 transition-colors text-sm"
            >
              <Trash2 className="w-4 h-4" />
              Clear All
            </button>
          )}
          <button
            onClick={() => navigate("/tasks/new")}
            className="flex items-center gap-2 px-5 py-2.5 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 transition-colors font-medium text-sm"
          >
            <Plus className="w-4 h-4" />
            New Task
          </button>
        </div>
      </div>

      {/* Stats row — click a card to filter by that status */}
      <div className="grid grid-cols-3 gap-4">
        <StatCard
          icon={Zap}
          label="Running"
          value={running.length}
          color="text-blue-400"
          active={statusFilter === "Running"}
          onClick={() => setStatusFilter(statusFilter === "Running" ? "all" : "Running")}
        />
        <StatCard
          icon={CheckCircle2}
          label="Completed"
          value={completed.length}
          color="text-emerald-400"
          active={statusFilter === "Completed"}
          onClick={() => setStatusFilter(statusFilter === "Completed" ? "all" : "Completed")}
        />
        <StatCard
          icon={Server}
          label="Failed"
          value={failed.length}
          color="text-red-400"
          active={statusFilter === "Failed"}
          onClick={() => setStatusFilter(statusFilter === "Failed" ? "all" : "Failed")}
        />
      </div>

      {/* Search + Filter toolbar */}
      <div className="flex items-center gap-3">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
          <input
            data-testid="task-search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search tasks..."
            className="w-full pl-9 pr-4 py-2 rounded-lg bg-card border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
          />
        </div>
        <select
          data-testid="task-status-filter"
          value={statusFilter}
          onChange={(e) => setStatusFilter(e.target.value as StatusFilter)}
          className="px-3 py-2 rounded-lg bg-card border border-border text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-primary/30"
        >
          <option value="all">All</option>
          <option value="Running">Running</option>
          <option value="Completed">Completed</option>
          <option value="Failed">Failed</option>
          <option value="Pending">Pending</option>
        </select>
        <button
          data-testid="task-sort-toggle"
          onClick={() => setSortOrder((o) => o === "newest" ? "oldest" : "newest")}
          className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-card border border-border text-sm hover:bg-muted transition-colors whitespace-nowrap"
        >
          <ArrowUpDown className="w-3.5 h-3.5 text-muted-foreground" />
          {sortOrder === "newest" ? "Newest" : "Oldest"}
        </button>
      </div>

      {/* Tag filter pills */}
      {allTags.length > 0 && (
        <div className="flex items-center gap-2 flex-wrap">
          <Tag className="w-3.5 h-3.5 text-muted-foreground" />
          {allTags.map((tag) => (
            <button
              key={tag}
              data-testid={`tag-filter-${tag}`}
              onClick={() => setTagFilter(tagFilter === tag ? null : tag)}
              className={cn(
                "inline-flex items-center gap-1 px-2.5 py-1 rounded-full text-xs font-medium transition-colors",
                tagFilter === tag
                  ? "bg-primary text-primary-foreground"
                  : "bg-gray-200 text-gray-700 dark:bg-gray-700 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-600"
              )}
            >
              {tag}
              {tagFilter === tag && <X className="w-3 h-3" />}
            </button>
          ))}
          {tagFilter && (
            <button
              data-testid="delete-by-tag"
              onClick={() => handleDeleteByTag(tagFilter)}
              disabled={deletingTag}
              className="inline-flex items-center gap-1 px-2.5 py-1 rounded-full text-xs font-medium text-destructive hover:bg-destructive/10 transition-colors disabled:opacity-50"
            >
              <Trash2 className="w-3 h-3" />
              {deletingTag ? "Deleting..." : `Delete "${tagFilter}"`}
            </button>
          )}
        </div>
      )}

      {/* Running tasks */}
      {filteredRunning.length > 0 && (
        <section>
          <h2 className="text-lg font-semibold mb-3 flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-blue-400 animate-pulse" />
            Running
          </h2>
          <div className="space-y-3">
            {filteredRunning.map((t: Task) => (
              <TaskCard key={t.id} task={t} />
            ))}
          </div>
        </section>
      )}

      {/* Clear All confirmation dialog */}
      {showClearConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => setShowClearConfirm(false)}>
          <div
            className="bg-card border border-border rounded-xl p-6 max-w-sm mx-4 shadow-lg space-y-4"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold">Clear All Tasks</h3>
            <p className="text-sm text-muted-foreground">
              Delete all {tasks.length} task{tasks.length !== 1 ? "s" : ""}? This cannot be undone.
            </p>
            <div className="flex items-center justify-end gap-3">
              <button
                onClick={() => setShowClearConfirm(false)}
                className="px-4 py-2 rounded-lg border border-border text-sm hover:bg-muted transition-colors"
                disabled={clearing}
              >
                Cancel
              </button>
              <button
                data-testid="confirm-clear-all"
                onClick={handleClearAll}
                disabled={clearing}
                className="px-4 py-2 rounded-lg bg-destructive text-destructive-foreground text-sm hover:bg-destructive/90 transition-colors disabled:opacity-50"
              >
                {clearing ? "Deleting..." : "Delete All"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Recent tasks */}
      <section>
        <h2 className="text-lg font-semibold mb-3">
          {search || statusFilter !== "all" || tagFilter ? (
            <span>
              Results{" "}
              <span className="text-muted-foreground font-normal text-sm">
                ({filteredTasks.length} task{filteredTasks.length !== 1 ? "s" : ""})
              </span>
            </span>
          ) : (
            "Recent"
          )}
        </h2>
        {isLoading ? (
          <ListSkeleton count={3} />
        ) : isError ? (
          <ErrorState message="Failed to load tasks" onRetry={() => refetch()} />
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
        ) : filteredOther.length === 0 && filteredRunning.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-12 text-center">
            <Search className="w-8 h-8 text-muted-foreground mb-3" />
            <p className="text-muted-foreground">No tasks match your filters</p>
            <p className="text-sm text-muted-foreground mt-1">
              Try adjusting your search or filter criteria
            </p>
            {(search || statusFilter !== "all" || tagFilter) && (
              <button
                onClick={() => { setSearch(""); setStatusFilter("all"); setTagFilter(null); }}
                className="mt-3 text-sm text-primary hover:underline"
              >
                Clear all filters
              </button>
            )}
          </div>
        ) : filteredOther.length === 0 ? (
          <p className="text-muted-foreground text-sm py-4">No additional tasks match your filters.</p>
        ) : (
          <div className="space-y-3">
            {filteredOther.map((t: Task) => (
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
  active,
  onClick,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  value: number;
  color: string;
  active?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "p-4 rounded-xl bg-card border border-border text-left w-full transition-colors",
        onClick && "cursor-pointer hover:bg-muted/50",
        active && "ring-2 ring-primary border-primary",
      )}
    >
      <div className="flex items-center gap-3">
        <Icon className={cn("w-5 h-5", color)} />
        <div>
          <p className="text-2xl font-bold">{value}</p>
          <p className="text-xs text-muted-foreground">{label}</p>
        </div>
      </div>
    </button>
  );
}
