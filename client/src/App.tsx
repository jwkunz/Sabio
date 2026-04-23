import { useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeHighlight from "rehype-highlight";
import rehypeKatex from "rehype-katex";
import { BUILT_IN_SYSTEM_PROMPT_PROFILES, buildPrompt, DEFAULT_SYSTEM_PROMPT, trimHistoryForContext } from "../../shared/prompt";
import logoUrl from "../../assets/Sabio_logo.png";
import versionText from "../../VERSION?raw";
import {
  clearMessages,
  builtInSystemPromptProfiles,
  loadFiles,
  loadMessages,
  loadSession,
  loadSystemPromptProfiles,
  saveFiles,
  saveMessages,
  saveSession,
  saveSystemPromptProfiles
} from "./lib/db";
import {
  downloadFileBundle,
  downloadTextFile,
  extractFilesFromMarkdown,
  inferCodeBlockFilename
} from "./lib/fileBundle";
import { normalizeMathDelimiters } from "./lib/markdown";
import type {
  AppMode,
  AgentEvent,
  AgentSessionSummary,
  AgentToolSpec,
  DisplayFontSize,
  DisplayTheme,
  Message,
  ModelOption,
  PaneWidths,
  SessionState,
  SystemPromptProfile,
  UploadedFile
} from "./types/app";

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

const defaultDisplayPreferences = {
  theme: "dark" as DisplayTheme,
  fontSize: "medium" as DisplayFontSize
};

const copyText = async (value: string) => navigator.clipboard.writeText(value);
const appVersion = versionText.trim();

const formatElapsedSeconds = (seconds: number) => {
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;

  return minutes > 0 ? `${minutes}m ${remainder}s` : `${remainder}s`;
};

const formatAgentEventType = (eventType: string) =>
  eventType
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");

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
    appMode: "chat",
    selectedModel: "",
    systemPrompt: DEFAULT_SYSTEM_PROMPT,
    selectedSystemPromptProfileId: "generic",
    draftInput: "",
    paneWidths: defaultPaneWidths,
    displayPreferences: defaultDisplayPreferences,
    agentWorkspace: {
      inputPath: "",
      canonicalPath: "",
      isGitRepo: false,
      gitBranch: "",
      cleanWorktree: null,
      trusted: false,
      message: "No workspace selected."
    }
  });
  const [systemPromptProfiles, setSystemPromptProfiles] = useState<SystemPromptProfile[]>([]);
  const [models, setModels] = useState<ModelOption[]>([]);
  const [modelStatus, setModelStatus] = useState("");
  const [streamingContent, setStreamingContent] = useState("");
  const [generationStatus, setGenerationStatus] = useState("");
  const [generationStartedAt, setGenerationStartedAt] = useState<number | null>(null);
  const [generationElapsedSeconds, setGenerationElapsedSeconds] = useState(0);
  const [status, setStatus] = useState("Loading session...");
  const [error, setError] = useState("");
  const [contextWarning, setContextWarning] = useState("");
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [lastRequest, setLastRequest] = useState<{ input: string; baseMessages: Message[] } | null>(null);
  const [isGenerating, setIsGenerating] = useState(false);
  const [activePanel, setActivePanel] = useState<"help" | "legal" | null>(null);
  const [promptHistoryCursor, setPromptHistoryCursor] = useState<number | null>(null);
  const [agentSessions, setAgentSessions] = useState<AgentSessionSummary[]>([]);
  const [selectedAgentSessionId, setSelectedAgentSessionId] = useState("");
  const [agentEvents, setAgentEvents] = useState<AgentEvent[]>([]);
  const [agentSessionStatus, setAgentSessionStatus] = useState("");
  const [agentTools, setAgentTools] = useState<AgentToolSpec[]>([]);
  const abortRef = useRef<AbortController | null>(null);
  const chatScrollRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{ startX: number; widths: PaneWidths; handle: "left" | "right" } | null>(null);
  const draftBeforeHistoryRef = useRef("");

  const selectedFiles = useMemo(
    () => files.filter((file) => file.isSelected).sort((a, b) => a.uploadedAt - b.uploadedAt),
    [files]
  );
  const selectedSystemPromptProfile = useMemo(
    () =>
      systemPromptProfiles.find((profile) => profile.id === session.selectedSystemPromptProfileId) ??
      systemPromptProfiles.find((profile) => profile.id === "generic"),
    [session.selectedSystemPromptProfileId, systemPromptProfiles]
  );
  const promptHistory = useMemo(
    () => messages.filter((message) => message.role === "user").map((message) => message.content),
    [messages]
  );
  const selectedAgentSession = useMemo(
    () => agentSessions.find((agentSession) => agentSession.id === selectedAgentSessionId) ?? null,
    [agentSessions, selectedAgentSessionId]
  );

  const loadModelOptions = async () => {
    setModelStatus("Loading models...");

    try {
      const response = await fetch("/api/models");

      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error || "Model discovery failed.");
      }

      const { models: modelOptions } = (await response.json()) as { models: ModelOption[] };

      setModels(modelOptions);
      setModelStatus(modelOptions.length > 0 ? `${modelOptions.length} models available.` : "No Ollama models found.");
      setSession((current) => {
        if (current.selectedModel && modelOptions.some((model) => model.name === current.selectedModel)) {
          return current;
        }

        return {
          ...current,
          selectedModel: modelOptions[0]?.name ?? ""
        };
      });
    } catch (modelError) {
      setModels([]);
      setModelStatus((modelError as Error).message || "Unable to load models.");
      setError("Ollama is unavailable. Start Ollama locally to list models.");
    }
  };

  useEffect(() => {
    const hydrate = async () => {
      const [storedSession, storedMessages, storedFiles] = await Promise.all([
        loadSession(),
        loadMessages(),
        loadFiles()
      ]);
      const profiles = await loadSystemPromptProfiles();
      const selectedProfile =
        profiles.find((profile) => profile.id === storedSession.selectedSystemPromptProfileId) ??
        profiles.find((profile) => profile.id === "generic");
      const systemPrompt = storedSession.systemPrompt || selectedProfile?.content || DEFAULT_SYSTEM_PROMPT;

      setSession({
        ...storedSession,
        systemPrompt,
        selectedSystemPromptProfileId: selectedProfile?.id ?? "generic",
        paneWidths: storedSession.paneWidths || defaultPaneWidths,
        displayPreferences: storedSession.displayPreferences || defaultDisplayPreferences
      });
      setSystemPromptProfiles(
        profiles.map((profile) =>
          profile.id === selectedProfile?.id && storedSession.systemPrompt
            ? { ...profile, content: storedSession.systemPrompt, updatedAt: Date.now() }
            : profile
        )
      );
      setMessages(storedMessages);
      setFiles(storedFiles);
      setStatus("");
      setIsHydrated(true);
    };

    hydrate().catch(() => {
      const profiles = builtInSystemPromptProfiles();
      const generic = profiles.find((profile) => profile.id === "generic");
      setSystemPromptProfiles(profiles);
      setSession((current) => ({
        ...current,
        selectedSystemPromptProfileId: generic?.id ?? "generic",
        systemPrompt: current.systemPrompt || generic?.content || DEFAULT_SYSTEM_PROMPT,
        displayPreferences: current.displayPreferences || defaultDisplayPreferences
      }));
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
    if (!isHydrated || systemPromptProfiles.length === 0) {
      return;
    }

    saveSystemPromptProfiles(systemPromptProfiles).catch(() => {
      setError("Unable to persist system prompt profiles.");
    });
  }, [isHydrated, systemPromptProfiles]);

  useEffect(() => {
    if (!isHydrated) {
      return;
    }

    void loadModelOptions();
  }, [isHydrated]);

  useEffect(() => {
    if (!isHydrated || session.appMode !== "agent" || !session.agentWorkspace.canonicalPath) {
      return;
    }

    void loadAgentSessions(session.agentWorkspace.canonicalPath);
  }, [isHydrated, session.appMode, session.agentWorkspace.canonicalPath]);

  useEffect(() => {
    if (!isHydrated || session.appMode !== "agent") {
      return;
    }

    void loadAgentTools();
  }, [isHydrated, session.appMode]);

  useEffect(() => {
    if (!selectedAgentSessionId) {
      setAgentEvents([]);
      return;
    }

    void loadAgentEvents(selectedAgentSessionId);
  }, [selectedAgentSessionId]);

  useEffect(() => {
    chatScrollRef.current?.scrollTo({
      top: chatScrollRef.current.scrollHeight,
      behavior: "smooth"
    });
  }, [messages, streamingContent, generationStatus, generationElapsedSeconds, error]);

  useEffect(() => {
    if (!isGenerating || generationStartedAt === null) {
      setGenerationElapsedSeconds(0);
      return;
    }

    const updateElapsed = () => {
      setGenerationElapsedSeconds(Math.max(0, Math.floor((Date.now() - generationStartedAt) / 1000)));
    };

    updateElapsed();
    const intervalId = window.setInterval(updateElapsed, 1000);

    return () => window.clearInterval(intervalId);
  }, [generationStartedAt, isGenerating]);

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

  const updateSelectedSystemPromptProfile = (profileId: string) => {
    const profile = systemPromptProfiles.find((entry) => entry.id === profileId);

    if (!profile) {
      return;
    }

    setSession((current) => ({
      ...current,
      selectedSystemPromptProfileId: profile.id,
      systemPrompt: profile.content
    }));
  };

  const updateSystemPrompt = (content: string) => {
    const now = Date.now();

    setSession((current) => ({
      ...current,
      systemPrompt: content
    }));
    setSystemPromptProfiles((current) =>
      current.map((profile) =>
        profile.id === session.selectedSystemPromptProfileId ? { ...profile, content, updatedAt: now } : profile
      )
    );
  };

  const createCustomSystemPromptProfile = () => {
    const existingCustomCount = systemPromptProfiles.filter((profile) => !profile.isBuiltIn).length;
    const profile: SystemPromptProfile = {
      id: `custom-${crypto.randomUUID()}`,
      name: `Custom ${existingCustomCount + 1}`,
      content: session.systemPrompt || selectedSystemPromptProfile?.content || DEFAULT_SYSTEM_PROMPT,
      isBuiltIn: false,
      updatedAt: Date.now()
    };

    setSystemPromptProfiles((current) => [...current, profile]);
    setSession((current) => ({
      ...current,
      selectedSystemPromptProfileId: profile.id,
      systemPrompt: profile.content
    }));
  };

  const renameSelectedSystemPromptProfile = (name: string) => {
    setSystemPromptProfiles((current) =>
      current.map((profile) =>
        profile.id === session.selectedSystemPromptProfileId
          ? { ...profile, name: name || "Untitled profile", updatedAt: Date.now() }
          : profile
      )
    );
  };

  const resetSelectedSystemPromptProfile = () => {
    const builtIn = BUILT_IN_SYSTEM_PROMPT_PROFILES.find(
      (profile) => profile.id === session.selectedSystemPromptProfileId
    );
    const content = builtIn?.content ?? DEFAULT_SYSTEM_PROMPT;

    setSession((current) => ({
      ...current,
      systemPrompt: content
    }));
    setSystemPromptProfiles((current) =>
      current.map((profile) =>
        profile.id === session.selectedSystemPromptProfileId
          ? { ...profile, content, updatedAt: Date.now() }
          : profile
      )
    );
  };

  const deleteSelectedSystemPromptProfile = () => {
    if (!selectedSystemPromptProfile || selectedSystemPromptProfile.isBuiltIn) {
      return;
    }

    const generic = systemPromptProfiles.find((profile) => profile.id === "generic");
    const nextProfiles = systemPromptProfiles.filter((profile) => profile.id !== selectedSystemPromptProfile.id);

    setSystemPromptProfiles(nextProfiles);
    setSession((current) => ({
      ...current,
      selectedSystemPromptProfileId: generic?.id ?? "generic",
      systemPrompt: generic?.content ?? DEFAULT_SYSTEM_PROMPT
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
    setGenerationStartedAt(Date.now());
    setGenerationStatus("Contacting the Ollama engine...");
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

      setGenerationStatus("Engine connected. Waiting for the first token...");

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

          if (payload.type === "error") {
            throw new Error(payload.content || "Chat stream failed.");
          }

          if (payload.type === "chunk" && payload.content) {
            finalContent += payload.content;
            setStreamingContent(finalContent);
            setGenerationStatus(
              `Streaming response: ${finalContent.length.toLocaleString()} characters received.`
            );
          }

          if (payload.type === "done") {
            setGenerationStatus("Finalizing response...");
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
      setGenerationStartedAt(null);
      setGenerationStatus("");
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
    setGenerationStartedAt(null);
    setGenerationStatus("");
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

  const updateDisplayPreferences = (updates: Partial<SessionState["displayPreferences"]>) => {
    setSession((current) => ({
      ...current,
      displayPreferences: {
        ...current.displayPreferences,
        ...updates
      }
    }));
  };

  const updateAppMode = (appMode: AppMode) => {
    setSession((current) => ({
      ...current,
      appMode
    }));
  };

  const loadAgentSessions = async (workspacePath?: string) => {
    const query = workspacePath ? `?workspacePath=${encodeURIComponent(workspacePath)}` : "";

    try {
      const response = await fetch(`/api/agent/sessions${query}`);

      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error || "Unable to load agent sessions.");
      }

      const sessions = (await response.json()) as AgentSessionSummary[];
      setAgentSessions(sessions);

      if (sessions.length > 0) {
        setSelectedAgentSessionId((current) =>
          current && sessions.some((agentSession) => agentSession.id === current)
            ? current
            : sessions[0].id
        );
      } else {
        setSelectedAgentSessionId("");
        setAgentEvents([]);
      }
    } catch (sessionError) {
      setAgentSessionStatus((sessionError as Error).message || "Unable to load agent sessions.");
    }
  };

  const loadAgentTools = async () => {
    try {
      const response = await fetch("/api/agent/tools");

      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error || "Unable to load agent tools.");
      }

      setAgentTools((await response.json()) as AgentToolSpec[]);
    } catch (toolError) {
      setAgentSessionStatus((toolError as Error).message || "Unable to load agent tools.");
    }
  };

  const loadAgentEvents = async (sessionId: string) => {
    try {
      const response = await fetch(`/api/agent/sessions/${encodeURIComponent(sessionId)}/events`);

      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error || "Unable to load agent events.");
      }

      const payload = (await response.json()) as { events: AgentEvent[] };
      setAgentEvents(payload.events);
    } catch (eventError) {
      setAgentSessionStatus((eventError as Error).message || "Unable to load agent events.");
    }
  };

  const createAgentSession = async () => {
    const workspace = session.agentWorkspace;

    if (!workspace.canonicalPath) {
      setAgentSessionStatus("Validate a workspace before creating an agent session.");
      return null;
    }

    try {
      const response = await fetch("/api/agent/sessions", {
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        },
        body: JSON.stringify({
          workspacePath: workspace.canonicalPath,
          gitBranch: workspace.gitBranch || null
        })
      });

      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error || "Unable to create agent session.");
      }

      const created = (await response.json()) as AgentSessionSummary & { eventLog?: AgentEvent[] };
      setAgentSessionStatus("Agent session ready.");
      await loadAgentSessions(workspace.canonicalPath);
      setSelectedAgentSessionId(created.id);
      setAgentEvents(created.eventLog ?? []);
      return created;
    } catch (sessionError) {
      setAgentSessionStatus((sessionError as Error).message || "Unable to create agent session.");
      return null;
    }
  };

  const renameAgentSession = async (title: string) => {
    if (!selectedAgentSessionId) {
      return;
    }

    try {
      const response = await fetch(`/api/agent/sessions/${encodeURIComponent(selectedAgentSessionId)}/rename`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        },
        body: JSON.stringify({ title })
      });

      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error || "Unable to rename agent session.");
      }

      await loadAgentSessions(session.agentWorkspace.canonicalPath);
      setAgentSessionStatus("Session renamed.");
    } catch (renameError) {
      setAgentSessionStatus((renameError as Error).message || "Unable to rename agent session.");
    }
  };

  const updateWorkspaceInput = (inputPath: string) => {
    setSession((current) => ({
      ...current,
      agentWorkspace: {
        ...current.agentWorkspace,
        inputPath,
        trusted: false
      }
    }));
  };

  const validateWorkspace = async () => {
    const path = session.agentWorkspace.inputPath.trim();

    if (!path) {
      setError("Enter a workspace path before validating.");
      return;
    }

    setError("");

    try {
      const response = await fetch("/api/agent/workspace/validate", {
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        },
        body: JSON.stringify({ path })
      });

      const payload = (await response.json().catch(() => null)) as
        | {
            canonicalPath?: string;
            isGitRepo?: boolean;
            gitBranch?: string;
            cleanWorktree?: boolean | null;
            message?: string;
            error?: string;
          }
        | null;

      if (!response.ok) {
        throw new Error(payload?.error || "Workspace validation failed.");
      }

      setSession((current) => ({
        ...current,
        agentWorkspace: {
          inputPath: path,
          canonicalPath: payload?.canonicalPath ?? "",
          isGitRepo: payload?.isGitRepo ?? false,
          gitBranch: payload?.gitBranch ?? "",
          cleanWorktree: payload?.cleanWorktree ?? null,
          trusted: false,
          message: payload?.message ?? "Workspace validated."
        }
      }));
    } catch (workspaceError) {
      setError((workspaceError as Error).message || "Workspace validation failed.");
      setSession((current) => ({
        ...current,
        agentWorkspace: {
          ...current.agentWorkspace,
          trusted: false,
          message: "Workspace validation failed."
        }
      }));
    }
  };

  const trustWorkspace = async () => {
    const workspace = session.agentWorkspace;
    const canTrust = Boolean(workspace.canonicalPath && workspace.isGitRepo && workspace.cleanWorktree);

    if (!canTrust) {
      setSession((current) => ({
        ...current,
        agentWorkspace: {
          ...current.agentWorkspace,
          trusted: false,
          message: "Workspace must be a clean git repository before trust."
        }
      }));
      return;
    }

    setSession((current) => ({
      ...current,
      agentWorkspace: {
        ...current.agentWorkspace,
        trusted: true,
        message: "Workspace trusted."
      }
    }));

    await createAgentSession();
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
    <div
      className={`app-shell theme-${session.displayPreferences.theme} font-${session.displayPreferences.fontSize}`}
    >
      <aside className="pane pane-left" style={{ width: `${session.paneWidths.left}%` }}>
        <div className="brand-panel">
          <img alt="Sabio logo" src={logoUrl} />
          <div>
            <h1>Sabio</h1>
            <p>Local Ollama Workspace</p>
            <span className="hero-version">{appVersion}</span>
          </div>
        </div>
        <div className="mode-switch" role="tablist" aria-label="Sabio mode">
          <button
            type="button"
            className={session.appMode === "chat" ? "mode-button is-active" : "mode-button"}
            role="tab"
            aria-selected={session.appMode === "chat"}
            onClick={() => updateAppMode("chat")}
          >
            Chat
          </button>
          <button
            type="button"
            className={session.appMode === "agent" ? "mode-button is-active" : "mode-button"}
            role="tab"
            aria-selected={session.appMode === "agent"}
            onClick={() => updateAppMode("agent")}
          >
            Agent
          </button>
        </div>
        <div className="pane-footer">
          <button type="button" className="secondary-button" onClick={() => setActivePanel("help")}>
            Help
          </button>
          <button type="button" className="secondary-button" onClick={() => setActivePanel("legal")}>
            Legal
          </button>
        </div>
        {session.appMode === "chat" ? (
          <>
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
                <article className="file-card" key={file.id}>
                  <label className="file-include-toggle">
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
                    <span>Included?</span>
                  </label>
                  <div className="file-card-body">
                    <div className="file-card-main">
                      <strong>{file.name}</strong>
                      <p>{file.type || "text/plain"}</p>
                      <p>{Math.round(file.size / 1024)} KB</p>
                      <p>{new Date(file.uploadedAt).toLocaleString()}</p>
                      {file.warning ? <span className="warning-tag">{file.warning}</span> : null}
                    </div>
                    <button
                      type="button"
                      className="secondary-button remove-file-button"
                      onClick={() => setFiles((current) => current.filter((entry) => entry.id !== file.id))}
                    >
                      Remove
                    </button>
                  </div>
                </article>
              ))}
            </div>
          </>
        ) : (
          <>
            <div className="pane-header">
              <h2>Workspace</h2>
            </div>
            <div className="pane-content scrollable agent-sidebar">
              <label className="field compact-field">
                <span>Workspace path</span>
                <input
                  value={session.agentWorkspace.inputPath}
                  onChange={(event) => updateWorkspaceInput(event.target.value)}
                  placeholder="/path/to/project"
                />
              </label>
              <div className="settings-button-row">
                <button type="button" className="secondary-button" onClick={() => void validateWorkspace()}>
                  Validate
                </button>
                <button
                  type="button"
                  onClick={() => void trustWorkspace()}
                  disabled={
                    !session.agentWorkspace.canonicalPath ||
                    !session.agentWorkspace.isGitRepo ||
                    session.agentWorkspace.cleanWorktree !== true
                  }
                >
                  Trust
                </button>
              </div>
              <section className="agent-status-card">
                <span>Trust</span>
                <strong>{session.agentWorkspace.trusted ? "Trusted" : "Not trusted"}</strong>
              </section>
              <section className="agent-status-card">
                <span>Path</span>
                <strong>{session.agentWorkspace.canonicalPath || "No workspace selected"}</strong>
              </section>
              <section className="agent-status-card">
                <span>Branch</span>
                <strong>{session.agentWorkspace.gitBranch || "Unavailable"}</strong>
              </section>
              <section className="agent-status-card">
                <span>Git</span>
                <strong>
                  {session.agentWorkspace.isGitRepo
                    ? session.agentWorkspace.cleanWorktree
                      ? "Clean"
                      : "Dirty"
                    : "Not a git repository"}
                </strong>
              </section>
              <section className="agent-status-card">
                <span>Status</span>
                <strong>{session.agentWorkspace.message}</strong>
              </section>
              <section className="agent-file-tree">
                <h3>Sessions</h3>
                {agentSessions.length === 0 ? (
                  <p className="empty-state">No backend sessions yet.</p>
                ) : (
                  <div className="agent-session-list">
                    {agentSessions.map((agentSession) => (
                      <button
                        type="button"
                        className={
                          agentSession.id === selectedAgentSessionId
                            ? "agent-session-button is-active"
                            : "agent-session-button"
                        }
                        key={agentSession.id}
                        onClick={() => setSelectedAgentSessionId(agentSession.id)}
                      >
                        <strong>{agentSession.title}</strong>
                        <span>{new Date(agentSession.updatedAt).toLocaleString()}</span>
                      </button>
                    ))}
                  </div>
                )}
              </section>
            </div>
          </>
        )}
      </aside>

      <div className="splitter" onMouseDown={(event) => startResize("left", event.clientX)} />

      <main className="pane pane-center" style={{ width: `${session.paneWidths.center}%` }}>
        {session.appMode === "chat" ? (
          <div className="toolbar">
            <button type="button" onClick={resetConversation}>
              Clear context
            </button>
            {contextWarning ? <span className="status-warning">{contextWarning}</span> : null}
            {status ? <span className="status-note">{status}</span> : null}
          </div>
        ) : (
          <div className="toolbar agent-toolbar">
            <div>
              <strong>Agent Mode</strong>
              <span className="status-note">
                {session.agentWorkspace.trusted
                  ? `Trusted workspace: ${session.agentWorkspace.gitBranch || "current branch"}`
                  : session.agentWorkspace.message}
              </span>
            </div>
            <button type="button" className="secondary-button" onClick={() => void validateWorkspace()}>
              Validate workspace
            </button>
          </div>
        )}

        {session.appMode === "chat" ? (
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
                      {normalizeMathDelimiters(message.content)}
                    </ReactMarkdown>
                  );
                })()
              ) : (
                <p>{message.content}</p>
              )}
            </article>
          ))}

          {isGenerating ? (
            <article className="message thinking-tile" aria-live="polite">
              <div className="message-meta">
                <span>Thinking</span>
                <span>{formatElapsedSeconds(generationElapsedSeconds)}</span>
              </div>
              <div className="thinking-content">
                <div className="thinking-pulse" aria-hidden="true">
                  <span />
                  <span />
                  <span />
                </div>
                <div>
                  <p>{generationStatus || "Preparing request..."}</p>
                  <span>
                    {streamingContent
                      ? `${streamingContent.length.toLocaleString()} characters generated so far.`
                      : "Waiting for the model to begin streaming tokens."}
                  </span>
                </div>
              </div>
            </article>
          ) : null}

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
                {normalizeMathDelimiters(streamingContent)}
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
        ) : (
          <div className="pane-content agent-transcript">
            {error ? (
              <div className="error-banner">
                <span>{error}</span>
              </div>
            ) : null}
            <article className="agent-event">
              <div className="message-meta">
                <span>Session</span>
                <span>{session.agentWorkspace.trusted ? "Trusted" : "Blocked"}</span>
              </div>
              <p>
                {selectedAgentSession
                  ? `${selectedAgentSession.title} on ${selectedAgentSession.gitBranch || "current branch"}`
                  : session.agentWorkspace.trusted
                    ? "No agent session selected."
                    : "Trust a clean git workspace before starting an agent run."}
              </p>
            </article>
            {agentSessionStatus ? (
              <article className="agent-event">
                <div className="message-meta">
                  <span>Status</span>
                </div>
                <p>{agentSessionStatus}</p>
              </article>
            ) : null}
            {agentEvents.length === 0 ? (
              <article className="agent-event">
                <div className="message-meta">
                  <span>Plan</span>
                  <span>Pending</span>
                </div>
                <p>Plan events will appear here after workspace trust and the agent loop are enabled.</p>
              </article>
            ) : (
              agentEvents.map((event) => (
                <article className="agent-event" key={event.id}>
                  <div className="message-meta">
                    <span>{formatAgentEventType(event.type)}</span>
                    <span>{new Date(event.timestamp).toLocaleString()}</span>
                  </div>
                  <pre className="agent-event-payload">{JSON.stringify(event.payload, null, 2)}</pre>
                </article>
              ))
            )}
          </div>
        )}

        {session.appMode === "chat" ? (
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
        ) : (
          <div className="composer agent-composer">
            <textarea placeholder="Describe an agent task." disabled />
            <div className="composer-actions">
              <div className="selection-summary">
                <span>{session.selectedModel || "No model selected"}</span>
                <span>{session.agentWorkspace.canonicalPath || "No workspace selected"}</span>
              </div>
              <button type="button" disabled>
                Run agent
              </button>
            </div>
          </div>
        )}
      </main>

      <div className="splitter" onMouseDown={(event) => startResize("right", event.clientX)} />

      <aside className="pane pane-right" style={{ width: `${session.paneWidths.right}%` }}>
        <div className="pane-header">
          <h2>{session.appMode === "chat" ? "Settings" : "Agent Console"}</h2>
        </div>
        {session.appMode === "chat" ? (
          <div className="pane-content scrollable settings-stack">
          <section className="settings-card display-preferences-card">
            <h3>Display Preferences</h3>
            <label className="field">
              <span>Color theme</span>
              <small>Choose the interface contrast mode used for this browser session.</small>
              <select
                value={session.displayPreferences.theme}
                onChange={(event) =>
                  updateDisplayPreferences({ theme: event.target.value as DisplayTheme })
                }
              >
                <option value="dark">Dark</option>
                <option value="light">Light</option>
              </select>
            </label>
            <label className="field">
              <span>Font size</span>
              <small>Scale chat, controls, and settings text without changing your content.</small>
              <select
                value={session.displayPreferences.fontSize}
                onChange={(event) =>
                  updateDisplayPreferences({ fontSize: event.target.value as DisplayFontSize })
                }
              >
                <option value="small">Small</option>
                <option value="medium">Medium</option>
                <option value="large">Large</option>
              </select>
            </label>
          </section>

          <section className="settings-card">
            <h3>Model Settings</h3>
            <label className="field">
              <span>Model</span>
              <small>{modelStatus || "Models are loaded from the local Ollama endpoint."}</small>
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
            <button type="button" className="secondary-button" onClick={() => void loadModelOptions()}>
              Refresh models
            </button>

            <label className="field">
              <span>System prompt profile</span>
              <small>Select a saved system prompt profile. Built-in profiles can be edited locally and reset.</small>
              <select
                value={session.selectedSystemPromptProfileId}
                onChange={(event) => updateSelectedSystemPromptProfile(event.target.value)}
              >
                {systemPromptProfiles.length === 0 ? <option value="generic">Generic</option> : null}
                {systemPromptProfiles.map((profile) => (
                  <option key={profile.id} value={profile.id}>
                    {profile.name}
                    {profile.isBuiltIn ? "" : " (custom)"}
                  </option>
                ))}
              </select>
            </label>

            {selectedSystemPromptProfile && !selectedSystemPromptProfile.isBuiltIn ? (
              <label className="field compact-field">
                <span>Custom profile name</span>
                <input
                  value={selectedSystemPromptProfile.name}
                  onChange={(event) => renameSelectedSystemPromptProfile(event.target.value)}
                />
              </label>
            ) : null}

            <label className="field">
              <span>System prompt</span>
              <small>
                This text is prepended to every chat request before the conversation, selected files, and your current
                prompt.
              </small>
              <textarea
                rows={18}
                value={session.systemPrompt}
                onChange={(event) => updateSystemPrompt(event.target.value)}
              />
            </label>

            <div className="settings-button-row">
              <button type="button" className="secondary-button" onClick={createCustomSystemPromptProfile}>
                New custom profile
              </button>
              <button type="button" className="secondary-button" onClick={resetSelectedSystemPromptProfile}>
                Reset profile
              </button>
            </div>

            {selectedSystemPromptProfile && !selectedSystemPromptProfile.isBuiltIn ? (
              <button type="button" className="danger-button" onClick={deleteSelectedSystemPromptProfile}>
                Delete custom profile
              </button>
            ) : null}
          </section>
          </div>
        ) : (
          <div className="pane-content scrollable settings-stack">
            <section className="settings-card">
              <h3>Session</h3>
              {selectedAgentSession ? (
                <>
                  <label className="field compact-field">
                    <span>Name</span>
                    <input
                      value={selectedAgentSession.title}
                      onChange={(event) => {
                        const title = event.target.value;
                        setAgentSessions((current) =>
                          current.map((agentSession) =>
                            agentSession.id === selectedAgentSession.id
                              ? { ...agentSession, title }
                              : agentSession
                          )
                        );
                      }}
                      onBlur={(event) => void renameAgentSession(event.target.value)}
                    />
                  </label>
                  <p className="session-meta">
                    {selectedAgentSession.workspacePath}
                    <br />
                    Updated {new Date(selectedAgentSession.updatedAt).toLocaleString()}
                  </p>
                </>
              ) : (
                <p className="empty-state">Trust a workspace to create a session.</p>
              )}
            </section>
            <section className="settings-card">
              <h3>Approvals</h3>
              <p className="empty-state">No approvals pending.</p>
            </section>
            <section className="settings-card">
              <h3>Command Log</h3>
              <p className="empty-state">No command output yet.</p>
            </section>
            <section className="settings-card">
              <h3>Tools</h3>
              {agentTools.length === 0 ? (
                <p className="empty-state">No tools loaded.</p>
              ) : (
                <div className="agent-tool-list">
                  {agentTools.map((tool) => (
                    <article className="agent-tool-row" key={tool.name}>
                      <strong>{formatAgentEventType(tool.name)}</strong>
                      <span>{tool.description}</span>
                    </article>
                  ))}
                </div>
              )}
            </section>
            <section className="settings-card">
              <h3>Diff</h3>
              <p className="empty-state">No file changes yet.</p>
            </section>
          </div>
        )}
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
                  prompt.
                </p>
                <p>
                  Use <strong>Display Preferences</strong> to switch between dark and light themes or scale the
                  interface font size. These preferences are stored locally in the browser with the rest of your
                  session.
                </p>
                <p>
                  System prompt profiles let you switch quickly between common modes such as generic assistance,
                  software planning, software development, teaching, writing, brainstorming, and project planning.
                  Built-in profiles are available immediately and are stored locally in your browser. You can also
                  create custom profiles, name them, edit their prompts, and delete them later. Use
                  <strong> Reset profile</strong> to restore a built-in profile to its default prompt.
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
                  Sabio is distributed under the terms of the MIT License. See the repository LICENSE file for the full
                  license text, including permissions, conditions, and warranty disclaimer.
                </p>
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
