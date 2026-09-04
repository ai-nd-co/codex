import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


MODULE_PATH = Path(__file__).with_name("stage_npm_packages.py")
SPEC = importlib.util.spec_from_file_location("stage_npm_packages", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class TargetSelectionTest(unittest.TestCase):
    def test_selects_only_requested_artifacts(self) -> None:
        artifacts = [
            MODULE.WorkflowArtifact("x86_64-unknown-linux-musl", 1),
            MODULE.WorkflowArtifact("x86_64-pc-windows-msvc", 2),
            MODULE.WorkflowArtifact("aarch64-pc-windows-msvc", 3),
        ]
        with mock.patch.object(MODULE, "list_workflow_artifacts", return_value=artifacts):
            selected = MODULE.select_target_artifacts(
                "run-id",
                [MODULE.CODEX_PACKAGE_COMPONENT],
                ["x86_64-unknown-linux-musl", "x86_64-pc-windows-msvc"],
            )
        self.assertEqual(
            [artifact.name for artifact in selected],
            ["x86_64-unknown-linux-musl", "x86_64-pc-windows-msvc"],
        )

    def test_target_mapping_matches_platform_metadata(self) -> None:
        self.assertEqual(
            MODULE.TARGET_TO_PLATFORM_PACKAGE["x86_64-pc-windows-msvc"],
            "codex-win32-x64",
        )

    def test_codex_expansion_excludes_unselected_platforms(self) -> None:
        packages = MODULE.expand_packages(
            ["codex"],
            ["x86_64-unknown-linux-musl", "x86_64-pc-windows-msvc"],
        )
        self.assertEqual(
            packages,
            ["codex", "codex-linux-x64", "codex-win32-x64"],
        )

    def test_local_artifacts_do_not_query_or_download_workflow(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            with (
                mock.patch.object(MODULE, "install_from_local_artifacts") as local,
                mock.patch.object(MODULE, "install_from_workflow_artifacts") as remote,
            ):
                MODULE.install_native_components(
                    None,
                    {MODULE.CODEX_PACKAGE_COMPONENT},
                    root / "vendor-root",
                    root / "artifacts",
                    ["x86_64-pc-windows-msvc"],
                )

        local.assert_called_once()
        remote.assert_not_called()


if __name__ == "__main__":
    unittest.main()
