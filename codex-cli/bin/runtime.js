import { existsSync } from "node:fs";
import {
  copyFile,
  lstat,
  mkdir,
  mkdtemp,
  readdir,
  readlink,
  rm,
  stat,
  symlink,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";

export const DEFAULT_STALE_RUNTIME_AGE_MS = 24 * 60 * 60 * 1000;
const RUN_DIR_PREFIX = "run-";

export function determineTargetTriple(platform, arch) {
  switch (platform) {
    case "linux":
    case "android":
      switch (arch) {
        case "x64":
          return "x86_64-unknown-linux-gnu";
        case "arm64":
          return "aarch64-unknown-linux-gnu";
        default:
          return null;
      }
    case "darwin":
      switch (arch) {
        case "x64":
          return "x86_64-apple-darwin";
        case "arm64":
          return "aarch64-apple-darwin";
        default:
          return null;
      }
    case "win32":
      switch (arch) {
        case "x64":
          return "x86_64-pc-windows-msvc";
        case "arm64":
          return "aarch64-pc-windows-msvc";
        default:
          return null;
      }
    default:
      return null;
  }
}

export function defaultWindowsRuntimeRoot() {
  return path.join(os.tmpdir(), "codex-npm-runtime");
}

export async function cleanupRuntimeDir(runtimeDir, options = {}) {
  if (!runtimeDir) {
    return;
  }

  const rmImpl = options.rmImpl ?? rm;

  try {
    await rmImpl(runtimeDir, {
      recursive: true,
      force: true,
      maxRetries: 3,
      retryDelay: 50,
    });
  } catch (error) {
    if (!isIgnorableCleanupError(error)) {
      throw error;
    }
  }
}

export async function pruneStaleRuntimeDirs(tempRoot, options = {}) {
  const readdirImpl = options.readdirImpl ?? readdir;
  const statImpl = options.statImpl ?? stat;
  const rmImpl = options.rmImpl ?? rm;
  const staleAgeMs = options.staleAgeMs ?? DEFAULT_STALE_RUNTIME_AGE_MS;
  const nowMs = options.nowMs ?? Date.now();

  let entries;
  try {
    entries = await readdirImpl(tempRoot, { withFileTypes: true });
  } catch (error) {
    if (error?.code === "ENOENT") {
      return;
    }
    throw error;
  }

  for (const entry of entries) {
    if (!entry.isDirectory() || !entry.name.startsWith(RUN_DIR_PREFIX)) {
      continue;
    }

    const candidatePath = path.join(tempRoot, entry.name);
    let candidateStat;
    try {
      candidateStat = await statImpl(candidatePath);
    } catch (error) {
      if (error?.code === "ENOENT") {
        continue;
      }
      throw error;
    }

    if (nowMs - candidateStat.mtimeMs < staleAgeMs) {
      continue;
    }

    try {
      await rmImpl(candidatePath, {
        recursive: true,
        force: true,
        maxRetries: 3,
        retryDelay: 50,
      });
    } catch (error) {
      if (!isIgnorableCleanupError(error)) {
        throw error;
      }
    }
  }
}

export async function prepareCodexRuntime(options) {
  const packageRoot = options.packageRoot;
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const staleAgeMs = options.staleAgeMs ?? DEFAULT_STALE_RUNTIME_AGE_MS;
  const nowMs = options.nowMs ?? Date.now();

  const targetTriple = determineTargetTriple(platform, arch);
  if (!targetTriple) {
    throw new Error(`Unsupported platform: ${platform} (${arch})`);
  }

  const binaryName = platform === "win32" ? "codex.exe" : "codex";
  const archRoot = path.join(packageRoot, "vendor", targetTriple);
  const inPlacePathDir = path.join(archRoot, "path");

  if (platform !== "win32") {
    return {
      targetTriple,
      runtimeRoot: archRoot,
      binaryPath: path.join(archRoot, "codex", binaryName),
      additionalPathDirs: existsSync(inPlacePathDir) ? [inPlacePathDir] : [],
      cleanup: async () => {},
    };
  }

  const tempRootBase = options.tempRoot ?? defaultWindowsRuntimeRoot();
  const targetTempRoot = path.join(tempRootBase, targetTriple);
  await mkdir(targetTempRoot, { recursive: true });
  await pruneStaleRuntimeDirs(targetTempRoot, {
    staleAgeMs,
    nowMs,
    readdirImpl: options.readdirImpl,
    statImpl: options.statImpl,
    rmImpl: options.rmImpl,
  });

  const mkdtempImpl = options.mkdtempImpl ?? mkdtemp;
  const runtimeContainer = await mkdtempImpl(path.join(targetTempRoot, RUN_DIR_PREFIX));
  const runtimeRoot = path.join(runtimeContainer, targetTriple);
  await copyDirectoryRecursive(archRoot, runtimeRoot);

  const runtimePathDir = path.join(runtimeRoot, "path");
  return {
    targetTriple,
    runtimeRoot,
    binaryPath: path.join(runtimeRoot, "codex", binaryName),
    additionalPathDirs: existsSync(runtimePathDir) ? [runtimePathDir] : [],
    cleanup: async () => cleanupRuntimeDir(runtimeContainer, { rmImpl: options.rmImpl }),
  };
}

async function copyDirectoryRecursive(sourceDir, destDir) {
  await mkdir(destDir, { recursive: true });

  const entries = await readdir(sourceDir, { withFileTypes: true });
  for (const entry of entries) {
    const sourcePath = path.join(sourceDir, entry.name);
    const destPath = path.join(destDir, entry.name);

    if (entry.isDirectory()) {
      await copyDirectoryRecursive(sourcePath, destPath);
      continue;
    }

    if (entry.isSymbolicLink()) {
      const linkTarget = await readlink(sourcePath);
      await symlink(linkTarget, destPath);
      continue;
    }

    if (entry.isFile()) {
      await copyFile(sourcePath, destPath);
      continue;
    }

    const sourceStat = await lstat(sourcePath);
    if (sourceStat.isDirectory()) {
      await copyDirectoryRecursive(sourcePath, destPath);
    } else if (sourceStat.isSymbolicLink()) {
      const linkTarget = await readlink(sourcePath);
      await symlink(linkTarget, destPath);
    } else {
      await copyFile(sourcePath, destPath);
    }
  }
}

function isIgnorableCleanupError(error) {
  return [
    "ENOENT",
    "EBUSY",
    "EPERM",
    "ENOTEMPTY",
    "EACCES",
  ].includes(error?.code);
}
