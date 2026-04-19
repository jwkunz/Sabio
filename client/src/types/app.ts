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

export interface SessionState {
  selectedModel: string;
  systemPrompt: string;
  selectedSystemPromptProfileId: string;
  draftInput: string;
  paneWidths: PaneWidths;
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
