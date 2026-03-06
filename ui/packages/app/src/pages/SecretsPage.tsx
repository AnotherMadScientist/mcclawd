import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Plus, Trash2, Eye, EyeOff, Pencil, Check, X, Copy } from "lucide-react";
import { api } from "../api/client";

export function SecretsPage() {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [revealed, setRevealed] = useState<Record<string, string>>({});
  const [editing, setEditing] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [copied, setCopied] = useState<string | null>(null);
  const queryClient = useQueryClient();

  const handleCopy = (secretName: string) => {
    const val = revealed[secretName];
    if (val === undefined) return;
    navigator.clipboard.writeText(val).then(() => {
      setCopied(secretName);
      setTimeout(() => setCopied(null), 1500);
    });
  };

  const { data: secrets = [] } = useQuery({
    queryKey: ["secrets"],
    queryFn: api.secrets.list,
    select: (data) =>
      [...data].sort((a, b) => a.name.localeCompare(b.name)),
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

  const update = useMutation({
    mutationFn: ({ n, v }: { n: string; v: string }) => api.secrets.update(n, v),
    onSuccess: () => {
      setEditing(null);
      setEditValue("");
      // Clear revealed cache for updated secret
      setRevealed((prev) => {
        const next = { ...prev };
        delete next[editing!];
        return next;
      });
      queryClient.invalidateQueries({ queryKey: ["secrets"] });
    },
  });

  const toggleReveal = async (secretName: string) => {
    if (revealed[secretName] !== undefined) {
      setRevealed((prev) => {
        const next = { ...prev };
        delete next[secretName];
        return next;
      });
    } else {
      try {
        const data = await api.secrets.get(secretName);
        setRevealed((prev) => ({ ...prev, [secretName]: data.value }));
      } catch {
        // ignore errors
      }
    }
  };

  const startEdit = async (secretName: string) => {
    try {
      const data = await api.secrets.get(secretName);
      setEditValue(data.value);
      setEditing(secretName);
    } catch {
      // ignore
    }
  };

  const cancelEdit = () => {
    setEditing(null);
    setEditValue("");
  };

  const saveEdit = (secretName: string) => {
    if (editValue) {
      update.mutate({ n: secretName, v: editValue });
    }
  };

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">Secrets</h1>
      <p className="text-sm text-muted-foreground">
        Encrypted secrets for API keys. Click the eye icon to reveal values.
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
            data-testid={`secret-row-${s.name}`}
            className="flex items-center justify-between p-4 rounded-xl bg-card border border-border"
          >
            <div className="flex items-center gap-3 flex-1 min-w-0">
              <KeyRound className="w-4 h-4 text-amber-400 shrink-0" />
              <span className="text-sm font-mono">{s.name}</span>
              {editing === s.name ? (
                <input
                  type="text"
                  value={editValue}
                  onChange={(e) => setEditValue(e.target.value)}
                  aria-label="Edit secret value"
                  className="flex-1 px-3 py-1 rounded-lg bg-background border border-border text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"
                />
              ) : revealed[s.name] !== undefined ? (
                <>
                  <span className="text-sm font-mono text-muted-foreground" data-testid="revealed-value">
                    {revealed[s.name]}
                  </span>
                  <button
                    onClick={() => handleCopy(s.name)}
                    aria-label="Copy secret value"
                    className="p-1 rounded text-muted-foreground hover:text-foreground transition-colors shrink-0"
                  >
                    {copied === s.name ? (
                      <span className="text-xs text-emerald-500">Copied!</span>
                    ) : (
                      <Copy className="w-3.5 h-3.5" />
                    )}
                  </button>
                </>
              ) : null}
            </div>
            <div className="flex items-center gap-1 shrink-0">
              {editing === s.name ? (
                <>
                  <button
                    onClick={() => saveEdit(s.name)}
                    aria-label="Save secret"
                    className="p-2 rounded-lg text-muted-foreground hover:text-green-500 hover:bg-green-500/10 transition-colors"
                  >
                    <Check className="w-4 h-4" />
                  </button>
                  <button
                    onClick={cancelEdit}
                    aria-label="Cancel edit"
                    className="p-2 rounded-lg text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
                  >
                    <X className="w-4 h-4" />
                  </button>
                </>
              ) : (
                <>
                  <button
                    onClick={() => toggleReveal(s.name)}
                    aria-label={revealed[s.name] !== undefined ? "Hide secret" : "Show secret"}
                    className="p-2 rounded-lg text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                  >
                    {revealed[s.name] !== undefined ? (
                      <EyeOff className="w-4 h-4" />
                    ) : (
                      <Eye className="w-4 h-4" />
                    )}
                  </button>
                  <button
                    onClick={() => startEdit(s.name)}
                    aria-label="Edit secret"
                    className="p-2 rounded-lg text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                  >
                    <Pencil className="w-4 h-4" />
                  </button>
                  <button
                    onClick={() => remove.mutate(s.name)}
                    aria-label="Delete secret"
                    className="p-2 rounded-lg text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </>
              )}
            </div>
          </div>
        ))}
        {secrets.length === 0 && (
          <p className="text-sm text-muted-foreground text-center py-8">No secrets stored</p>
        )}
      </div>
    </div>
  );
}
