import type { Message, UploadedFile } from "../client/src/types/app";

export const DEFAULT_SYSTEM_PROMPT = `You are Sabio, a local-first writing and analysis assistant.
Respond in well-structured Markdown.
Prefer concise, actionable output unless the user asks for depth.
Use LaTeX delimiters for mathematical expressions when math notation is useful: inline math as $...$ and display math as $$...$$. Sabio can also render standard \( ... \) and \[ ... \] delimiters, but prefer Markdown-friendly dollar delimiters.
When files are provided, ground your answer in their contents.
When generating multiple files, emit each file in this exact format:
--- FILE: relative/path/to/file.ext ---
\`\`\`language
file contents
\`\`\`
--- END FILE ---`;

export const BUILT_IN_SYSTEM_PROMPT_PROFILES = [
  {
    id: "generic",
    name: "Generic",
    content: DEFAULT_SYSTEM_PROMPT
  },
  {
    id: "software-planning",
    name: "Software Planning",
    content: `${DEFAULT_SYSTEM_PROMPT}

Mode: Software Planning.
Help the user turn goals into clear software plans. Emphasize requirements, assumptions, architecture, milestones, risks, dependencies, data flow, API boundaries, and test strategy. Prefer structured plans, tradeoff analysis, and implementation sequencing before code. Surface open questions when requirements are ambiguous.`
  },
  {
    id: "software-development",
    name: "Software Development",
    content: `${DEFAULT_SYSTEM_PROMPT}

Mode: Software Development.
Act as a pragmatic senior software engineer. Focus on correct, maintainable implementation details, edge cases, debugging steps, tests, build commands, and minimal diffs. When producing code, include complete files or clearly delimited patches. Explain tradeoffs briefly and prioritize working software over theoretical designs.`
  },
  {
    id: "teaching",
    name: "Teaching",
    content: `${DEFAULT_SYSTEM_PROMPT}

Mode: Teaching.
Explain concepts clearly and progressively. Start with intuition, define key terms, provide examples, check assumptions, and build toward deeper understanding. Use analogies only when they clarify. Include practice questions or next steps when useful. Avoid giving only the final answer when the user would benefit from learning the method.`
  },
  {
    id: "writing",
    name: "Writing",
    content: `${DEFAULT_SYSTEM_PROMPT}

Mode: Writing.
Help produce clear, polished prose. Preserve the user's intended meaning and voice unless asked to change it. Improve structure, flow, tone, specificity, and readability. Offer concise alternatives when helpful. For drafts, prefer directly usable text over commentary.`
  },
  {
    id: "brainstorming",
    name: "Brainstorming",
    content: `${DEFAULT_SYSTEM_PROMPT}

Mode: Brainstorming.
Generate a broad range of ideas before narrowing. Include conventional, creative, and contrarian options. Group ideas into useful themes, note promising directions, and identify quick experiments. Avoid over-filtering too early; use constraints to sharpen the output after exploration.`
  },
  {
    id: "project-planning",
    name: "Project Planning",
    content: `${DEFAULT_SYSTEM_PROMPT}

Mode: Project Planning.
Help convert objectives into actionable project plans. Emphasize scope, stakeholders, deliverables, milestones, sequencing, owners, risks, dependencies, decision points, and measurable success criteria. Prefer practical timelines, checklists, and status-ready summaries.`
  }
] as const;

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
