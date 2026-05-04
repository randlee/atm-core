from __future__ import annotations

from pathlib import Path
import json
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from build_view_site import build_boundaries_panel
from build_view_site import build_deps_panel
from build_view_site import build_modules_panel
from build_view_site import build_site
from build_view_site import build_unsafe_panel


def write_json(path: Path, data: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True), encoding="utf-8")


class ViewSiteTests(unittest.TestCase):
    def test_build_boundaries_panel_uses_summary_data(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_json(
                repo_root / "artifacts/view/boundaries/summary.json",
                {"docs": [{"doc": "docs/atm-core/boundaries.md", "records": "2", "active": "1", "planned": "1", "retired": "0"}], "record_count": 2, "violation_count": 0},
            )
            (repo_root / "artifacts/view/boundaries/summary.txt").write_text("ok\n", encoding="utf-8")
            (repo_root / "artifacts/view/boundaries/findings.txt").write_text("", encoding="utf-8")
            panel = build_boundaries_panel(repo_root, repo_root / "artifacts/view/panels/boundaries.xhtml")
            self.assertEqual(panel.status, "PASS")
            self.assertIn("2 records", panel.summary)

    def test_build_deps_panel_includes_duplicate_count(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            write_json(repo_root / "artifacts/view/deps/summary.json", {"graph_html": "artifacts/view/deps/index.html"})
            write_json(
                repo_root / "artifacts/view/deps/report.json",
                {"summary": {"total_dependencies": 10, "unique_crates": 9, "duplicate_crates": 1}, "diagnostics": {"duplicates": [{"name": "serde", "versions": ["1.0.0", "1.0.1"]}]}},
            )
            (repo_root / "artifacts/view/deps/index.html").write_text("<html></html>", encoding="utf-8")
            (repo_root / "artifacts/view/deps/report.html").write_text("<html></html>", encoding="utf-8")
            panel = build_deps_panel(repo_root, repo_root / "artifacts/view/panels/deps.xhtml")
            self.assertEqual(panel.status, "PASS")
            self.assertIn("1 duplicates", panel.summary)

    def test_build_modules_panel_marks_failure_from_latest_log(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            log_dir = repo_root / ".just/logs"
            log_dir.mkdir(parents=True)
            (log_dir / "20260505-view-modules.log").write_text("exit_code: 1\nAssertion failed: dot exploded\n", encoding="utf-8")
            panel = build_modules_panel(repo_root, repo_root / "artifacts/view/panels/modules.xhtml")
            self.assertEqual(panel.status, "ERROR")
            self.assertIn("Graphviz", panel.summary)

    def test_build_unsafe_panel_marks_geiger_blocked(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            log_dir = repo_root / ".just/logs"
            log_dir.mkdir(parents=True)
            (log_dir / "20260505-view-unsafe.log").write_text(
                "exit_code: 1\nFailed to match (ignoring source) package: registry+https://github.com/rust-lang/crates.io-index#valuable@0.1.1\n",
                encoding="utf-8",
            )
            panel = build_unsafe_panel(repo_root, repo_root / "artifacts/view/panels/unsafe.xhtml")
            self.assertEqual(panel.status, "ERROR")
            self.assertIn("cargo-geiger", panel.summary)

    @mock.patch("build_view_site.validate_xhtml")
    @mock.patch("build_view_site.render_template")
    def test_build_site_writes_index_json(self, render_template_mock: mock.Mock, validate_xhtml_mock: mock.Mock) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / ".just/templates").mkdir(parents=True)
            (repo_root / ".just/templates/view-report.html.j2").write_text("stub", encoding="utf-8")
            (repo_root / ".just/templates/view-panel.xhtml.j2").write_text("stub", encoding="utf-8")
            write_json(
                repo_root / "artifacts/view/boundaries/summary.json",
                {"docs": [], "record_count": 0, "violation_count": 0},
            )
            (repo_root / "artifacts/view/boundaries/summary.txt").write_text("ok\n", encoding="utf-8")
            (repo_root / "artifacts/view/boundaries/findings.txt").write_text("", encoding="utf-8")
            write_json(repo_root / "artifacts/view/deps/summary.json", {"graph_html": "artifacts/view/deps/index.html"})
            write_json(repo_root / "artifacts/view/deps/report.json", {"summary": {"total_dependencies": 1, "unique_crates": 1, "duplicate_crates": 0}, "diagnostics": {"duplicates": []}})
            (repo_root / "artifacts/view/deps/index.html").write_text("<html></html>", encoding="utf-8")
            (repo_root / "artifacts/view/deps/report.html").write_text("<html></html>", encoding="utf-8")
            index_path = build_site(repo_root)
            self.assertEqual(index_path, repo_root / "artifacts/view/index.html")
            model = json.loads((repo_root / "artifacts/view/index.json").read_text(encoding="utf-8"))
            self.assertEqual(len(model["sections"]), 4)
            self.assertTrue(render_template_mock.called)
            self.assertTrue(validate_xhtml_mock.called)


if __name__ == "__main__":
    unittest.main()
