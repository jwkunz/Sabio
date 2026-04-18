import JSZip from "jszip";

export interface ExtractedFile {
  path: string;
  language: string;
  content: string;
}

export interface ExtractedCodeBlock {
  filename: string;
  language: string;
  content: string;
}

const FILE_BLOCK_PATTERN =
  /--- FILE:\s*([^\n]+?)\s*---\s*```([^\n`]*)\n([\s\S]*?)```[\t ]*\n--- END FILE ---/g;
const CODE_BLOCK_PATTERN = /```([^\n`]*)\n([\s\S]*?)```/g;
const FILE_NAME_HINT_PATTERN =
  /(?:^|\n)\s*(?:#{1,6}\s*)?(?:[-*]\s*)?(?:\*\*)?(?:file(?:name)?\s*:\s*)?`?([A-Za-z0-9._/@-]+\/[A-Za-z0-9._@-]+|[A-Za-z0-9._@-]+\.[A-Za-z0-9._@-]+)`?(?:\*\*)?\s*$/i;

const sanitizePath = (rawPath: string) => {
  const normalized = rawPath
    .trim()
    .replace(/\\/g, "/")
    .replace(/^\/+/, "")
    .replace(/\/+/g, "/");

  const safeParts = normalized
    .split("/")
    .filter(Boolean)
    .filter((part) => part !== "." && part !== "..");

  return safeParts.join("/");
};

const baseName = (path: string) => path.split("/").pop() || path;

const parseFenceInfo = (info: string) => {
  const parts = info.trim().split(/\s+/).filter(Boolean);
  const language = parts[0] ?? "";
  const filenameMatch = info.match(/(?:file(?:name)?=|path=)(?:"([^"]+)"|'([^']+)'|([^\s]+))/i);
  const positionalPath = parts.find((part, index) => index > 0 && sanitizePath(part).includes("."));
  const filename = sanitizePath(filenameMatch?.[1] ?? filenameMatch?.[2] ?? filenameMatch?.[3] ?? positionalPath ?? "");

  return {
    language,
    filename
  };
};

const findFilenameHintBefore = (markdown: string, codeBlockStart: number) => {
  const before = markdown.slice(0, codeBlockStart).split("\n").slice(-4).join("\n");
  const match = before.match(FILE_NAME_HINT_PATTERN);
  return sanitizePath(match?.[1] ?? "");
};

export const extractFilesFromMarkdown = (markdown: string): ExtractedFile[] => {
  const matches = markdown.matchAll(FILE_BLOCK_PATTERN);
  const seenPaths = new Set<string>();
  const extracted: ExtractedFile[] = [];

  for (const match of matches) {
    const candidatePath = sanitizePath(match[1] ?? "");
    const language = (match[2] ?? "").trim();
    const content = (match[3] ?? "").replace(/\n$/, "");

    if (!candidatePath || !content) {
      continue;
    }

    let nextPath = candidatePath;
    let suffix = 1;

    while (seenPaths.has(nextPath)) {
      const dotIndex = candidatePath.lastIndexOf(".");
      nextPath =
        dotIndex > 0
          ? `${candidatePath.slice(0, dotIndex)}-${suffix}${candidatePath.slice(dotIndex)}`
          : `${candidatePath}-${suffix}`;
      suffix += 1;
    }

    seenPaths.add(nextPath);
    extracted.push({
      path: nextPath,
      language,
      content
    });
  }

  return extracted;
};

export const extractCodeBlocksFromMarkdown = (markdown: string): ExtractedCodeBlock[] => {
  const codeBlocks: ExtractedCodeBlock[] = [];
  const fileBlocks = extractFilesFromMarkdown(markdown);
  let fileBlockIndex = 0;

  for (const match of markdown.matchAll(CODE_BLOCK_PATTERN)) {
    const info = match[1] ?? "";
    const content = (match[2] ?? "").replace(/\n$/, "");
    const codeBlockStart = match.index ?? 0;
    const fence = parseFenceInfo(info);
    const hintedFilename = findFilenameHintBefore(markdown, codeBlockStart);
    const matchingFileBlock =
      fileBlocks[fileBlockIndex]?.content.trim() === content.trim() ? fileBlocks[fileBlockIndex++] : undefined;
    const filename = fence.filename || hintedFilename || matchingFileBlock?.path || "";

    codeBlocks.push({
      filename,
      language: fence.language || matchingFileBlock?.language || "",
      content
    });
  }

  return codeBlocks;
};

export const downloadFileBundle = async (files: ExtractedFile[], archiveName: string) => {
  const zip = new JSZip();

  for (const file of files) {
    zip.file(file.path, file.content);
  }

  const blob = await zip.generateAsync({ type: "blob" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = archiveName;
  anchor.click();
  URL.revokeObjectURL(url);
};

const languageExtensionMap: Record<string, string> = {
  js: "js",
  jsx: "jsx",
  ts: "ts",
  tsx: "tsx",
  py: "py",
  rb: "rb",
  go: "go",
  java: "java",
  c: "c",
  cpp: "cpp",
  cs: "cs",
  rs: "rs",
  html: "html",
  css: "css",
  scss: "scss",
  json: "json",
  md: "md",
  markdown: "md",
  sh: "sh",
  bash: "sh",
  sql: "sql",
  yml: "yml",
  yaml: "yaml",
  xml: "xml",
  txt: "txt"
};

export const downloadTextFile = ({
  content,
  filename
}: {
  content: string;
  filename: string;
}) => {
  const blob = new Blob([content], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
};

export const inferCodeBlockFilename = ({
  messageContent,
  language,
  blockIndex
}: {
  messageContent: string;
  language: string;
  blockIndex: number;
}) => {
  const codeBlock = extractCodeBlocksFromMarkdown(messageContent)[blockIndex];
  const providedFilename = sanitizePath(codeBlock?.filename ?? "");

  if (providedFilename) {
    return baseName(providedFilename);
  }

  const normalizedLanguage = (language || codeBlock?.language || "").toLowerCase().trim();
  const extension = languageExtensionMap[normalizedLanguage] ?? "txt";
  return `sabio-code-${blockIndex + 1}.${extension}`;
};
