import type { McclawdConfig, McpServer, Task, WorkspaceFile } from "./types";

let authToken: string | null = null;

export function setToken(token: string) {
  authToken = token;
}

export function clearToken() {
  authToken = null;
}

export function getToken() {
  return authToken;
}

async function apiFetch<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...((options.headers as Record<string, string>) || {}),
  };

  if (authToken) {
    headers["Authorization"] = `Bearer ${authToken}`;
  }

  const res = await fetch(path, { ...options, headers });

  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${res.statusText}`);
  }

  if (res.status === 204) return undefined as T;
  return res.json();
}

export const api = {
  auth: {
    login: (password: string) =>
      apiFetch<{ token: string }>("/api/auth/login", {
        method: "POST",
        body: JSON.stringify({ password }),
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
    add: (name: string, value: string) =>
      apiFetch<void>("/api/secrets", {
        method: "POST",
        body: JSON.stringify({ name, value }),
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
};
