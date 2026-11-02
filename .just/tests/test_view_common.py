from __future__ import annotations

from pathlib import Path
import tempfile
import unittest


from sc_lint.view_common import relative_artifact_path
from sc_lint.view_common import reset_view_dir
from sc_lint.view_common import write_json
from sc_lint.view_common import write_text


class ViewCommonTests(unittest.TestCase):
    def test_reset_view_dir_recreates_target(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            existing = repo_root / "artifacts/view/modules"
            existing.mkdir(parents=True)
            (existing / "stale.txt").write_text("stale", encoding="utf-8")

            target = reset_view_dir(repo_root, "modules")

            self.assertEqual(target, repo_root / "artifacts/view/modules")
            self.assertTrue(target.is_dir())
            self.assertFalse((target / "stale.txt").exists())

    def test_write_text_and_json(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            text_path = repo_root / "artifacts/view/sample.txt"
            json_path = repo_root / "artifacts/view/sample.json"

            write_text(text_path, "hello\n")
            write_json(json_path, {"name": "atm"})

            self.assertEqual(text_path.read_text(encoding="utf-8"), "hello\n")
            self.assertIn('"name": "atm"', json_path.read_text(encoding="utf-8"))

    def test_relative_artifact_path_is_posix(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            path = repo_root / "artifacts/view/modules/index.txt"

            self.assertEqual(relative_artifact_path(repo_root, path), "artifacts/view/modules/index.txt")


if __name__ == "__main__":
    unittest.main()
