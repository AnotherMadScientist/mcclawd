import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { AlertCircle, User } from "lucide-react";
import { cn } from "../lib/utils";
import type { StreamEvent } from "../hooks/useTaskStream";

export function StreamEntry({ event }: { event: StreamEvent }) {
  if (event.type === "user") {
    return (
      <div className="flex justify-end">
        <div className="flex items-start gap-3 max-w-[80%]">
          <div className="rounded-2xl rounded-tr-sm bg-primary/15 border border-primary/20 px-4 py-2.5">
            <p className="text-sm text-foreground whitespace-pre-wrap">{event.content}</p>
          </div>
          <div className="w-8 h-8 rounded-full bg-primary/20 flex items-center justify-center shrink-0">
            <User className="w-4 h-4 text-primary" />
          </div>
        </div>
      </div>
    );
  }

  if (event.type === "error") {
    return (
      <div className="flex items-start gap-3 rounded-lg bg-red-500/10 border border-red-500/20 p-4">
        <AlertCircle className="w-5 h-5 text-red-400 shrink-0 mt-0.5" />
        <p className="text-sm text-red-300 whitespace-pre-wrap">{event.content}</p>
      </div>
    );
  }

  // text response — rendered as markdown
  return (
    <div className={cn("rounded-lg bg-card border border-border p-4", "agent-response")}>
      <div className="prose-response text-sm text-foreground">
        <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
          {event.content}
        </Markdown>
      </div>
    </div>
  );
}
