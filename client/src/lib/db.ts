import { openDB } from "idb";
import type { Message, SessionState, UploadedFile } from "../types/app";

const DB_NAME = "sabio-db";
const DB_VERSION = 1;
const SESSION_KEY = "session";

const defaultSession: SessionState = {
  selectedModel: "",
  systemPrompt: "",
  draftInput: "",
  paneWidths: {
    left: 22,
    center: 52,
    right: 26
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
  }
});

export const loadSession = async () => {
  const db = await dbPromise;
  return ((await db.get("session", SESSION_KEY)) as SessionState | undefined) ?? defaultSession;
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
