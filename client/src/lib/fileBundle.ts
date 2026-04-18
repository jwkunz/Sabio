import JSZip from "jszip";

export interface ExtractedFile {
  path: string;
  language: string;
  content: string;
}

const FILE_BLOCK_PATTERN =
  /--- FILE:\s*([^\n]+?)\s*---\s*```([^\n`]*)\n([\s\S]*?)```[\t ]*\n--- END FILE ---/g;

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
  codeContent,
  language,
  blockIndex
}: {
  messageContent: string;
  codeContent: string;
  language: string;
  blockIndex: number;
}) => {
  const extractedFiles = extractFilesFromMarkdown(messageContent);
  const matched = extractedFiles.find((file) => file.content.trim() === codeContent.trim());

  if (matched) {
    return matched.path.split("/").pop() || matched.path;
  }

  const normalizedLanguage = language.toLowerCase().trim();
  const extension = languageExtensionMap[normalizedLanguage] ?? "txt";
  return `sabio-code-${blockIndex + 1}.${extension}`;
};
