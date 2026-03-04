import { Puzzle } from "lucide-react";

export function SkillsPage() {
  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Skills</h1>
      <div className="rounded-xl border border-border bg-card p-8">
        <div className="flex flex-col items-center justify-center py-16">
          <Puzzle className="w-12 h-12 text-muted-foreground mb-4" />
          <p className="text-muted-foreground">No skills installed</p>
          <p className="text-sm text-muted-foreground mt-1">
            ClawHub integration coming in Phase 1+
          </p>
          <p className="text-sm text-muted-foreground mt-4">
            Install skills via CLI:{" "}
            <code className="px-2 py-0.5 rounded bg-muted font-mono text-xs">
              mc skills install &lt;name&gt;
            </code>
          </p>
        </div>
      </div>
    </div>
  );
}
