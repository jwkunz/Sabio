import { openDB } from "idb";
import { BUILT_IN_SYSTEM_PROMPT_PROFILES } from "../../../shared/prompt";
import type { Message, SessionState, SystemPromptProfile, UploadedFile } from "../types/app";

const DB_NAME = "sabio-db";
const DB_VERSION = 2;
const SESSION_KEY = "session";

const defaultSession: SessionState = {
  selectedModel: "",
  systemPrompt: "",
  selectedSystemPromptProfileId: "generic",
  draftInput: "",
  paneWidths: {
    left: 22,
    center: 52,
    right: 26
  },
  displayPreferences: {
    theme: "dark",
    fontSize: "medium"
  }
};

const dbPromise = openDB(DB_NAME, DB_VERSION, {
  upgrade(db) {
    if (!db.objectStoreNames.contains("messages")) {
      db.createObjectStore("messages", { keyPath: "id" });
    }

    if (!db.objectStoreNames.contains("files")) {
      db.createObjectStore("files", { keyPath: "id" });
    }

    if (!db.objectStoreNames.contains("session")) {
      db.createObjectStore("session");
    }

    if (!db.objectStoreNames.contains("systemPromptProfiles")) {
      db.createObjectStore("systemPromptProfiles", { keyPath: "id" });
    }
  }
});

export const loadSession = async () => {
  const db = await dbPromise;
  const stored = (await db.get("session", SESSION_KEY)) as Partial<SessionState> | undefined;

  return {
    ...defaultSession,
    ...stored,
    paneWidths: stored?.paneWidths ?? defaultSession.paneWidths,
    displayPreferences: stored?.displayPreferences ?? defaultSession.displayPreferences,
    selectedSystemPromptProfileId: stored?.selectedSystemPromptProfileId ?? "generic"
  };
};

export const saveSession = async (session: SessionState) => {
  const db = await dbPromise;
  await db.put("session", session, SESSION_KEY);
};

export const loadMessages = async () => {
  const db = await dbPromise;
  return ((await db.getAll("messages")) as Message[]).sort((a, b) => a.createdAt - b.createdAt);
};

export const saveMessages = async (messages: Message[]) => {
  const db = await dbPromise;
  const tx = db.transaction("messages", "readwrite");
  await tx.store.clear();

  for (const message of messages) {
    await tx.store.put(message);
  }

  await tx.done;
};

export const loadFiles = async () => {
  const db = await dbPromise;
  return ((await db.getAll("files")) as UploadedFile[]).sort((a, b) => a.uploadedAt - b.uploadedAt);
};

export const saveFiles = async (files: UploadedFile[]) => {
  const db = await dbPromise;
  const tx = db.transaction("files", "readwrite");
  await tx.store.clear();

  for (const file of files) {
    await tx.store.put(file);
  }

  await tx.done;
};

export const clearMessages = async () => {
  const db = await dbPromise;
  await db.clear("messages");
};

export const builtInSystemPromptProfiles = (): SystemPromptProfile[] =>
  BUILT_IN_SYSTEM_PROMPT_PROFILES.map((profile) => ({
    ...profile,
    isBuiltIn: true,
    updatedAt: 0
  }));

export const loadSystemPromptProfiles = async () => {
  const db = await dbPromise;
  const stored = (await db.getAll("systemPromptProfiles")) as SystemPromptProfile[];
  const byId = new Map<string, SystemPromptProfile>();

  for (const profile of builtInSystemPromptProfiles()) {
    byId.set(profile.id, profile);
  }

  for (const profile of stored) {
    byId.set(profile.id, {
      ...profile,
      isBuiltIn: profile.isBuiltIn ?? false
    });
  }

  const profiles = Array.from(byId.values()).sort((a, b) => {
    if (a.isBuiltIn !== b.isBuiltIn) {
      return a.isBuiltIn ? -1 : 1;
    }

    return a.name.localeCompare(b.name);
  });

  await saveSystemPromptProfiles(profiles);
  return profiles;
};

export const saveSystemPromptProfiles = async (profiles: SystemPromptProfile[]) => {
  const db = await dbPromise;
  const tx = db.transaction("systemPromptProfiles", "readwrite");
  await tx.store.clear();

  for (const profile of profiles) {
    await tx.store.put(profile);
  }

  await tx.done;
};
