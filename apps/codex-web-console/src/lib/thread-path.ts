export function normalizeThreadPath(
  path: string,
  caseInsensitive = process.platform === "win32",
): string {
  const normalized = path.replaceAll("\\", "/").replace(/\/+$/, "");
  return caseInsensitive ? normalized.toLowerCase() : normalized;
}

export function projectThreadPathsMatch(
  projectPath: string,
  threadCwd: string,
  caseInsensitive = process.platform === "win32",
): boolean {
  return (
    normalizeThreadPath(projectPath, caseInsensitive) ===
    normalizeThreadPath(threadCwd, caseInsensitive)
  );
}
