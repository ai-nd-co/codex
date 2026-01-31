#!/usr/bin/env python3
"""Update codex-rs/Cargo.toml workspace version in-place."""

from __future__ import annotations

import re
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: bump_rust_version.py <version>", file=sys.stderr)
        return 2

    version = sys.argv[1]
    cargo_toml = Path(__file__).resolve().parents[1] / "codex-rs" / "Cargo.toml"
    text = cargo_toml.read_text(encoding="utf-8")

    # Replace the first workspace/package version only (matches tag-check behavior).
    pattern = re.compile(r'^(version\s*=\s*")[^"]+(")', re.MULTILINE)
    match = pattern.search(text)
    if not match:
        print(f"version field not found in {cargo_toml}", file=sys.stderr)
        return 1

    updated = pattern.sub(rf'\1{version}\2', text, count=1)
    cargo_toml.write_text(updated, encoding="utf-8")
    print(f"Updated {cargo_toml} to version {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
