from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_version_sync import replace_version_occurrences
from prerelease_tag import copy_tracked_files
from prerelease_tag import sync_python_version


class CheckVersionSyncTests(unittest.TestCase):
    def test_replace_version_occurrences_rejects_adjacent_version_component(self) -> None:
        self.assertEqual(
            replace_version_occurrences(
                "https://example.invalid/atm_1.1.2.0_x86_64.zip", "1.1.2", "1.1.3"
            ),
            "https://example.invalid/atm_1.1.2.0_x86_64.zip",
        )

    @mock.patch("prerelease_tag.subprocess.run")
    def test_sync_python_version_propagates_owner_failure(self, run: mock.Mock) -> None:
        run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="owner failed"
        )
        with self.assertRaisesRegex(SystemExit, "owner failed"):
            sync_python_version(Path("/tmp/atm"), Path("/tmp/atm/crates/example/pyproject.toml"))

    @unittest.skipUnless(hasattr(os, "symlink"), "symlink support is required")
    def test_copy_tracked_files_excludes_untracked_content_and_preserves_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            source = Path(tempdir) / "source"
            destination = Path(tempdir) / "destination"
            source.mkdir()
            (source / "tracked.txt").write_text("tracked", encoding="utf-8")
            (source / "target.txt").write_text("target", encoding="utf-8")
            (source / "link.txt").symlink_to("target.txt")
            (source / "secret.txt").write_text("untracked", encoding="utf-8")
            subprocess.run(["git", "init", "-q"], cwd=source, check=True)
            subprocess.run(["git", "add", "tracked.txt", "target.txt", "link.txt"], cwd=source, check=True)
            copy_tracked_files(source, destination)
            self.assertTrue((destination / "tracked.txt").is_file())
            self.assertFalse((destination / "secret.txt").exists())
            self.assertTrue((destination / "link.txt").is_symlink())
            self.assertEqual(os.readlink(destination / "link.txt"), "target.txt")



if __name__ == "__main__":
    unittest.main()
