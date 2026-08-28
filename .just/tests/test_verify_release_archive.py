"""ATM-owned manifest-data coverage for the retained archive verifier."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "verify_release_archive.py"
SPEC = importlib.util.spec_from_file_location("verify_release_archive", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class VerifyReleaseArchiveTests(unittest.TestCase):
    def test_expected_members_uses_manifest_bundled_paths_for_directory_and_file(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            manifest_path = root / "release" / "publish-artifacts.toml"
            manifest_path.parent.mkdir()
            docs = root / "docs"
            docs.mkdir()
            (docs / "README.md").write_text("docs\n", encoding="utf-8")
            nested = docs / "guides"
            nested.mkdir()
            (nested / "setup.md").write_text("setup\n", encoding="utf-8")
            notice = root / "NOTICE"
            notice.write_text("notice\n", encoding="utf-8")
            manifest_path.write_text(
                textwrap.dedent(
                    """
                    [[release_binaries]]
                    name = "atm"
                    bundled_paths = [
                      { source = "docs", destination = "share/doc/atm" },
                      { source = "NOTICE", destination = "share/licenses/NOTICE" },
                    ]
                    """
                ).strip()
                + "\n",
                encoding="utf-8",
            )

            self.assertEqual(
                MODULE.expected_members(manifest_path, windows=False),
                {
                    "bin/atm",
                    "share/doc/atm/README.md",
                    "share/doc/atm/guides/setup.md",
                    "share/licenses/NOTICE",
                },
            )
            self.assertIn("bin/atm.exe", MODULE.expected_members(manifest_path, windows=True))

    def test_expected_members_fails_closed_for_missing_bundle_source(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            manifest_path = root / "release" / "publish-artifacts.toml"
            manifest_path.parent.mkdir()
            manifest_path.write_text(
                "[[release_binaries]]\nname = \"atm\"\nbundled_paths = [{ source = \"missing\", destination = \"share/missing\"}]\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(SystemExit, "bundled path source is missing"):
                MODULE.expected_members(manifest_path, windows=False)


if __name__ == "__main__":
    unittest.main()
