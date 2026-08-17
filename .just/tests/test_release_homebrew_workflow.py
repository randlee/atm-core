from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
PUBLISHER = REPO_ROOT / ".claude" / "agents" / "publisher.md"


class ReleaseWorkflowTests(unittest.TestCase):
    def test_root_release_workflow_has_no_destination_specific_publishing(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertNotIn("homebrew-tap", text)
        self.assertNotIn("winget-releaser", text)
        self.assertIn("release-target-matrix", text)
        self.assertIn("channel-dispatch-plan", PUBLISHER.read_text(encoding="utf-8"))

    def test_python_build_matrix_supports_each_manifest_declared_build_system(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("matrix.build_system == 'maturin'", text)
        self.assertIn("python -m build --wheel", text)
        self.assertIn("python -m build --sdist", text)

    def test_release_archives_are_verified_against_the_manifest_before_upload(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("scripts/verify_release_archive.py", text)
        self.assertIn('"--archive",', text)


if __name__ == "__main__":
    unittest.main()
