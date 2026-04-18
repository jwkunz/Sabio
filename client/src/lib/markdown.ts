const FENCED_CODE_BLOCK_PATTERN = /(```[\s\S]*?```)/g;

const normalizeSegmentMathDelimiters = (segment: string) =>
  segment
    .replace(/\\\[([\s\S]*?)\\\]/g, (_match, content: string) => `$$\n${content.trim()}\n$$`)
    .replace(/\\\(([\s\S]*?)\\\)/g, (_match, content: string) => `$${content.trim()}$`);

export const normalizeMathDelimiters = (markdown: string) =>
  markdown
    .split(FENCED_CODE_BLOCK_PATTERN)
    .map((segment) => (segment.startsWith("```") ? segment : normalizeSegmentMathDelimiters(segment)))
    .join("");
