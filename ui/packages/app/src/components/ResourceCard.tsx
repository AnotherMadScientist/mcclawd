import { cn } from "../lib/utils";

interface ResourceCardProps {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  description: string;
  items?: string[];
  status?: "active" | "inactive";
  color?: string;
}

export function ResourceCard({
  icon: Icon,
  title,
  description,
  items,
  status,
  color = "text-primary",
}: ResourceCardProps) {
  return (
    <div className="p-4 rounded-xl bg-card border border-border hover:border-primary/20 transition-colors">
      <div className="flex items-start gap-3">
        <div
          className={cn(
            "w-10 h-10 rounded-lg flex items-center justify-center shrink-0",
            status === "active" ? "bg-emerald-500/10" : "bg-muted"
          )}
        >
          <Icon className={cn("w-5 h-5", color)} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-medium">{title}</h3>
            {status && (
              <span
                className={cn(
                  "w-2 h-2 rounded-full",
                  status === "active" ? "bg-emerald-400" : "bg-zinc-600"
                )}
              />
            )}
          </div>
          <p className="text-xs text-muted-foreground mt-0.5">{description}</p>
          {items && items.length > 0 && (
            <div className="flex flex-wrap gap-1.5 mt-2">
              {items.map((item) => (
                <span
                  key={item}
                  className="px-2 py-0.5 rounded-md bg-muted text-xs text-muted-foreground"
                >
                  {item}
                </span>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
