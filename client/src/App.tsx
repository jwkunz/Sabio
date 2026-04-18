import { useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeHighlight from "rehype-highlight";
import rehypeKatex from "rehype-katex";
import { buildPrompt, DEFAULT_SYSTEM_PROMPT, trimHistoryForContext } from "../../shared/prompt";
import logoUrl from "../../assets/Sabio_logo.png";
import versionText from "../../VERSION?raw";
import { clearMessages, loadFiles, loadMessages, loadSession, saveFiles, saveMessages, saveSession } from "./lib/db";
import {
  downloadFileBundle,
  downloadTextFile,
  extractFilesFromMarkdown,
  inferCodeBlockFilename
} from "./lib/fileBundle";
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
const appVersion = versionText.trim();

const downloadMarkdown = (content: string, createdAt: number) => {
  const blob = new Blob([content], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `sabio-response-${createdAt}.md`;
  anchor.click();
  URL.revokeObjectURL(url);
};

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
  const [activePanel, setActivePanel] = useState<"help" | "legal" | null>(null);
  const [promptHistoryCursor, setPromptHistoryCursor] = useState<number | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const chatScrollRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{ startX: number; widths: PaneWidths; handle: "left" | "right" } | null>(null);
  const draftBeforeHistoryRef = useRef("");

  const selectedFiles = useMemo(
    () => files.filter((file) => file.isSelected).sort((a, b) => a.uploadedAt - b.uploadedAt),
    [files]
  );
  const promptHistory = useMemo(
    () => messages.filter((message) => message.role === "user").map((message) => message.content),
    [messages]
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

  const updateDraftInput = (value: string, options?: { preserveHistoryCursor?: boolean }) => {
    if (!options?.preserveHistoryCursor) {
      setPromptHistoryCursor(null);
      draftBeforeHistoryRef.current = "";
    }

    setSession((current) => ({
      ...current,
      draftInput: value
    }));
  };

  const navigatePromptHistory = (direction: "up" | "down") => {
    if (promptHistory.length === 0) {
      return;
    }

    if (promptHistoryCursor === null) {
      if (direction === "down") {
        return;
      }

      draftBeforeHistoryRef.current = session.draftInput;
      const nextCursor = promptHistory.length - 1;
      setPromptHistoryCursor(nextCursor);
      updateDraftInput(promptHistory[nextCursor], { preserveHistoryCursor: true });
      return;
    }

    if (direction === "up") {
      const nextCursor = Math.max(0, promptHistoryCursor - 1);
      setPromptHistoryCursor(nextCursor);
      updateDraftInput(promptHistory[nextCursor], { preserveHistoryCursor: true });
      return;
    }

    const nextCursor = promptHistoryCursor + 1;

    if (nextCursor >= promptHistory.length) {
      setPromptHistoryCursor(null);
      updateDraftInput(draftBeforeHistoryRef.current, { preserveHistoryCursor: true });
      draftBeforeHistoryRef.current = "";
      return;
    }

    setPromptHistoryCursor(nextCursor);
    updateDraftInput(promptHistory[nextCursor], { preserveHistoryCursor: true });
  };

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
    setPromptHistoryCursor(null);
    draftBeforeHistoryRef.current = "";

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

  const panelTitle =
    activePanel === "help" ? "Help" : activePanel === "legal" ? "Legal" : "";

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
        <div className="pane-footer">
          <button type="button" className="secondary-button" onClick={() => setActivePanel("help")}>
            Help
          </button>
          <button type="button" className="secondary-button" onClick={() => setActivePanel("legal")}>
            Legal
          </button>
        </div>
      </aside>

      <div className="splitter" onMouseDown={(event) => startResize("left", event.clientX)} />

      <main className="pane pane-center" style={{ width: `${session.paneWidths.center}%` }}>
        <div className="hero">
          <img alt="Sabio logo" src={logoUrl} />
          <div>
            <h1>Sabio</h1>
            <p>Local Ollama Workspace</p>
            <span className="hero-version">{appVersion}</span>
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
                    {(() => {
                      const extractedFiles = extractFilesFromMarkdown(message.content);

                      return extractedFiles.length >= 2 ? (
                        <button
                          type="button"
                          onClick={() =>
                            void downloadFileBundle(extractedFiles, `sabio-files-${message.createdAt}.zip`)
                          }
                        >
                          Download .zip
                        </button>
                      ) : null;
                    })()}
                    <button type="button" onClick={() => copyText(message.content)}>
                      Copy
                    </button>
                    <button
                      type="button"
                      onClick={() => downloadMarkdown(message.content, message.createdAt)}
                    >
                      Download .md
                    </button>
                  </div>
                )}
              </div>
              {message.role === "assistant" ? (
                (() => {
                  let codeBlockIndex = 0;

                  return (
                    <ReactMarkdown
                      className="markdown-body"
                  rehypePlugins={[rehypeKatex, rehypeHighlight]}
                  remarkPlugins={[remarkGfm, remarkMath]}
                      components={{
                        code(props) {
                          const { children, className, ...rest } = props;
                          const codeValue = String(children).replace(/\n$/, "");
                          const isBlock = codeValue.includes("\n");
                          const language = className?.replace(/^language-/, "") ?? "";

                          if (!isBlock) {
                            return (
                              <code className={className} {...rest}>
                                {children}
                              </code>
                            );
                          }

                          const currentBlockIndex = codeBlockIndex;
                          codeBlockIndex += 1;

                          return (
                            <div className="code-block">
                              <div className="code-block-actions">
                                <button type="button" onClick={() => copyText(codeValue)}>
                                  Copy code
                                </button>
                                <button
                                  type="button"
                                  onClick={() =>
                                    downloadTextFile({
                                      content: codeValue,
                                      filename: inferCodeBlockFilename({
                                        messageContent: message.content,
                                        language,
                                        blockIndex: currentBlockIndex
                                      })
                                    })
                                  }
                                >
                                  Download file
                                </button>
                              </div>
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
                  );
                })()
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
              <ReactMarkdown
                className="markdown-body"
                rehypePlugins={[rehypeKatex]}
                remarkPlugins={[remarkGfm, remarkMath]}
              >
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
          <div className="composer-input-row">
            <div className="prompt-history-controls" aria-label="Prompt history controls">
              <button
                type="button"
                className="secondary-button"
                aria-label="Previous prompt"
                title="Previous prompt"
                onClick={() => navigatePromptHistory("up")}
                disabled={promptHistory.length === 0}
              >
                ↑
              </button>
              <button
                type="button"
                className="secondary-button"
                aria-label="Next prompt"
                title="Next prompt"
                onClick={() => navigatePromptHistory("down")}
                disabled={promptHistory.length === 0 || promptHistoryCursor === null}
              >
                ↓
              </button>
            </div>
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
          </div>
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
            <small>
              This text is prepended to every chat request before the conversation, selected files, and your current
              prompt.
            </small>
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

      {activePanel ? (
        <div className="overlay" onClick={() => setActivePanel(null)}>
          <section className="dialog" onClick={(event) => event.stopPropagation()}>
            <div className="dialog-header">
              <h2>{panelTitle}</h2>
              <button type="button" className="secondary-button" onClick={() => setActivePanel(null)}>
                Close
              </button>
            </div>

            {activePanel === "help" ? (
              <div className="dialog-body">
                <p>
                  Sabio is a local-first workspace for working with a locally running Ollama model through a dedicated
                  chat interface. The layout is split into three panes so you can manage files on the left, conduct the
                  conversation in the center, and adjust settings on the right without leaving the main screen.
                </p>
                <p>
                  Begin by selecting a model in the settings pane. Sabio reads the model list from your local Ollama
                  instance and remembers the selected model between sessions. You can also edit the system prompt in
                  the settings pane to change response style, formatting rules, or task constraints. The system prompt
                  is prepended to every chat request before the conversation history, selected files, and your current
                  prompt. Restore the default behavior at any time with <strong>Reset default prompt</strong>.
                </p>
                <p>
                  <strong>Pro-tip:</strong> You can ask the model itself to generate a strong system prompt for a
                  specific application. Describe the role, audience, preferred output format, constraints, and examples
                  of good behavior, then paste the generated prompt into the system prompt editor.
                </p>
                <p>
                  To provide source material, use <strong>Upload</strong> in the file pane. Sabio extracts raw text
                  from supported documents including plain text, Markdown, JSON, CSV, source code, PDF, and DOCX
                  files. Uploaded files are stored locally in your browser and remain available after a reload. Use the
                  checkbox beside each file to control whether it is included in the next prompt. Only the files
                  currently selected are sent as context.
                </p>
                <p>
                  The center pane is the primary workspace. Type a multi-line prompt in the composer and press
                  <strong> Ctrl+Shift+Enter</strong> or click <strong>Send</strong>. Sabio keeps conversation history
                  locally and assembles each request from the system prompt, prior conversation, selected files, and
                  your current input. As the model responds, the assistant output streams into a temporary bubble. When
                  generation finishes successfully, the response is committed to history.
                </p>
                <p>
                  If a response is not going in the right direction, use <strong>Cancel</strong> during generation.
                  This stops the active request and avoids saving a completed assistant message for that interrupted
                  turn. If a request fails, Sabio shows an error banner with a <strong>Retry</strong> option. If you
                  need to revise an earlier user prompt, use <strong>Edit &amp; rerun</strong> on that message. Sabio
                  will truncate the later conversation, replace the edited message, and regenerate from that point.
                </p>
                <p>
                  Assistant responses are rendered as Markdown. Each assistant message includes a <strong>Copy</strong>
                  action that copies the raw Markdown and a <strong>Download .md</strong> action that exports the
                  response as a Markdown file. Fenced code blocks also include a dedicated <strong>Copy code</strong>
                  button for snippet-level reuse.
                </p>
                <p>
                  Sabio persists your session locally, including message history, uploaded files, file selections,
                  draft input, pane widths, selected model, and system prompt. Use <strong>Clear context</strong> to
                  reset the conversation when you want a fresh chat while keeping the rest of your local workspace
                  intact.
                </p>
              </div>
            ) : (
              <div className="dialog-body">
                <p><strong>Copyright 2026 Numerius Engineering LLC</strong></p>
                <p>
                  Sabio is a local-first application. Sabio does not collect telemetry, analytics, usage metrics,
                  prompt histories, uploaded document contents, model responses, or any other application data for
                  centralized monitoring or vendor-side retention.
                </p>
                <p>
                  Sabio does not send your prompts, files, or chat history to a Sabio-operated cloud service. Local
                  application state is stored in your browser so the interface can restore your session, files, pane
                  sizes, and settings after a reload.
                </p>
                <p>
                  All prompt data is handled by the large language model endpoint that you configure for Sabio. In the
                  default configuration, that endpoint is your local Ollama instance. When you submit a prompt, Sabio
                  forwards the assembled request to that endpoint so the model can generate a response.
                </p>
                <p>
                  Sabio itself does not maintain centralized logs of prompt data or completions. Any data handling,
                  logging, retention, transport, or privacy characteristics beyond the Sabio application are governed
                  by the large language model endpoint and infrastructure you choose to use. You are responsible for
                  evaluating that endpoint’s privacy and security posture.
                </p>
              </div>
            )}
          </section>
        </div>
      ) : null}
    </div>
  );
}

export default App;
