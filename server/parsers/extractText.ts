import { Buffer } from "node:buffer";
import mammoth from "mammoth";
import pdf from "pdf-parse";
import { AppError } from "../utils/errors";

const textDecoder = new TextDecoder("utf-8", { fatal: false });

const codeExtensions = new Set([
  ".js",
  ".jsx",
  ".ts",
  ".tsx",
  ".py",
  ".rb",
  ".go",
  ".java",
  ".c",
  ".cpp",
  ".cs",
  ".rs",
  ".html",
  ".css",
  ".scss",
  ".sql",
  ".sh"
]);

const extensionOf = (name: string) => {
  const dotIndex = name.lastIndexOf(".");
  return dotIndex >= 0 ? name.slice(dotIndex).toLowerCase() : "";
};

const parseCsv = (content: string) =>
  content
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => line.split(",").map((cell) => cell.trim()).join(" | "))
    .join("\n");

export const extractRawText = async (file: Express.Multer.File) => {
  const extension = extensionOf(file.originalname);

  try {
    if (extension === ".pdf") {
      const parsed = await pdf(file.buffer);
      return parsed.text.trim();
    }

    if (extension === ".docx") {
      const parsed = await mammoth.extractRawText({ buffer: file.buffer });
      return parsed.value.trim();
    }

    if (extension === ".csv") {
      return parseCsv(textDecoder.decode(file.buffer));
    }

    if (extension === ".json") {
      const parsed = JSON.parse(textDecoder.decode(file.buffer));
      return JSON.stringify(parsed, null, 2);
    }

    if (extension === ".txt" || extension === ".md" || codeExtensions.has(extension)) {
      return textDecoder.decode(file.buffer).trim();
    }

    return textDecoder.decode(Buffer.from(file.buffer)).trim();
  } catch (error) {
    try {
      return textDecoder.decode(Buffer.from(file.buffer)).trim();
    } catch {
      throw new AppError(400, `Unable to parse file: ${file.originalname}`);
    }
  }
};
