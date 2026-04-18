import { AppError } from "../utils/errors";

const OLLAMA_BASE_URL = process.env.OLLAMA_BASE_URL ?? "http://127.0.0.1:11434";

export const fetchModels = async () => {
  const response = await fetch(`${OLLAMA_BASE_URL}/api/tags`);

  if (!response.ok) {
    throw new AppError(502, "Ollama is unavailable.");
  }

  const data = (await response.json()) as {
    models?: Array<{ name: string; size?: number; modified_at?: string }>;
  };

  return (data.models ?? []).map((model) => ({
    name: model.name,
    size: model.size,
    modifiedAt: model.modified_at
  }));
};

export const createChatStream = async ({
  model,
  prompt,
  signal
}: {
  model: string;
  prompt: string;
  signal: AbortSignal;
}) => {
  const response = await fetch(`${OLLAMA_BASE_URL}/api/generate`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify({
      model,
      prompt,
      stream: true
    }),
    signal
  });

  if (!response.ok || !response.body) {
    throw new AppError(502, "Unable to stream from Ollama.");
  }

  return response.body;
};
