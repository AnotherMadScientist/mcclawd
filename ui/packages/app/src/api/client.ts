import type {
  PublicKeyCredentialCreationOptionsJSON,
  PublicKeyCredentialRequestOptionsJSON,
  RegistrationResponseJSON,
  AuthenticationResponseJSON,
} from "@simplewebauthn/browser";
import type { AttachmentMeta, McclawdConfig, McpServer, Task, WorkspaceFile, InstalledSkill, CachedSearchResult, ClawHubSkillMeta, ScanResult, Provider, DetailedUsageSummary, BudgetInfo, BudgetUpdate, AnthropicModel, ModelPricing, CreditsResponse, DockerBuildStatus, ContainerInfo, ContainerDetail, SecurityEvent, SecuritySummary, SecurityStatus, DlpPolicy, DlpPatternInfo, TaskSecurityGroup } from "./types";

const TOKEN_KEY = "mcclawd_token";

export function setToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
}

export function getToken() {
  return localStorage.getItem(TOKEN_KEY);
}

async function apiFetch<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...((options.headers as Record<string, string>) || {}),
  };

  const token = getToken();
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const res = await fetch(path, { ...options, headers });

  if (res.status === 401 && getToken()) {
    clearToken();
    window.location.href = "/login";
    throw new Error("Session expired");
  }

  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${res.statusText}`);
  }

  if (res.status === 204) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text);
}

export const api = {
  health: {
    check: async () => {
      const token = getToken();
      const headers: Record<string, string> = {};
      if (token) headers["Authorization"] = `Bearer ${token}`;
      const res = await fetch("/api/health", { headers });
      return { ok: res.ok };
    },
    llm: () => apiFetch<{ ok: boolean; error?: string }>("/api/health/llm"),
  },
  auth: {
    login: (password: string) =>
      apiFetch<{ token: string }>("/api/auth/login", {
        method: "POST",
        body: JSON.stringify({ password }),
      }),
    status: () =>
      apiFetch<{ setup_complete: boolean }>("/api/auth/status"),
    registerStart: () =>
      apiFetch<PublicKeyCredentialCreationOptionsJSON>("/api/auth/register/start", {
        method: "POST",
      }),
    registerFinish: (credential: RegistrationResponseJSON) =>
      apiFetch<{ token: string }>("/api/auth/register/finish", {
        method: "POST",
        body: JSON.stringify(credential),
      }),
    loginStart: () =>
      apiFetch<PublicKeyCredentialRequestOptionsJSON>("/api/auth/login/start", {
        method: "POST",
      }),
    loginFinish: (credential: AuthenticationResponseJSON) =>
      apiFetch<{ token: string }>("/api/auth/login/finish", {
        method: "POST",
        body: JSON.stringify(credential),
      }),
  },
  tasks: {
    list: () => apiFetch<Task[]>("/api/tasks"),
    create: (prompt: string, workspace?: string, model?: string, delayStart?: boolean, tags?: string[], skills?: string[], toolProfile?: string) =>
      apiFetch<Task>("/api/tasks", {
        method: "POST",
        body: JSON.stringify({ prompt, workspace, model, delay_start: delayStart ?? false, tags, skills, tool_profile: toolProfile }),
      }),
    get: (id: string) => apiFetch<Task>(`/api/tasks/${id}`),
    cancel: (id: string) => apiFetch<void>(`/api/tasks/${id}`, { method: "DELETE" }),
    clearAll: () => apiFetch<{ deleted: number }>("/api/tasks", { method: "DELETE" }),
    deleteByTag: (tag: string) =>
      apiFetch<{ deleted: number }>(`/api/tasks?tag=${encodeURIComponent(tag)}`, { method: "DELETE" }),
    sendMessage: (id: string, message: string, truncateHistoryTo?: number, addSkills?: string[]) =>
      apiFetch<void>(`/api/tasks/${id}/message`, {
        method: "POST",
        body: JSON.stringify({
          message,
          truncate_history_to: truncateHistoryTo ?? null,
          add_skills: addSkills ?? null,
        }),
      }),
    uploadAttachments: async (taskId: string, files: File[]) => {
      if (!files.length) return [] as AttachmentMeta[];
      const formData = new FormData();
      for (const file of files) {
        formData.append("files", file);
      }
      const token = getToken();
      const headers: Record<string, string> = {};
      if (token) headers["Authorization"] = `Bearer ${token}`;
      // Do NOT set Content-Type — browser sets it with boundary for FormData
      const res = await fetch(`/api/tasks/${taskId}/attachments`, {
        method: "POST",
        headers,
        body: formData,
      });
      if (!res.ok) {
        const body = await res.text().catch(() => "");
        console.error(
          `[uploadAttachments] ${res.status} ${res.statusText}`,
          { taskId, fileCount: files.length, body },
        );
        throw new Error(`Upload failed: ${res.status} — ${body || res.statusText}`);
      }
      return res.json() as Promise<AttachmentMeta[]>;
    },
    listAttachments: (taskId: string) =>
      apiFetch<AttachmentMeta[]>(`/api/tasks/${taskId}/attachments`),
    listFiles: (taskId: string) =>
      apiFetch<AttachmentMeta[]>(`/api/tasks/${taskId}/files`),
    downloadFileUrl: (taskId: string, filename: string) =>
      `/api/tasks/${taskId}/files/${encodeURIComponent(filename)}`,
  },
  workspace: {
    list: () => apiFetch<WorkspaceFile[]>("/api/workspace"),
    get: (file: string) => apiFetch<WorkspaceFile>(`/api/workspace/${file}`),
    update: (file: string, content: string) =>
      apiFetch<void>(`/api/workspace/${file}`, {
        method: "PUT",
        body: JSON.stringify({ content }),
      }),
    profiles: () =>
      apiFetch<{ name: string; description: string; builtin: boolean }[]>(
        "/api/workspace/profiles",
      ),
    applyProfile: (name: string) =>
      apiFetch<{ applied: string }>(`/api/workspace/profiles/${name}/apply`, {
        method: "POST",
      }),
    saveProfile: (name: string, description?: string) =>
      apiFetch<{ saved: string }>(`/api/workspace/profiles/${name}/save`, {
        method: "POST",
        body: JSON.stringify({ description: description || "" }),
      }),
    deleteProfile: (name: string) =>
      apiFetch<{ deleted: string }>(`/api/workspace/profiles/${name}`, {
        method: "DELETE",
      }),
  },
  worldmonitor: {
    status: () => apiFetch<{ running: boolean; status?: number }>("/api/worldmonitor/status"),
    syncEnv: () =>
      apiFetch<{ synced: number; keys: string[] }>("/api/worldmonitor/sync-env", {
        method: "POST",
      }),
  },
  secrets: {
    list: () => apiFetch<{ name: string }[]>("/api/secrets"),
    get: (name: string) => apiFetch<{ name: string; value: string }>(`/api/secrets/${name}`),
    add: (name: string, value: string) =>
      apiFetch<void>("/api/secrets", {
        method: "POST",
        body: JSON.stringify({ name, value }),
      }),
    update: (name: string, value: string) =>
      apiFetch<void>(`/api/secrets/${name}`, {
        method: "PUT",
        body: JSON.stringify({ value }),
      }),
    delete: (name: string) => apiFetch<void>(`/api/secrets/${name}`, { method: "DELETE" }),
  },
  config: {
    get: () => apiFetch<McclawdConfig>("/api/config"),
    update: (update: { model?: string; max_turns?: number; default_workspace?: string; default_tool_profile?: string }) =>
      apiFetch<void>("/api/config", {
        method: "PUT",
        body: JSON.stringify(update),
      }),
  },
  mcp: {
    servers: () => apiFetch<McpServer[]>("/api/mcp/servers"),
    addServer: (server: {
      name: string;
      image: string;
      port: number;
      command?: string;
      args?: string[];
      env?: Record<string, string>;
    }) =>
      apiFetch<McpServer>("/api/mcp/servers", {
        method: "POST",
        body: JSON.stringify(server),
      }),
    removeServer: (name: string) =>
      apiFetch<void>(`/api/mcp/servers/${encodeURIComponent(name)}`, {
        method: "DELETE",
      }),
    restartServer: (name: string) =>
      apiFetch<void>(`/api/mcp/servers/${encodeURIComponent(name)}/restart`, {
        method: "POST",
      }),
  },
  skills: {
    list: () => apiFetch<InstalledSkill[]>("/api/skills"),
    catalog: (query = "", page = 0, perPage = 50) =>
      apiFetch<CachedSearchResult>(
        `/api/skills/catalog?q=${encodeURIComponent(query)}&page=${page}&per_page=${perPage}`,
      ),
    detail: (name: string) =>
      apiFetch<ClawHubSkillMeta>(`/api/skills/catalog/${encodeURIComponent(name)}`),
    content: (name: string) =>
      apiFetch<{ name: string; content: string }>(`/api/skills/${encodeURIComponent(name)}/content`),
    refresh: () =>
      apiFetch<{ refreshed: number }>("/api/skills/refresh", { method: "POST" }),
    create: (name: string, content: string) =>
      apiFetch<{ name: string; path: string }>("/api/skills/create", {
        method: "POST",
        body: JSON.stringify({ name, content }),
      }),
    install: (name: string, version?: string) =>
      apiFetch<InstalledSkill>("/api/skills/install", {
        method: "POST",
        body: JSON.stringify({ name, version }),
      }),
    uninstall: (name: string) =>
      apiFetch<void>(`/api/skills/${encodeURIComponent(name)}`, { method: "DELETE" }),
    scan: (name: string) =>
      apiFetch<ScanResult>(`/api/skills/${encodeURIComponent(name)}/scan`),
    previewScan: (name: string) =>
      apiFetch<ScanResult>(`/api/skills/${encodeURIComponent(name)}/preview-scan`, {
        method: "POST",
      }),
    upgradeStubs: () =>
      apiFetch<{ upgraded: number; failed: number }>("/api/skills/upgrade-stubs", {
        method: "POST",
      }),
  },
  providers: {
    list: () => apiFetch<Provider[]>("/api/providers"),
    models: () => apiFetch<AnthropicModel[]>("/api/providers/models"),
    pricing: () => apiFetch<ModelPricing[]>("/api/providers/pricing"),
    usage: (granularity?: string) =>
      apiFetch<DetailedUsageSummary>(
        `/api/providers/usage/detailed${granularity ? `?granularity=${granularity}` : ""}`,
      ),
    budgetInfo: () => apiFetch<BudgetInfo>("/api/providers/budget/info"),
    setBudget: (budget: BudgetUpdate) =>
      apiFetch<{ status: string }>("/api/providers/budget", {
        method: "PUT",
        body: JSON.stringify(budget),
      }),
    credits: () => apiFetch<CreditsResponse>("/api/providers/credits"),
  },
  systemAgent: {
    chat: (message: string) =>
      apiFetch<{ task_id: string }>("/api/system-agent/chat", {
        method: "POST",
        body: JSON.stringify({ message }),
      }),
    history: () => apiFetch<unknown[]>("/api/system-agent/history"),
    clearHistory: () =>
      apiFetch<void>("/api/system-agent/history", { method: "DELETE" }),
  },
  docker: {
    buildStatus: () => apiFetch<DockerBuildStatus>("/api/docker/build-status"),
    triggerBuild: () => apiFetch<{ status: string }>("/api/docker/build", { method: "POST" }),
    containers: () => apiFetch<ContainerInfo[]>("/api/docker/containers"),
    container: (id: string) =>
      apiFetch<ContainerDetail>(`/api/docker/containers/${encodeURIComponent(id)}`),
    deleteContainer: (id: string) =>
      apiFetch<{ deleted: boolean; container_id: string; task_id?: string }>(
        `/api/docker/containers/${encodeURIComponent(id)}`,
        { method: "DELETE" },
      ),
  },
  security: {
    events: (taskId?: string, since?: string) => {
      const params = new URLSearchParams();
      if (taskId) params.set("task_id", taskId);
      if (since) params.set("since", since);
      return apiFetch<SecurityEvent[]>(`/api/security/events?${params}`);
    },
    eventsGrouped: (since?: string) => {
      const params = new URLSearchParams();
      if (since) params.set("since", since);
      return apiFetch<TaskSecurityGroup[]>(`/api/security/events/grouped?${params}`);
    },
    summary: (since = "24h") =>
      apiFetch<SecuritySummary>(`/api/security/summary?since=${encodeURIComponent(since)}`),
    status: () => apiFetch<SecurityStatus>("/api/security/status"),
    patterns: () => apiFetch<DlpPatternInfo[]>("/api/security/patterns"),
    policies: () => apiFetch<DlpPolicy[]>("/api/security/policies"),
    createPolicy: (policy: Omit<DlpPolicy, "id" | "updated_at">) =>
      apiFetch<DlpPolicy>("/api/security/policies", {
        method: "POST",
        body: JSON.stringify(policy),
      }),
    deletePolicy: (id: number) =>
      apiFetch<void>(`/api/security/policies/${id}`, { method: "DELETE" }),
  },
};
