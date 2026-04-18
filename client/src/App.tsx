import { useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { buildPrompt, DEFAULT_SYSTEM_PROMPT, trimHistoryForContext } from "../../shared/prompt";
import logoUrl from "../../assets/Sabio_logo.png";
import { clearMessages, loadFiles, loadMessages, loadSession, saveFiles, saveMessages, saveSession } from "./lib/db";
import type { Message, ModelOption, PaneWidths, SessionState, UploadedFile } from "./types/app";

const createMessage = (role: Message["role"], content: string): Message => ({
  id: crypto.randomUUID(),
  role,
  content,
  createdAt: Date.now()
});

const defaultPaneWidths: PaneWidths = {
  left: 22,
  center: 52,
  right: 26
};

const copyText = async (value: string) => navigator.clipboard.writeText(value);

function App() {
  const [isHydrated, setIsHydrated] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [files, setFiles] = useState<UploadedFile[]>([]);
  const [session, setSession] = useState<SessionState>({
    selectedModel: "",
    systemPrompt: DEFAULT_SYSTEM_PROMPT,
    draftInput: "",
    paneWidths: defaultPaneWidths
  });
  const [models, setModels] = useState<ModelOption[]>([]);
  const [streamingContent, setStreamingContent] = useState("");
  const [status, setStatus] = useState("Loading session...");
  const [error, setError] = useState("");
  const [contextWarning, setContextWarning] = useState("");
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [lastRequest, setLastRequest] = useState<{ input: string; baseMessages: Message[] } | null>(null);
  const [isGenerating, setIsGenerating] = useState(false);
  const abortRef = useRef<AbortController | null>(null);
  const chatScrollRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{ startX: number; widths: PaneWidths; handle: "left" | "right" } | null>(null);

  const selectedFiles = useMemo(
    () => files.filter((file) => file.isSelected).sort((a, b) => a.uploadedAt - b.uploadedAt),
    [files]
  );

  useEffect(() => {
    const hydrate = async () => {
      const [storedSession, storedMessages, storedFiles] = await Promise.all([
        loadSession(),
        loadMessages(),
        loadFiles()
      ]);

      setSession({
        ...storedSession,
        systemPrompt: storedSession.systemPrompt || DEFAULT_SYSTEM_PROMPT,
        paneWidths: storedSession.paneWidths || defaultPaneWidths
      });
      setMessages(storedMessages);
      setFiles(storedFiles);
      setStatus("");
      setIsHydrated(true);
    };

    hydrate().catch(() => {
      setError("Unable to load local session data.");
      setStatus("");
      setIsHydrated(true);
    });
  }, []);

  useEffect(() => {
    if (!isHydrated) {
      return;
    }

    saveSession(session).catch(() => {
      setError("Unable to persist session settings.");
    });
  }, [isHydrated, session]);

  useEffect(() => {
    if (!isHydrated) {
      return;
    }

    saveMessages(messages).catch(() => {
      setError("Unable to persist conversation history.");
    });
  }, [isHydrated, messages]);

  useEffect(() => {
    if (!isHydrated) {
      return;
    }

    saveFiles(files).catch(() => {
      setError("Unable to persist files.");
    });
  }, [isHydrated, files]);

  useEffect(() => {
    if (!isHydrated) {
      return;
    }

    fetch("/api/models")
      .then(async (response) => {
        if (!response.ok) {
          throw new Error("Model discovery failed.");
        }

        return response.json() as Promise<{ models: ModelOption[] }>;
      })
      .then(({ models: modelOptions }) => {
        setModels(modelOptions);
        setSession((current) => {
          if (current.selectedModel && modelOptions.some((model) => model.name === current.selectedModel)) {
            return current;
          }

          return {
            ...current,
            selectedModel: modelOptions[0]?.name ?? ""
          };
        });
      })
      .catch(() => {
        setError("Ollama is unavailable. Start Ollama locally to list models.");
      });
  }, [isHydrated]);

  useEffect(() => {
    chatScrollRef.current?.scrollTo({
      top: chatScrollRef.current.scrollHeight,
      behavior: "smooth"
    });
  }, [messages, streamingContent, error]);

  useEffect(() => {
    const onMove = (event: MouseEvent) => {
      if (!dragRef.current) {
        return;
      }

      const deltaPercent = (event.clientX - dragRef.current.startX) / window.innerWidth * 100;
      const next = { ...dragRef.current.widths };

      if (dragRef.current.handle === "left") {
        next.left = Math.min(35, Math.max(15, dragRef.current.widths.left + deltaPercent));
        next.center = Math.min(70, Math.max(30, dragRef.current.widths.center - deltaPercent));
      } else {
        next.center = Math.min(70, Math.max(30, dragRef.current.widths.center + deltaPercent));
        next.right = Math.min(35, Math.max(18, dragRef.current.widths.right - deltaPercent));
      }

      setSession((current) => ({
        ...current,
        paneWidths: next
      }));
    };

    const onUp = () => {
      dragRef.current = null;
      document.body.classList.remove("is-resizing");
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);

    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  const updateDraftInput = (value: string) =>
    setSession((current) => ({
      ...current,
      draftInput: value
    }));

  const submitPrompt = async ({
    input,
    baseMessages,
    nextMessages
  }: {
    input: string;
    baseMessages: Message[];
    nextMessages: Message[];
  }) => {
    if (!input.trim() || isGenerating) {
      return;
    }

    if (!session.selectedModel) {
      setError("Select an Ollama model before sending a prompt.");
      return;
    }

    const { messages: trimmedMessages, warning } = trimHistoryForContext({
      systemPrompt: session.systemPrompt || DEFAULT_SYSTEM_PROMPT,
      messages: baseMessages,
      selectedFiles,
      currentInput: input.trim()
    });
    const prompt = buildPrompt({
      systemPrompt: session.systemPrompt || DEFAULT_SYSTEM_PROMPT,
      messages: trimmedMessages,
      selectedFiles,
      currentInput: input.trim()
    });

    setContextWarning(warning);
    setError("");
    setStatus("");
    setLastRequest({ input: input.trim(), baseMessages });
    setIsGenerating(true);
    setStreamingContent("");
    setMessages(nextMessages);

    setSession((current) => ({
      ...current,
      draftInput: ""
    }));

    const controller = new AbortController();
    abortRef.current = controller;

    try {
      const response = await fetch("/api/chat", {
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        },
        body: JSON.stringify({
          model: session.selectedModel,
          prompt,
          requestId: crypto.randomUUID()
        }),
        signal: controller.signal
      });

      if (!response.ok || !response.body) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error || "Chat request failed.");
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      let finalContent = "";

      while (true) {
        const { done, value } = await reader.read();

        if (done) {
          break;
        }

        buffer += decoder.decode(value, { stream: true });
        const parts = buffer.split("\n\n");
        buffer = parts.pop() ?? "";

        for (const part of parts) {
          const line = part.trim();

          if (!line.startsWith("data:")) {
            continue;
          }

          const payload = JSON.parse(line.slice(5).trim()) as { type: string; content?: string };

          if (payload.type === "chunk" && payload.content) {
            finalContent += payload.content;
            setStreamingContent(finalContent);
          }
        }
      }

      if (finalContent.trim()) {
        setMessages((current) => [...current, createMessage("assistant", finalContent.trim())]);
      }
      setEditingMessageId(null);
      setStreamingContent("");
    } catch (requestError) {
      if ((requestError as Error).name === "AbortError") {
        setStatus("Generation canceled.");
      } else {
        setError((requestError as Error).message || "Chat request failed.");
      }
    } finally {
      abortRef.current = null;
      setIsGenerating(false);
      setStreamingContent("");
    }
  };

  const handleSend = async () => {
    const input = session.draftInput;
    const trimmedInput = input.trim();

    if (!trimmedInput) {
      return;
    }

    if (editingMessageId) {
      const messageIndex = messages.findIndex((message) => message.id === editingMessageId);

      if (messageIndex >= 0) {
        const revised = messages.slice(0, messageIndex + 1).map((message, index) =>
          index === messageIndex ? { ...message, content: input.trim() } : message
        );
        const historyBeforeEditedMessage = revised.slice(0, messageIndex);
        await submitPrompt({
          input: trimmedInput,
          baseMessages: historyBeforeEditedMessage,
          nextMessages: revised
        });
      }

      return;
    }

    const userMessage = createMessage("user", trimmedInput);
    await submitPrompt({
      input: trimmedInput,
      baseMessages: messages,
      nextMessages: [...messages, userMessage]
    });
  };

  const handleRetry = async () => {
    if (!lastRequest) {
      return;
    }

    const userMessage = createMessage("user", lastRequest.input);
    await submitPrompt({
      input: lastRequest.input,
      baseMessages: lastRequest.baseMessages,
      nextMessages: [...lastRequest.baseMessages, userMessage]
    });
  };

  const handleCancel = () => {
    abortRef.current?.abort();
    setStreamingContent("");
    setIsGenerating(false);
  };

  const handleUpload = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const targetFiles = event.target.files;

    if (!targetFiles?.length) {
      return;
    }

    const formData = new FormData();
    Array.from(targetFiles).forEach((file) => formData.append("files", file));

    try {
      const response = await fetch("/api/upload", {
        method: "POST",
        body: formData
      });

      if (!response.ok) {
        const payload = (await response.json()) as { error?: string };
        throw new Error(payload.error || "Upload failed.");
      }

      const payload = (await response.json()) as { files: UploadedFile[] };
      setFiles((current) => [...current, ...payload.files]);
    } catch (uploadError) {
      setError((uploadError as Error).message || "Upload failed.");
    } finally {
      event.target.value = "";
    }
  };

  const startEditingMessage = (message: Message) => {
    setEditingMessageId(message.id);
    updateDraftInput(message.content);
    setStatus("Editing earlier message. Later conversation will be replaced on resend.");
  };

  const resetConversation = async () => {
    setMessages([]);
    setStreamingContent("");
    setError("");
    setStatus("Conversation cleared.");
    setEditingMessageId(null);
    setLastRequest(null);
    await clearMessages();
  };

  const startResize = (handle: "left" | "right", clientX: number) => {
    dragRef.current = {
      startX: clientX,
      widths: session.paneWidths,
      handle
    };
    document.body.classList.add("is-resizing");
  };

  return (
    <div className="app-shell">
      <aside className="pane pane-left" style={{ width: `${session.paneWidths.left}%` }}>
        <div className="pane-header">
          <h2>Files</h2>
          <label className="upload-button">
            Upload
            <input type="file" multiple onChange={handleUpload} />
          </label>
        </div>
        <div className="pane-content scrollable">
          {files.length === 0 ? <p className="empty-state">Upload files to add local context.</p> : null}
          {files.map((file) => (
            <label className="file-card" key={file.id}>
              <input
                checked={file.isSelected}
                type="checkbox"
                onChange={() =>
                  setFiles((current) =>
                    current.map((entry) =>
                      entry.id === file.id ? { ...entry, isSelected: !entry.isSelected } : entry
                    )
                  )
                }
              />
              <div>
                <strong>{file.name}</strong>
                <p>{file.type || "text/plain"}</p>
                <p>{Math.round(file.size / 1024)} KB</p>
                <p>{new Date(file.uploadedAt).toLocaleString()}</p>
                {file.warning ? <span className="warning-tag">{file.warning}</span> : null}
              </div>
            </label>
          ))}
        </div>
      </aside>

      <div className="splitter" onMouseDown={(event) => startResize("left", event.clientX)} />

      <main className="pane pane-center" style={{ width: `${session.paneWidths.center}%` }}>
        <div className="hero">
          <img alt="Sabio logo" src={logoUrl} />
          <div>
            <h1>Sabio</h1>
            <p>Local Ollama Workspace</p>
          </div>
        </div>

        <div className="toolbar">
          <button type="button" onClick={resetConversation}>
            Clear context
          </button>
          {contextWarning ? <span className="status-warning">{contextWarning}</span> : null}
          {status ? <span className="status-note">{status}</span> : null}
        </div>

        <div className="pane-content chat-scroll" ref={chatScrollRef}>
          {messages.length === 0 ? (
            <div className="empty-chat">
              <p>Ask Sabio to analyze files, draft Markdown, or work with your local Ollama models.</p>
            </div>
          ) : null}

          {messages.map((message) => (
            <article className={`message message-${message.role}`} key={message.id}>
              <div className="message-meta">
                <span>{message.role === "user" ? "You" : "Sabio"}</span>
                {message.role === "user" ? (
                  <button type="button" onClick={() => startEditingMessage(message)}>
                    Edit & rerun
                  </button>
                ) : (
                  <div className="message-actions">
                    <button type="button" onClick={() => copyText(message.content)}>
                      Copy
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        const blob = new Blob([message.content], { type: "text/markdown;charset=utf-8" });
                        const url = URL.createObjectURL(blob);
                        const anchor = document.createElement("a");
                        anchor.href = url;
                        anchor.download = `sabio-response-${message.createdAt}.md`;
                        anchor.click();
                        URL.revokeObjectURL(url);
                      }}
                    >
                      Download .md
                    </button>
                  </div>
                )}
              </div>
              {message.role === "assistant" ? (
                <ReactMarkdown
                  className="markdown-body"
                  rehypePlugins={[rehypeHighlight]}
                  remarkPlugins={[remarkGfm]}
                  components={{
                    code(props) {
                      const { children, className, ...rest } = props;
                      const codeValue = String(children).replace(/\n$/, "");
                      const isBlock = codeValue.includes("\n");

                      if (!isBlock) {
                        return (
                          <code className={className} {...rest}>
                            {children}
                          </code>
                        );
                      }

                      return (
                        <div className="code-block">
                          <button type="button" onClick={() => copyText(codeValue)}>
                            Copy code
                          </button>
                          <code className={className} {...rest}>
                            {children}
                          </code>
                        </div>
                      );
                    }
                  }}
                >
                  {message.content}
                </ReactMarkdown>
              ) : (
                <p>{message.content}</p>
              )}
            </article>
          ))}

          {streamingContent ? (
            <article className="message message-assistant is-streaming">
              <div className="message-meta">
                <span>Sabio</span>
              </div>
              <ReactMarkdown className="markdown-body" remarkPlugins={[remarkGfm]}>
                {streamingContent}
              </ReactMarkdown>
            </article>
          ) : null}

          {error ? (
            <div className="error-banner">
              <span>{error}</span>
              <button type="button" onClick={handleRetry} disabled={!lastRequest}>
                Retry
              </button>
            </div>
          ) : null}
        </div>

        <div className="composer">
          <textarea
            placeholder="Write a prompt. Press Ctrl+Shift+Enter to send."
            value={session.draftInput}
            onChange={(event) => updateDraftInput(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && event.ctrlKey && event.shiftKey) {
                event.preventDefault();
                void handleSend();
              }
            }}
          />
          <div className="composer-actions">
            <div className="selection-summary">
              <span>{selectedFiles.length} files selected</span>
              <span>{session.selectedModel || "No model selected"}</span>
            </div>
            <div>
              {isGenerating ? (
                <button type="button" className="secondary-button" onClick={handleCancel}>
                  Cancel
                </button>
              ) : null}
              <button type="button" onClick={() => void handleSend()} disabled={isGenerating}>
                Send
              </button>
            </div>
          </div>
        </div>
      </main>

      <div className="splitter" onMouseDown={(event) => startResize("right", event.clientX)} />

      <aside className="pane pane-right" style={{ width: `${session.paneWidths.right}%` }}>
        <div className="pane-header">
          <h2>Settings</h2>
        </div>
        <div className="pane-content scrollable settings-stack">
          <label className="field">
            <span>Model</span>
            <select
              value={session.selectedModel}
              onChange={(event) =>
                setSession((current) => ({
                  ...current,
                  selectedModel: event.target.value
                }))
              }
            >
              <option value="">Select a model</option>
              {models.map((model) => (
                <option key={model.name} value={model.name}>
                  {model.name}
                </option>
              ))}
            </select>
          </label>

          <label className="field">
            <span>System prompt</span>
            <textarea
              rows={18}
              value={session.systemPrompt}
              onChange={(event) =>
                setSession((current) => ({
                  ...current,
                  systemPrompt: event.target.value
                }))
              }
            />
          </label>

          <button
            type="button"
            className="secondary-button"
            onClick={() =>
              setSession((current) => ({
                ...current,
                systemPrompt: DEFAULT_SYSTEM_PROMPT
              }))
            }
          >
            Reset default prompt
          </button>
        </div>
      </aside>
    </div>
  );
}

export default App;
