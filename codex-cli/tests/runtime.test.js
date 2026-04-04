import assert from "node:assert/strict";
import { mkdir, mkdtemp, stat, utimes, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  DEFAULT_STALE_RUNTIME_AGE_MS,
  cleanupRuntimeDir,
  determineTargetTriple,
  prepareCodexRuntime,
  pruneStaleRuntimeDirs,
} from "../bin/runtime.js";

async function createFakePackageRoot(t, targetTriple, binaryName) {
  const packageRoot = await mkdtemp(path.join(os.tmpdir(), "codex-cli-package-"));
  t.after(async () => {
    await cleanupRuntimeDir(packageRoot);
  });

  const codexDir = path.join(packageRoot, "vendor", targetTriple, "codex");
  const pathDir = path.join(packageRoot, "vendor", targetTriple, "path");
  await mkdir(codexDir, { recursive: true });
  await mkdir(pathDir, { recursive: true });
  await writeFile(path.join(codexDir, binaryName), "binary");
  await writeFile(path.join(codexDir, "codex-command-runner.exe"), "runner");
  await writeFile(path.join(pathDir, "rg.exe"), "rg");
  return packageRoot;
}

test("determineTargetTriple maps supported platforms", () => {
  assert.equal(determineTargetTriple("win32", "x64"), "x86_64-pc-windows-msvc");
  assert.equal(determineTargetTriple("darwin", "arm64"), "aarch64-apple-darwin");
  assert.equal(determineTargetTriple("linux", "x64"), "x86_64-unknown-linux-gnu");
  assert.equal(determineTargetTriple("win32", "ia32"), null);
});

test("prepareCodexRuntime keeps in-place runtime on non-windows", async (t) => {
  const targetTriple = "x86_64-unknown-linux-gnu";
  const packageRoot = await createFakePackageRoot(t, targetTriple, "codex");

  const runtime = await prepareCodexRuntime({
    packageRoot,
    platform: "linux",
    arch: "x64",
  });

  assert.equal(runtime.targetTriple, targetTriple);
  assert.equal(runtime.runtimeRoot, path.join(packageRoot, "vendor", targetTriple));
  assert.equal(
    runtime.binaryPath,
    path.join(packageRoot, "vendor", targetTriple, "codex", "codex"),
  );
  assert.deepEqual(runtime.additionalPathDirs, [
    path.join(packageRoot, "vendor", targetTriple, "path"),
  ]);

  await runtime.cleanup();
  assert.ok((await stat(runtime.runtimeRoot)).isDirectory());
});

test("prepareCodexRuntime stages a temp runtime copy on windows", async (t) => {
  const targetTriple = "x86_64-pc-windows-msvc";
  const packageRoot = await createFakePackageRoot(t, targetTriple, "codex.exe");
  const tempRoot = await mkdtemp(path.join(os.tmpdir(), "codex-cli-runtime-root-"));
  t.after(async () => {
    await cleanupRuntimeDir(tempRoot);
  });

  const runtime = await prepareCodexRuntime({
    packageRoot,
    platform: "win32",
    arch: "x64",
    tempRoot,
  });

  assert.notEqual(runtime.runtimeRoot, path.join(packageRoot, "vendor", targetTriple));
  assert.ok(runtime.runtimeRoot.startsWith(path.join(tempRoot, targetTriple)));
  assert.equal(
    runtime.binaryPath,
    path.join(runtime.runtimeRoot, "codex", "codex.exe"),
  );
  assert.deepEqual(runtime.additionalPathDirs, [
    path.join(runtime.runtimeRoot, "path"),
  ]);
  assert.ok((await stat(runtime.binaryPath)).isFile());
  assert.ok((await stat(path.join(runtime.runtimeRoot, "path", "rg.exe"))).isFile());
  assert.ok(
    (await stat(path.join(runtime.runtimeRoot, "codex", "codex-command-runner.exe"))).isFile(),
  );

  const stagedContainer = path.dirname(runtime.runtimeRoot);
  await runtime.cleanup();
  await assert.rejects(stat(stagedContainer));
});

test("pruneStaleRuntimeDirs removes only stale run directories", async (t) => {
  const tempRoot = await mkdtemp(path.join(os.tmpdir(), "codex-cli-prune-root-"));
  t.after(async () => {
    await cleanupRuntimeDir(tempRoot);
  });

  const staleDir = path.join(tempRoot, "run-stale");
  const freshDir = path.join(tempRoot, "run-fresh");
  const unrelatedDir = path.join(tempRoot, "keep-me");
  await mkdir(staleDir, { recursive: true });
  await mkdir(freshDir, { recursive: true });
  await mkdir(unrelatedDir, { recursive: true });

  const staleDate = new Date(Date.now() - DEFAULT_STALE_RUNTIME_AGE_MS - 60_000);
  await utimes(staleDir, staleDate, staleDate);

  await pruneStaleRuntimeDirs(tempRoot, { staleAgeMs: DEFAULT_STALE_RUNTIME_AGE_MS });

  await assert.rejects(stat(staleDir));
  assert.ok((await stat(freshDir)).isDirectory());
  assert.ok((await stat(unrelatedDir)).isDirectory());
});

test("cleanupRuntimeDir ignores expected removal failures", async () => {
  let attempts = 0;
  await cleanupRuntimeDir("C:/fake/runtime", {
    rmImpl: async () => {
      attempts += 1;
      const error = new Error("busy");
      error.code = "EBUSY";
      throw error;
    },
  });
  assert.equal(attempts, 1);
});
