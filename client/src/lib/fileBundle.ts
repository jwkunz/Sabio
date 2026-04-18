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
