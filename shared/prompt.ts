import type { Message, UploadedFile } from "../client/src/types/app";

export const DEFAULT_SYSTEM_PROMPT = `You are Sabio, a local-first writing and analysis assistant.
Respond in well-structured Markdown.
Prefer concise, actionable output unless the user asks for depth.
When files are provided, ground your answer in their contents.
When generating multiple files, emit each file in this exact format:
--- FILE: relative/path/to/file.ext ---
\`\`\`language
file contents
\`\`\`
--- END FILE ---`;

export const SOFT_CONTEXT_LIMIT = 28000;
export const HARD_CONTEXT_LIMIT = 36000;

const formatHistory = (messages: Message[]) =>
  messages
    .map((message) => {
      const label = message.role === "user" ? "User" : "Assistant";
      return `${label}:\n${message.content.trim()}`;
    })
    .join("\n\n");

const formatFiles = (files: UploadedFile[]) =>
  files
    .slice()
    .sort((a, b) => a.uploadedAt - b.uploadedAt)
    .map((file) => `--- FILE: ${file.name} ---\n${file.rawText}\n--- END FILE ---`)
    .join("\n\n");

export const trimHistoryForContext = ({
  systemPrompt,
  messages,
  selectedFiles,
  currentInput
}: {
  systemPrompt: string;
  messages: Message[];
  selectedFiles: UploadedFile[];
  currentInput: string;
}) => {
  const prioritizedFiles = formatFiles(selectedFiles);
  const trimmed = [...messages];

  while (trimmed.length > 0) {
    const draft = buildPrompt({
      systemPrompt,
      messages: trimmed,
      selectedFiles,
      currentInput
    });

    if (draft.length <= HARD_CONTEXT_LIMIT) {
      break;
    }

    trimmed.shift();
  }

  return {
    messages: trimmed,
    warning:
      prioritizedFiles.length + formatHistory(trimmed).length > SOFT_CONTEXT_LIMIT
        ? "Context is getting large. Older conversation may be trimmed."
        : ""
  };
};

export const buildPrompt = ({
  systemPrompt,
  messages,
  selectedFiles,
  currentInput
}: {
  systemPrompt: string;
  messages: Message[];
  selectedFiles: UploadedFile[];
  currentInput: string;
}) => {
  const sections = [
    "# System Instructions",
    systemPrompt.trim(),
    "# Conversation History",
    formatHistory(messages) || "(no prior messages)",
    "# Selected Files",
    formatFiles(selectedFiles) || "(no files selected)",
    "# Current User Input",
    currentInput.trim()
  ];

  return sections.join("\n\n");
};
