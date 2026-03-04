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
  | { TextDelta: string }
  | { TextBlock: string }
  | { ToolStart: { name: string } }
  | { ToolEnd: { name: string; summary: string | null } }
  | "Done"
  | { Error: string };
