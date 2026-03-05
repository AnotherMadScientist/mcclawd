export interface Task {
  id: string;
  prompt: string;
  status: "Running" | "Completed" | { Failed: string };
}

export interface WorkspaceFile {
  name: string;
  content?: string;
}

export interface McpServer {
  name: string;
  image: string;
  port: number;
}

export interface McclawdConfig {
  data_dir: string;
  agent: {
    max_turns: number;
    model: string;
    default_workspace: string;
  };
  providers: {
    anthropic?: { api_key_secret: string };
    openai?: { api_key_secret: string };
    ollama?: { url: string };
  };
  mcp: {
    agentgateway_url: string;
    servers: McpServer[];
  };
}

export type StreamChunk =
  | { UserMessage: string }
  | { TextDelta: string }
  | { TextBlock: string }
  | { ToolStart: { name: string } }
  | { ToolEnd: { name: string; summary: string | null } }
  | { StatusIndicator: "Typing" | "Processing" | "UploadingMedia" | "Done" }
  | "Done"
  | { Error: string }
  | {
      Attachments: Array<{
        name: string;
        size: number;
        content_type: string;
        url: string;
      }>;
    };

export interface AttachmentMeta {
  name: string;
  size: number;
  content_type: string;
  url: string;
}

export interface InstalledSkill {
  name: string;
  version: string;
  source: { Local: string } | { Registry: { registry_url: string } };
  installed_at: string;
}

export interface ClawHubSkillMeta {
  name: string;
  version: string;
  author: string;
  description: string;
  downloads: number;
  tags: string[];
  updated_at: string;
}

export interface ClawHubSearchResult {
  skills: ClawHubSkillMeta[];
  total: number;
  page: number;
}

export interface CachedSearchResult {
  skills: ClawHubSkillMeta[];
  total: number;
  page: number;
  cached: boolean;
  last_refreshed: string | null;
}

export interface CacheStats {
  skill_count: number;
  last_refreshed: string | null;
  cache_path: string;
}

export type ScanStatus = "Pass" | "Warning" | "Critical" | "NotScanned";

export interface ScanIssue {
  code: string;
  severity: string;
  description: string;
}

export interface ScanResult {
  status: ScanStatus;
  issues: ScanIssue[];
}
