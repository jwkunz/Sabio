export type Role = "user" | "assistant";

export interface Message {
  id: string;
  role: Role;
  content: string;
  createdAt: number;
}

export interface UploadedFile {
  id: string;
  name: string;
  type: string;
  size: number;
  uploadedAt: number;
  rawText: string;
  isSelected: boolean;
  warning?: string;
}

export interface PaneWidths {
  left: number;
  center: number;
  right: number;
}

export type DisplayTheme = "dark" | "light";
export type DisplayFontSize = "small" | "medium" | "large";
export type AppMode = "chat" | "agent";

export interface DisplayPreferences {
  theme: DisplayTheme;
  fontSize: DisplayFontSize;
}

export interface AgentWorkspaceState {
  inputPath: string;
  canonicalPath: string;
  isGitRepo: boolean;
  gitBranch: string;
  cleanWorktree: boolean | null;
  trusted: boolean;
  message: string;
}

export interface SessionState {
  appMode: AppMode;
  selectedModel: string;
  systemPrompt: string;
  selectedSystemPromptProfileId: string;
  draftInput: string;
  paneWidths: PaneWidths;
  displayPreferences: DisplayPreferences;
  agentWorkspace: AgentWorkspaceState;
}

export interface SystemPromptProfile {
  id: string;
  name: string;
  content: string;
  isBuiltIn: boolean;
  updatedAt: number;
}

export interface ModelOption {
  name: string;
  size?: number;
  modifiedAt?: string;
}

export interface ChatRequestBody {
  model: string;
  prompt: string;
  requestId: string;
}

export interface AgentSessionSummary {
  id: string;
  title: string;
  workspacePath: string;
  gitBranch?: string;
  createdAt: number;
  updatedAt: number;
}

export type AgentEventType =
  | "session_started"
  | "assistant_message_delta"
  | "plan_created"
  | "approval_requested"
  | "approval_resolved"
  | "tool_started"
  | "tool_output"
  | "tool_finished"
  | "patch_created"
  | "git_commit_created"
  | "error"
  | "cancelled"
  | "session_finished";

export interface AgentEvent {
  id: string;
  sessionId: string;
  timestamp: number;
  type: AgentEventType;
  payload: Record<string, unknown>;
  parentEventId?: string | null;
}
