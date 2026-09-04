import importlib.util
from pathlib import Path
import unittest


MODULE_PATH = Path(__file__).with_name("fork_beta_release.py")
SPEC = importlib.util.spec_from_file_location("fork_beta_release", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
release_settings = MODULE.release_settings


class ReleaseSettingsTest(unittest.TestCase):
    def test_numbered_beta_maps_to_beta_tags(self) -> None:
        settings = release_settings("0.1.0-beta.7")
        self.assertEqual(settings["npm_tag"], "beta")
        self.assertEqual(settings["fork_beta"], "true")

    def test_other_versions_leave_upstream_path_selected(self) -> None:
        for version in ("0.1.0", "0.1.0-alpha.1", "0.1.0-beta.0", "1.0.0-beta.1"):
            with self.subTest(version=version):
                self.assertEqual(release_settings(version)["fork_beta"], "false")


if __name__ == "__main__":
    unittest.main()
