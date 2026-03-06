import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { randomUUID } from "node:crypto";

export type ProjectId = string;
export type ThreadId = string;

export type Project = {
  id: ProjectId;
  path: string;
  createdAt: number;
};

export type ThreadRef = {
  id: ThreadId;
  projectId: ProjectId;
  createdAt: number;
  name?: string | null;
};

type StoreFile = {
  projects: Project[];
  threads: ThreadRef[];
};

const DEFAULT_STORE_PATH = path.join(
  os.homedir(),
  ".codex",
  "codex-web-console",
  "store.json",
);

function storePath(): string {
  return process.env.CODEX_WEB_STORE_PATH ?? DEFAULT_STORE_PATH;
}

async function loadStore(): Promise<StoreFile> {
  const p = storePath();
  try {
    const raw = await fs.readFile(p, "utf-8");
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") throw new Error("invalid store");
    const obj = parsed as Record<string, unknown>;
    const projects = Array.isArray(obj.projects) ? (obj.projects as Project[]) : [];
    const threads = Array.isArray(obj.threads) ? (obj.threads as ThreadRef[]) : [];
    return { projects, threads };
  } catch {
    return { projects: [], threads: [] };
  }
}

async function saveStore(store: StoreFile): Promise<void> {
  const p = storePath();
  await fs.mkdir(path.dirname(p), { recursive: true });
  const tmp = `${p}.tmp`;
  await fs.writeFile(tmp, JSON.stringify(store, null, 2), "utf-8");
  await fs.rename(tmp, p);
}

async function updateStore<T>(fn: (store: StoreFile) => T | Promise<T>): Promise<T> {
  const store = await loadStore();
  const out = await fn(store);
  await saveStore(store);
  return out;
}

export async function createProject(projectPath: string): Promise<Project> {
  return updateStore((store) => {
    const existing = store.projects.find((p) => p.path === projectPath);
    if (existing) return existing;
    const project: Project = {
      id: randomUUID(),
      path: projectPath,
      createdAt: Date.now(),
    };
    store.projects.push(project);
    return project;
  });
}

export async function listProjects(): Promise<Project[]> {
  const store = await loadStore();
  return store.projects.slice().sort((a, b) => b.createdAt - a.createdAt);
}

export async function getProject(id: ProjectId): Promise<Project | null> {
  const store = await loadStore();
  return store.projects.find((p) => p.id === id) ?? null;
}

export async function upsertThread(ref: ThreadRef): Promise<ThreadRef> {
  return updateStore((store) => {
    const idx = store.threads.findIndex((t) => t.id === ref.id);
    if (idx >= 0) {
      store.threads[idx] = { ...store.threads[idx], ...ref };
      return store.threads[idx];
    }
    store.threads.push(ref);
    return ref;
  });
}

export async function listThreadsForProject(projectId: ProjectId): Promise<ThreadRef[]> {
  const store = await loadStore();
  return store.threads
    .filter((t) => t.projectId === projectId)
    .slice()
    .sort((a, b) => b.createdAt - a.createdAt);
}

