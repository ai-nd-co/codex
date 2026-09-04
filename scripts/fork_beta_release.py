#!/usr/bin/env python3
"""Validate and describe an ai-nd-co Codex beta release."""

from __future__ import annotations

import argparse
import json
import re


BETA_VERSION_RE = re.compile(r"^(?P<base>0\.1\.0)-beta\.(?P<number>[1-9][0-9]*)$")
SUPPORTED_TARGETS = (
    "x86_64-unknown-linux-musl",
    "x86_64-pc-windows-msvc",
)
PLATFORM_TAGS = {
    "x86_64-unknown-linux-musl": "beta-linux-x64",
    "x86_64-pc-windows-msvc": "beta-win32-x64",
}


def release_settings(version: str) -> dict[str, object]:
    if BETA_VERSION_RE.fullmatch(version) is None:
        raise ValueError(
            f"unsupported beta version {version!r}; expected 0.1.0-beta.N with N >= 1"
        )
    return {
        "version": version,
        "tag": f"rust-v{version}",
        "npm_tag": "beta",
        "prerelease": True,
        "targets": list(SUPPORTED_TARGETS),
        "platform_tags": dict(PLATFORM_TAGS),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("--github-output", action="store_true")
    args = parser.parse_args()
    settings = release_settings(args.version)
    if args.github_output:
        print(f"version={settings['version']}")
        print(f"npm_tag={settings['npm_tag']}")
        print("prerelease=true")
    else:
        print(json.dumps(settings, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
