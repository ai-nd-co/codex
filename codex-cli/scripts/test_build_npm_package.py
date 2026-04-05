#!/usr/bin/env python3
"""Regression tests for npm package staging."""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).resolve().parent / "build_npm_package.py"
SPEC = importlib.util.spec_from_file_location("build_npm_package", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Unable to load module from {SCRIPT_PATH}")
BUILD_NPM_PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD_NPM_PACKAGE)


def list_relative_files(root: Path) -> set[str]:
    return {
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_file()
    }


class StageSourcesTest(unittest.TestCase):
    def test_stage_codex_copies_full_bin_directory(self) -> None:
        with tempfile.TemporaryDirectory(prefix="codex-stage-test-") as temp_dir:
            staging_dir = Path(temp_dir)

            BUILD_NPM_PACKAGE.stage_sources(staging_dir, "9.9.9", "codex")

            source_bin = BUILD_NPM_PACKAGE.CODEX_CLI_ROOT / "bin"
            staged_bin = staging_dir / "bin"

            self.assertEqual(list_relative_files(source_bin), list_relative_files(staged_bin))
            self.assertTrue((staged_bin / "runtime.js").exists())

            with open(staging_dir / "package.json", "r", encoding="utf-8") as fh:
                package_json = json.load(fh)
            self.assertEqual(package_json["version"], "9.9.9")


if __name__ == "__main__":
    unittest.main()
