import type { McclawdConfig, McpServer, Task, WorkspaceFile } from "./types";

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
