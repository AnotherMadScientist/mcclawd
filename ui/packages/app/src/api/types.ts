export interface Task {
  id: string;
  prompt: string;
  status: "Running" | "Completed" | { Failed: string };
  tags?: string[];
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

// --- Provider & Budget types ---

export interface Provider {
  name: string;
  kind: string;
  models: string[];
  enabled: boolean;
  priority: number;
}

export interface ModelUsageEntry {
  model: string;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  estimated_cost_usd: number;
  request_count: number;
}

export interface TaskUsageEntry {
  task_id: string;
  prompt_preview: string;
  model: string;
  total_tokens: number;
  estimated_cost_usd: number;
}

export interface DailyUsage {
  date: string; // "YYYY-MM-DD"
  cost_usd: number;
  tokens: number;
}

export interface DetailedUsageSummary {
  by_model: ModelUsageEntry[];
  by_task: TaskUsageEntry[];
  total: ModelUsageEntry;
  period: string;
  daily_history: DailyUsage[];
}

export interface AccountCredits {
  source: "admin_api" | "local_tracking";
  monthly_cost_usd: number;
  data_available: boolean;
}

export interface BudgetInfo {
  daily_limit_usd: number | null;
  monthly_limit_usd: number | null;
  daily_spent_usd: number;
  monthly_spent_usd: number;
  alerts: string[];
  account_credits?: AccountCredits;
}

export interface CreditsResponse {
  available: boolean;
  monthly_cost_usd: number;
  source: "admin_api" | "local_tracking";
  error?: string;
}

export interface BudgetUpdate {
  daily_limit_usd?: number | null;
  monthly_limit_usd?: number | null;
  per_task_limit_usd?: number | null;
}

export interface AnthropicModel {
  id: string;
  display_name: string;
  created_at?: string;
}

export interface ModelPricing {
  model_id: string;
  input_price_per_mtok: number;
  output_price_per_mtok: number;
}

// --- Docker types ---

export interface DockerBuildStatus {
  status: "checking" | "image_ready" | "building" | "complete" | "failed";
  progress_pct: number;
  logs: string[];
  error: string | null;
  image_available: boolean;
  image_id: string | null;
  image_size: number | null;
  build_duration_secs?: number | null;
  agent_startup_secs?: number | null;
}

export interface ContainerMount {
  source: string;
  destination: string;
  mode: string;
}

export interface ContainerAttachmentMeta {
  name: string;
  size: number;
  is_image: boolean;
}

export interface ContainerInfo {
  id: string;
  name: string;
  task_id: string | null;
  status: string;
  state: string;
  image: string;
  created: number;
  ports: string[];
  mounts: ContainerMount[];
  env_vars: Record<string, string>;
  labels: Record<string, string>;
  attachments?: ContainerAttachmentMeta[];
  skills?: string[];
  mcp_tools?: string[];
  gateway_url?: string | null;
}

export interface ContainerDetailMount {
  source: string;
  destination: string;
  mode: string;
  rw: boolean;
}

export interface ContainerDetail {
  id: string;
  name: string;
  image: string;
  status: string;
  running: boolean;
  started_at: string | null;
  finished_at: string | null;
  exit_code: number | null;
  env: Record<string, string>;
  mounts: ContainerDetailMount[];
  network: string[];
  labels: Record<string, string>;
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
