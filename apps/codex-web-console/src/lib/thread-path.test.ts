import { describe, expect, it } from "vitest";
import { normalizeThreadPath, projectThreadPathsMatch } from "./thread-path";

describe("normalizeThreadPath", () => {
  it("preserves case on case-sensitive filesystems", () => {
    expect(normalizeThreadPath("/Work/App/", false)).toBe("/Work/App");
    expect(normalizeThreadPath("/work/app/", false)).toBe("/work/app");
  });

  it("normalizes separators and folds case on case-insensitive filesystems", () => {
    expect(normalizeThreadPath("C:\\Work\\App\\", true)).toBe("c:/work/app");
  });
});

describe("projectThreadPathsMatch", () => {
  it("treats differently-cased POSIX paths as distinct", () => {
    expect(projectThreadPathsMatch("/Work/App", "/work/app", false)).toBe(false);
  });

  it("matches Windows-style paths case-insensitively", () => {
    expect(projectThreadPathsMatch("C:\\Work\\App", "c:/work/app/", true)).toBe(true);
  });
});
