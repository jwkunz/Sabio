import fs from "node:fs";
import path from "node:path";
import express from "express";
import multer from "multer";
import { createServer as createViteServer } from "vite";
import { extractRawText } from "./parsers/extractText";
import { fetchModels, createChatStream } from "./services/ollama";
import { AppError, toErrorMessage } from "./utils/errors";

const app = express();
const upload = multer({ storage: multer.memoryStorage() });
const rootDir = process.cwd();
const distDir = path.join(rootDir, "dist/client");
const clientDir = path.join(rootDir, "client");
const hasBuiltClient = fs.existsSync(path.join(distDir, "index.html"));
const isProduction = process.env.NODE_ENV === "production" || hasBuiltClient;

app.use(express.json({ limit: "2mb" }));

app.get("/api/health", (_req, res) => {
  res.json({ ok: true });
});

app.get("/api/models", async (_req, res, next) => {
  try {
    const models = await fetchModels();
    res.json({ models });
  } catch (error) {
    next(error);
  }
});

app.post("/api/upload", upload.array("files"), async (req, res, next) => {
  try {
    const files = (req.files as Express.Multer.File[] | undefined) ?? [];

    const parsed = await Promise.all(
      files.map(async (file) => {
        const rawText = await extractRawText(file);

        if (!rawText) {
          throw new AppError(400, `Parsed file is empty: ${file.originalname}`);
        }

        return {
          id: crypto.randomUUID(),
          name: file.originalname,
          type: file.mimetype || "text/plain",
          size: file.size,
          uploadedAt: Date.now(),
          rawText,
          warning: file.size > 1024 * 1024 ? "Large file: prompt size may increase." : ""
        };
      })
    );

    res.json({ files: parsed });
  } catch (error) {
    next(error);
  }
});

app.post("/api/chat", async (req, res, next) => {
  const controller = new AbortController();

  req.on("aborted", () => {
    controller.abort();
  });

  res.on("close", () => {
    if (!res.writableEnded) {
      controller.abort();
    }
  });

  try {
    const { model, prompt } = req.body as { model?: string; prompt?: string };

    if (!model) {
      throw new AppError(400, "A model must be selected.");
    }

    if (!prompt?.trim()) {
      throw new AppError(400, "Prompt cannot be empty.");
    }

    const stream = await createChatStream({
      model,
      prompt,
      signal: controller.signal
    });

    res.setHeader("Content-Type", "text/event-stream");
    res.setHeader("Cache-Control", "no-cache");
    res.setHeader("Connection", "keep-alive");

    const reader = stream.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    const pushChunk = (payload: unknown) => {
      res.write(`data: ${JSON.stringify(payload)}\n\n`);
    };

    while (true) {
      const { done, value } = await reader.read();

      if (done) {
        break;
      }

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() ?? "";

      for (const line of lines) {
        if (!line.trim()) {
          continue;
        }

        const parsed = JSON.parse(line) as {
          response?: string;
          done?: boolean;
          error?: string;
        };

        if (parsed.error) {
          throw new AppError(502, parsed.error);
        }

        if (parsed.response) {
          pushChunk({ type: "chunk", content: parsed.response });
        }

        if (parsed.done) {
          pushChunk({ type: "done" });
        }
      }
    }

    res.end();
  } catch (error) {
    if (controller.signal.aborted) {
      res.end();
      return;
    }

    next(error);
  }
});

app.use((error: unknown, _req: express.Request, res: express.Response, _next: express.NextFunction) => {
  const status = error instanceof AppError ? error.status : 500;
  console.error("[sabio]", {
    status,
    error: error instanceof AppError ? error.message : "Unexpected server error.",
    detail: error instanceof Error ? error.message : toErrorMessage(error)
  });
  res.status(status).json({
    error: error instanceof AppError ? error.message : "Unexpected server error.",
    detail: error instanceof Error ? error.message : toErrorMessage(error)
  });
});

const start = async () => {
  if (!isProduction) {
    const vite = await createViteServer({
      root: clientDir,
      server: {
        middlewareMode: true
      },
      appType: "spa"
    });

    app.use(vite.middlewares);
  } else {
    app.use(express.static(distDir));
    app.get("*", (_req, res) => {
      res.sendFile(path.join(distDir, "index.html"));
    });
  }

  app.listen(3000, "127.0.0.1", () => {
    const source = isProduction ? "production bundle" : "Vite middleware";
    console.log(`Sabio listening on http://127.0.0.1:3000 using ${source}`);
  });
};

start().catch((error) => {
  console.error(error);
  process.exit(1);
});
