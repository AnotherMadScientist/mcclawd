import type {
  PublicKeyCredentialCreationOptionsJSON,
  PublicKeyCredentialRequestOptionsJSON,
  RegistrationResponseJSON,
  AuthenticationResponseJSON,
} from "@simplewebauthn/browser";
import type { AttachmentMeta, McclawdConfig, McpServer, Task, WorkspaceFile, InstalledSkill, CachedSearchResult, ClawHubSkillMeta, ScanResult } from "./types";

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
    create: (prompt: string, workspace?: string, model?: string) =>
      apiFetch<Task>("/api/tasks", {
        method: "POST",
        body: JSON.stringify({ prompt, workspace, model }),
      }),
    get: (id: string) => apiFetch<Task>(`/api/tasks/${id}`),
    cancel: (id: string) => apiFetch<void>(`/api/tasks/${id}`, { method: "DELETE" }),
    sendMessage: (id: string, message: string) =>
      apiFetch<void>(`/api/tasks/${id}/message`, {
        method: "POST",
        body: JSON.stringify({ message }),
      }),
    uploadAttachments: async (taskId: string, files: File[]) => {
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
      if (!res.ok) throw new Error(`Upload failed: ${res.status}`);
      return res.json() as Promise<AttachmentMeta[]>;
    },
    listAttachments: (taskId: string) =>
      apiFetch<AttachmentMeta[]>(`/api/tasks/${taskId}/attachments`),
  },
  workspace: {
    list: () => apiFetch<WorkspaceFile[]>("/api/workspace"),
    get: (file: string) => apiFetch<WorkspaceFile>(`/api/workspace/${file}`),
    update: (file: string, content: string) =>
      apiFetch<void>(`/api/workspace/${file}`, {
        method: "PUT",
        body: JSON.stringify({ content }),
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
    update: (config: Partial<McclawdConfig>) =>
      apiFetch<void>("/api/config", {
        method: "PUT",
        body: JSON.stringify(config),
      }),
  },
  mcp: {
    servers: () => apiFetch<McpServer[]>("/api/mcp/servers"),
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
    install: (name: string, version?: string) =>
      apiFetch<InstalledSkill>("/api/skills/install", {
        method: "POST",
        body: JSON.stringify({ name, version }),
      }),
    uninstall: (name: string) =>
      apiFetch<void>(`/api/skills/${encodeURIComponent(name)}`, { method: "DELETE" }),
    scan: (name: string) =>
      apiFetch<ScanResult>(`/api/skills/${encodeURIComponent(name)}/scan`),
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
};
