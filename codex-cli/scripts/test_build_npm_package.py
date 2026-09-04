import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


MODULE_PATH = Path(__file__).with_name("build_npm_package.py")
SPEC = importlib.util.spec_from_file_location("build_npm_package", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PackageMetadataTest(unittest.TestCase):
    def test_root_uses_selected_fork_platform_dependencies(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            stage = Path(temp_dir)
            MODULE.stage_sources(
                stage,
                "0.1.0-beta.1",
                "codex",
                ["codex-linux-x64", "codex-win32-x64"],
            )
            manifest = json.loads((stage / "package.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["name"], "@ai-nd-co/codex")
        self.assertEqual(
            manifest["optionalDependencies"],
            {
                "@ai-nd-co/codex-linux-x64": "0.1.0-beta.1",
                "@ai-nd-co/codex-win32-x64": "0.1.0-beta.1",
            },
        )

    def test_platform_package_has_distinct_public_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            stage = Path(temp_dir)
            MODULE.stage_sources(stage, "0.1.0-beta.1", "codex-win32-x64", [])
            manifest = json.loads((stage / "package.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["name"], "@ai-nd-co/codex-win32-x64")
        self.assertEqual(manifest["version"], "0.1.0-beta.1")
        self.assertEqual(manifest["publishConfig"], {"access": "public"})


if __name__ == "__main__":
    unittest.main()
