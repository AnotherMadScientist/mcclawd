import { useState, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { FileText, Save } from "lucide-react";
import { api } from "../api/client";
import { cn } from "../lib/utils";

const files = ["SOUL.md", "AGENTS.md", "USER.md"];

export function WorkspacePage() {
  const [selected, setSelected] = useState("SOUL.md");
  const [content, setContent] = useState("");
  const queryClient = useQueryClient();

  const { data, isLoading } = useQuery({
    queryKey: ["workspace", selected],
    queryFn: () => api.workspace.get(selected),
  });

  useEffect(() => {
    if (data) {
      setContent(data.content || "");
    }
  }, [data]);

  const save = useMutation({
    mutationFn: () => api.workspace.update(selected, content),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["workspace", selected] }),
  });

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Workspace Files</h1>

      <div className="flex gap-2">
        {files.map((f) => (
          <button
            key={f}
            onClick={() => setSelected(f)}
            className={cn(
              "flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors",
              selected === f
                ? "bg-primary/10 text-primary border border-primary/20"
                : "text-muted-foreground hover:bg-muted"
            )}
          >
            <FileText className="w-4 h-4" />
            {f}
          </button>
        ))}
      </div>

      <div className="relative">
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          className="w-full h-96 p-4 rounded-xl bg-card border border-border font-mono text-sm resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
          disabled={isLoading}
        />
        <button
          onClick={() => save.mutate()}
          disabled={save.isPending}
          className="absolute top-3 right-3 flex items-center gap-2 px-3 py-1.5 rounded-lg bg-primary/10 text-primary hover:bg-primary/20 text-sm transition-colors"
        >
          <Save className="w-4 h-4" />
          {save.isPending ? "Saving..." : "Save"}
        </button>
      </div>
    </div>
  );
}
