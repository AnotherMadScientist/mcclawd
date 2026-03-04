import { useState } from "react";
import { Brain, Wrench, MessageSquare, AlertCircle, CheckCircle2, ChevronDown } from "lucide-react";
import { cn } from "../lib/utils";
import type { StreamEvent } from "../hooks/useTaskStream";

const typeConfig = {
  thinking: { icon: Brain, color: "text-violet-400", bg: "bg-violet-500/10", label: "Thinking" },
  "tool-start": { icon: Wrench, color: "text-amber-400", bg: "bg-amber-500/10", label: "Tool Call" },
  "tool-end": { icon: CheckCircle2, color: "text-emerald-400", bg: "bg-emerald-500/10", label: "Tool Result" },
  text: { icon: MessageSquare, color: "text-blue-400", bg: "bg-blue-500/10", label: "Response" },
  done: { icon: CheckCircle2, color: "text-emerald-400", bg: "bg-emerald-500/10", label: "Complete" },
  error: { icon: AlertCircle, color: "text-red-400", bg: "bg-red-500/10", label: "Error" },
};

export function StreamEntry({ event }: { event: StreamEvent }) {
  const [expanded, setExpanded] = useState(event.type === "text" || event.type === "error");
  const { icon: Icon, color, bg, label } = typeConfig[event.type];

  return (
    <div className="group">
      <button
        onClick={() => setExpanded(!expanded)}
        className={cn(
          "w-full flex items-start gap-3 p-3 rounded-lg transition-colors text-left",
          bg,
          "hover:opacity-90"
        )}
      >
        <div className={cn("w-8 h-8 rounded-lg flex items-center justify-center shrink-0", bg)}>
          <Icon className={cn("w-4 h-4", color)} />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className={cn("text-xs font-medium", color)}>{label}</span>
            {event.toolName && (
              <span className="text-xs text-muted-foreground font-mono">{event.toolName}</span>
            )}
            <span className="text-xs text-muted-foreground ml-auto">
              {event.timestamp.toLocaleTimeString()}
            </span>
          </div>
          {expanded && (
            <p className="text-sm text-foreground mt-1.5 whitespace-pre-wrap">{event.content}</p>
          )}
        </div>
        <ChevronDown
          className={cn(
            "w-4 h-4 text-muted-foreground transition-transform shrink-0 mt-1",
            expanded && "rotate-180"
          )}
        />
      </button>
    </div>
  );
}
