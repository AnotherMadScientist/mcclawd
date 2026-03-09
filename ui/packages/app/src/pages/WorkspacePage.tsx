import { useState, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { FileText, Save, ChevronDown, BookmarkPlus, Trash2 } from "lucide-react";
import { api } from "../api/client";
import { cn } from "../lib/utils";

const files = ["SOUL.md", "AGENTS.md", "USER.md", "IDENTITY.md", "TOOLS.md", "HEARTBEAT.md"];

export function WorkspacePage() {
  const [selected, setSelected] = useState("SOUL.md");
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const [profileMenuOpen, setProfileMenuOpen] = useState(false);
  const [saveDialogOpen, setSaveDialogOpen] = useState(false);
  const [newProfileName, setNewProfileName] = useState("");
  const [newProfileDesc, setNewProfileDesc] = useState("");
  const [confirmApply, setConfirmApply] = useState<string | null>(null);
  const queryClient = useQueryClient();

  const { data, isLoading } = useQuery({
    queryKey: ["workspace", selected],
    queryFn: () => api.workspace.get(selected),
  });

  const { data: profiles } = useQuery({
    queryKey: ["workspace-profiles"],
    queryFn: () => api.workspace.profiles(),
  });

  useEffect(() => {
    if (data && !dirty) {
      setContent(data.content || "");
    }
  }, [data, dirty]);

  const save = useMutation({
    mutationFn: (params: { file: string; content: string }) =>
      api.workspace.update(params.file, params.content),
    onSuccess: (_data, variables) => {
      setDirty(false);
      queryClient.setQueryData(["workspace", variables.file], {
        name: variables.file,
        content: variables.content,
      });
      setToast({ msg: "Saved successfully", ok: true });
      setTimeout(() => setToast(null), 2500);
    },
    onError: () => {
      setToast({ msg: "Failed to save", ok: false });
      setTimeout(() => setToast(null), 2500);
    },
  });

  const applyProfile = useMutation({
    mutationFn: (name: string) => api.workspace.applyProfile(name),
    onSuccess: (_data, name) => {
      setDirty(false);
      // Invalidate ALL workspace file queries (each tab has ["workspace", filename])
      // and the profiles list so active badge updates
      queryClient.invalidateQueries({ queryKey: ["workspace"] });
      queryClient.invalidateQueries({ queryKey: ["workspace-profiles"] });
      // Force refetch the currently selected file immediately so the textarea updates
      queryClient.refetchQueries({ queryKey: ["workspace", selected] });
      setToast({ msg: `Profile "${name}" applied`, ok: true });
      setTimeout(() => setToast(null), 2500);
      setConfirmApply(null);
    },
    onError: () => {
      setToast({ msg: "Failed to apply profile", ok: false });
      setTimeout(() => setToast(null), 2500);
      setConfirmApply(null);
    },
  });

  const saveProfile = useMutation({
    mutationFn: ({ name, description }: { name: string; description: string }) =>
      api.workspace.saveProfile(name, description),
    onSuccess: (_data, vars) => {
      queryClient.invalidateQueries({ queryKey: ["workspace-profiles"] });
      setToast({ msg: `Profile "${vars.name}" saved`, ok: true });
      setTimeout(() => setToast(null), 2500);
      setSaveDialogOpen(false);
      setNewProfileName("");
      setNewProfileDesc("");
    },
    onError: () => {
      setToast({ msg: "Failed to save profile", ok: false });
      setTimeout(() => setToast(null), 2500);
    },
  });

  const deleteProfile = useMutation({
    mutationFn: (name: string) => api.workspace.deleteProfile(name),
    onSuccess: (_data, name) => {
      queryClient.invalidateQueries({ queryKey: ["workspace-profiles"] });
      setToast({ msg: `Profile "${name}" deleted`, ok: true });
      setTimeout(() => setToast(null), 2500);
    },
    onError: () => {
      setToast({ msg: "Failed to delete profile", ok: false });
      setTimeout(() => setToast(null), 2500);
    },
  });

  const handleChange = (value: string) => {
    setContent(value);
    setDirty(true);
  };

  const handleSave = () => {
    save.mutate({ file: selected, content });
  };

  const handleTabSwitch = (file: string) => {
    if (dirty && !window.confirm("You have unsaved changes. Discard them?")) {
      return;
    }
    setDirty(false);
    setSelected(file);
  };

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Workspace Files</h1>

        {/* Profile selector */}
        <div className="relative">
          <button
            onClick={() => setProfileMenuOpen(!profileMenuOpen)}
            className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-card border border-border text-sm hover:bg-muted transition-colors"
          >
            Profiles
            <ChevronDown className="w-4 h-4" />
          </button>

          {profileMenuOpen && (
            <>
              <div
                className="fixed inset-0 z-40"
                onClick={() => setProfileMenuOpen(false)}
              />
              <div className="absolute right-0 top-full mt-1 z-50 w-72 rounded-lg border border-border bg-card shadow-lg">
                <div className="p-2 border-b border-border">
                  <p className="text-xs text-muted-foreground px-2 py-1">
                    Apply a profile to overwrite all workspace files
                  </p>
                </div>
                <div className="max-h-60 overflow-y-auto p-1">
                  {profiles?.map((p) => (
                    <div
                      key={p.name}
                      className="flex items-center justify-between px-2 py-1.5 rounded hover:bg-muted group"
                    >
                      <button
                        className="flex-1 text-left"
                        onClick={() => {
                          setProfileMenuOpen(false);
                          setConfirmApply(p.name);
                        }}
                      >
                        <span className="text-sm font-medium">{p.name}</span>
                        {p.builtin && (
                          <span className="ml-2 text-[10px] px-1.5 py-0.5 rounded bg-primary/10 text-primary">
                            built-in
                          </span>
                        )}
                        <p className="text-xs text-muted-foreground truncate">
                          {p.description}
                        </p>
                      </button>
                      {!p.builtin && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            deleteProfile.mutate(p.name);
                          }}
                          className="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive transition-all"
                          title="Delete profile"
                        >
                          <Trash2 className="w-3.5 h-3.5" />
                        </button>
                      )}
                    </div>
                  ))}
                </div>
                <div className="p-2 border-t border-border">
                  <button
                    onClick={() => {
                      setProfileMenuOpen(false);
                      setSaveDialogOpen(true);
                    }}
                    className="flex items-center gap-2 w-full px-2 py-1.5 rounded text-sm hover:bg-muted transition-colors"
                  >
                    <BookmarkPlus className="w-4 h-4" />
                    Save current as profile...
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      </div>

      <div className="flex gap-2">
        {files.map((f) => (
          <button
            key={f}
            onClick={() => handleTabSwitch(f)}
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
          onChange={(e) => handleChange(e.target.value)}
          className="w-full h-96 p-4 rounded-xl bg-card border border-border font-mono text-sm resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
          disabled={isLoading}
        />
        <button
          onClick={handleSave}
          disabled={save.isPending || !dirty}
          className="absolute top-3 right-3 flex items-center gap-2 px-3 py-1.5 rounded-lg bg-primary/10 text-primary hover:bg-primary/20 text-sm transition-colors disabled:opacity-40"
        >
          <Save className="w-4 h-4" />
          {save.isPending ? "Saving..." : "Save"}
        </button>
      </div>
      {toast && (
        <p className={`text-xs ${toast.ok ? "text-emerald-500" : "text-destructive"}`}>
          {toast.msg}
        </p>
      )}

      {/* Confirm apply dialog */}
      {confirmApply && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="bg-card rounded-xl border border-border p-6 max-w-sm w-full mx-4 space-y-4">
            <h2 className="text-lg font-semibold">Apply Profile</h2>
            <p className="text-sm text-muted-foreground">
              This will overwrite all 6 workspace files with the "{confirmApply}" profile content.
              Unsaved changes will be lost.
            </p>
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setConfirmApply(null)}
                className="px-4 py-2 rounded-lg text-sm hover:bg-muted transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={() => applyProfile.mutate(confirmApply)}
                disabled={applyProfile.isPending}
                className="px-4 py-2 rounded-lg text-sm bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
              >
                {applyProfile.isPending ? "Applying..." : "Apply"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Save profile dialog */}
      {saveDialogOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="bg-card rounded-xl border border-border p-6 max-w-sm w-full mx-4 space-y-4">
            <h2 className="text-lg font-semibold">Save as Profile</h2>
            <p className="text-sm text-muted-foreground">
              Save the current workspace files as a reusable profile.
            </p>
            <div className="space-y-3">
              <input
                type="text"
                placeholder="Profile name"
                value={newProfileName}
                onChange={(e) => setNewProfileName(e.target.value)}
                className="w-full px-3 py-2 rounded-lg bg-background border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
              />
              <input
                type="text"
                placeholder="Description (optional)"
                value={newProfileDesc}
                onChange={(e) => setNewProfileDesc(e.target.value)}
                className="w-full px-3 py-2 rounded-lg bg-background border border-border text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
              />
            </div>
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => {
                  setSaveDialogOpen(false);
                  setNewProfileName("");
                  setNewProfileDesc("");
                }}
                className="px-4 py-2 rounded-lg text-sm hover:bg-muted transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={() =>
                  saveProfile.mutate({
                    name: newProfileName,
                    description: newProfileDesc,
                  })
                }
                disabled={!newProfileName.trim() || saveProfile.isPending}
                className="px-4 py-2 rounded-lg text-sm bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
              >
                {saveProfile.isPending ? "Saving..." : "Save"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
