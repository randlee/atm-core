"""Direct tests for shared report provenance and rendering plumbing."""
from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SMOKE_DIR = ROOT / "scripts/smoke"
if str(SMOKE_DIR) not in sys.path:
    sys.path.insert(0, str(SMOKE_DIR))

import report_runtime as MODULE  # noqa: E402


class FixtureError(RuntimeError):
    pass


def git(repository: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=repository,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


class ReportRuntimeTests(unittest.TestCase):
    def test_source_revision_and_procedure_page_use_real_git_ancestry(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            git(root, "init", "--quiet")
            git(root, "config", "user.email", "fixture@example.test")
            git(root, "config", "user.name", "Fixture")
            tracked = root / "runner.py"
            tracked.write_text("first\n", encoding="utf-8")
            git(root, "add", "runner.py")
            git(root, "commit", "--quiet", "-m", "first")
            procedure_revision = git(root, "rev-parse", "HEAD")
            tracked.write_text("second\n", encoding="utf-8")
            git(root, "commit", "--quiet", "-am", "second")
            source_revision = git(root, "rev-parse", "HEAD")

            procedure_html = "procedures/example/first.html"
            page = root / "site/reports" / procedure_html
            page.parent.mkdir(parents=True)
            page.write_text("<html>procedure</html>\n", encoding="utf-8")
            manifest = {
                "schema_version": 1,
                "procedures": [{
                    "procedure": "example",
                    "revisions": [{
                        "rev": procedure_revision,
                        "date": "2026-09-12",
                        "html": procedure_html,
                    }],
                }],
            }
            manifest_path = root / "site/reports/procedures/manifest.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

            self.assertEqual(MODULE.source_revision(root), source_revision)
            selected = MODULE.resolve_procedure_page("example", source_revision, root=root)
            self.assertIsNotNone(selected)
            assert selected is not None
            self.assertEqual(selected.revision, procedure_revision)
            self.assertEqual(selected.html, procedure_html)

    def test_compose_renders_and_cleans_its_temporary_variables_file(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            temp = Path(tempdir)
            output = temp / "frame.html"
            template = ROOT / "templates/smoke-report/inbound-peer-frame.html.j2"
            variables = {
                "title": "Fixture title",
                "generated_at": "2026-09-12T00:00:00Z",
                "pane_src": "pane.xhtml",
                "procedure_label": "fixture @ 00000000",
                "procedure_href": "procedures/fixture/00000000.html",
            }
            with mock.patch.object(MODULE.tempfile, "tempdir", tempdir):
                MODULE.compose(template, variables, output, root=ROOT)
            self.assertIn("Fixture title", output.read_text(encoding="utf-8"))
            self.assertEqual(list(temp.glob("*.json")), [])

    def test_compose_substitutes_the_callers_error_type_and_cleans_up(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            temp = Path(tempdir)
            with mock.patch.object(MODULE.tempfile, "tempdir", tempdir):
                with self.assertRaisesRegex(FixtureError, "sc-compose render failed"):
                    MODULE.compose(
                        temp / "missing-template.j2",
                        {"value": "fixture"},
                        temp / "out.html",
                        root=ROOT,
                        error_type=FixtureError,
                    )
            self.assertEqual(list(temp.glob("*.json")), [])


if __name__ == "__main__":
    unittest.main()
