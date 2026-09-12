from __future__ import annotations

import importlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
MODULE = importlib.import_module("scripts.smoke.report_runtime")


class ReportRuntimeTests(unittest.TestCase):
    def test_source_revision_returns_full_git_object_id_from_real_checkout(self) -> None:
        revision = MODULE.source_revision(Path(__file__).resolve().parents[2])
        self.assertRegex(revision or "", r"^[0-9a-f]{40}$")

    def test_source_revision_returns_none_for_invalid_git_output(self) -> None:
        with mock.patch.object(MODULE.subprocess, "run", return_value=mock.Mock(returncode=0, stdout="not-a-revision\n")):
            self.assertIsNone(MODULE.source_revision(Path("/tmp")))

    def test_compose_serializes_variables_and_removes_tempfile(self) -> None:
        observed: dict[str, object] = {}

        def render(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            variables = Path(command[command.index("--var-file") + 1])
            observed["variables_path"] = variables
            observed["variables"] = json.loads(variables.read_text(encoding="utf-8"))
            observed["exists_during_call"] = variables.exists()
            return subprocess.CompletedProcess(command, 0, "", "")

        with tempfile.TemporaryDirectory() as tempdir, mock.patch.object(MODULE.subprocess, "run", side_effect=render):
            output = Path(tempdir) / "nested" / "report.html"
            MODULE.compose(Path(tempdir) / "template.j2", {"body_html": "<p>ok</p>"}, output, root=Path(tempdir))
            self.assertTrue(observed["exists_during_call"])
            self.assertEqual(observed["variables"], {"body_html": "<p>ok</p>"})
            self.assertFalse(Path(observed["variables_path"]).exists())

    def test_compose_uses_caller_error_type_on_render_failure(self) -> None:
        completed = subprocess.CompletedProcess(["sc-compose"], 1, "", "render failed")
        with tempfile.TemporaryDirectory() as tempdir, mock.patch.object(MODULE.subprocess, "run", return_value=completed):
            with self.assertRaisesRegex(ValueError, "sc-compose render failed: render failed"):
                MODULE.compose(Path(tempdir) / "template.j2", {}, Path(tempdir) / "report.html", root=Path(tempdir), error_type=ValueError)


if __name__ == "__main__":
    unittest.main()
