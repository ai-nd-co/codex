#!/usr/bin/env python3
"""Compute the fork-only outputs needed by the upstream release workflow."""

from __future__ import annotations

import argparse
import re


BETA_VERSION_RE = re.compile(r"^0\.1\.0-beta\.[1-9][0-9]*$")


def release_settings(version: str) -> dict[str, str]:
    fork_beta = BETA_VERSION_RE.fullmatch(version) is not None
    return {
        "version": version,
        "npm_tag": "beta" if fork_beta else "",
        "fork_beta": str(fork_beta).lower(),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    args = parser.parse_args()
    settings = release_settings(args.version)
    for key, value in settings.items():
        print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
