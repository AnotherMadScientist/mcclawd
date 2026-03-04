import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import { api } from "../api/client";

export function SecretsPage() {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const queryClient = useQueryClient();

  const { data: secrets = [] } = useQuery({
    queryKey: ["secrets"],
    queryFn: api.secrets.list,
  });

  const add = useMutation({
    mutationFn: () => api.secrets.add(name, value),
    onSuccess: () => {
      setName("");
      setValue("");
      queryClient.invalidateQueries({ queryKey: ["secrets"] });
    },
  });

  const remove = useMutation({
    mutationFn: (n: string) => api.secrets.delete(n),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["secrets"] }),
  });

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Secrets</h1>
      <p className="text-sm text-muted-foreground">
        Encrypted secrets for API keys. Values are never displayed.
      </p>

      <div className="flex gap-3">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Secret name (e.g. ANTHROPIC_API_KEY)"
          className="flex-1 px-4 py-2 rounded-lg bg-card border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
        />
        <input
          type="password"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder="Value"
          className="flex-1 px-4 py-2 rounded-lg bg-card border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
        />
        <button
          onClick={() => add.mutate()}
          disabled={!name || !value}
          aria-label="Add secret"
          className="px-4 py-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40 text-sm"
        >
          <Plus className="w-4 h-4" />
        </button>
      </div>

      <div className="space-y-2">
        {secrets.map((s) => (
          <div
            key={s.name}
            className="flex items-center justify-between p-4 rounded-xl bg-card border border-border"
          >
            <div className="flex items-center gap-3">
              <KeyRound className="w-4 h-4 text-amber-400" />
              <span className="text-sm font-mono">{s.name}</span>
            </div>
            <button
              onClick={() => remove.mutate(s.name)}
              className="p-2 rounded-lg text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
            >
              <Trash2 className="w-4 h-4" />
            </button>
          </div>
        ))}
        {secrets.length === 0 && (
          <p className="text-sm text-muted-foreground text-center py-8">No secrets stored</p>
        )}
      </div>
    </div>
  );
}
