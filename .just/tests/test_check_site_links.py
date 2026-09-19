from __future__ import annotations

from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
import importlib.util
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
JUST_DIR = REPO_ROOT / ".just"
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))
SCRIPT_PATH = JUST_DIR / "check_site_links.py"
SPEC = importlib.util.spec_from_file_location("check_site_links", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise ImportError(f"unable to import {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class CheckSiteLinksTests(unittest.TestCase):
    def run_check(
        self, files: dict[str, str], *, site_root: str = "site"
    ) -> tuple[int, list[str]]:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            for relative, contents in files.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(contents, encoding="utf-8")
            output = StringIO()
            with redirect_stdout(output):
                result = MODULE.main(["--root", str(root), "--site-root", site_root])
            return result, output.getvalue().splitlines()

    def test_good_relative_link(self) -> None:
        result, lines = self.run_check(
            {"site/index.html": '<a href="reports/report.html">report</a>', "site/reports/report.html": "ok"}
        )
        self.assertEqual(result, 0)
        self.assertEqual(lines[-1], "site links: 2 pages scanned, 0 broken links")

    def test_good_directory_link_requires_existing_index(self) -> None:
        result, _lines = self.run_check(
            {"site/index.html": '<a href="reports/">reports</a>', "site/reports/index.html": "ok"}
        )
        self.assertEqual(result, 0)

    def test_directory_without_index_is_exact_failure(self) -> None:
        result, lines = self.run_check(
            {"site/index.html": '<a href="reports/">reports</a>', "site/reports/asset.txt": "ok"}
        )
        self.assertEqual(result, 1)
        self.assertEqual(
            lines[0],
            "site/index.html:1: reports/ -> site/reports/index.html (directory has no index.html)",
        )

    def test_missing_target_is_exact_failure(self) -> None:
        result, lines = self.run_check({"site/index.html": '<link href="missing.css">'})
        self.assertEqual(result, 1)
        self.assertEqual(
            lines,
            [
                "site/index.html:1: missing.css -> site/missing.css (target does not exist)",
                "site links: 1 pages scanned, 1 broken links",
            ],
        )

    def test_up_tree_path_that_stays_inside_site_passes(self) -> None:
        result, _lines = self.run_check(
            {"site/index.html": "home", "site/reports/report.html": '<a href="../index.html">home</a>'}
        )
        self.assertEqual(result, 0)

    def test_up_tree_escape_fails(self) -> None:
        result, lines = self.run_check(
            {"outside.html": "outside", "site/index.html": '<a href="../outside.html">outside</a>'}
        )
        self.assertEqual(result, 1)
        self.assertTrue(lines[0].endswith("outside.html (escapes site root)"), lines[0])

    def test_root_absolute_path_fails(self) -> None:
        result, lines = self.run_check({"site/index.html": '<a href="/reports/">reports</a>'})
        self.assertEqual(result, 1)
        self.assertEqual(lines[0], "site/index.html:1: /reports/ -> site/reports (root-absolute path)")

    def test_external_mailto_fragment_data_and_empty_links_are_ignored(self) -> None:
        html = """<a href="https://example.com">web</a>
<a href="mailto:test@example.com">mail</a>
<a href="#section">fragment</a>
<img src="data:image/png;base64,AA==">
<a href="">empty</a>
<script src="javascript:void(0)"></script>
<a href="tel:+15555550100">call</a>"""
        result, _lines = self.run_check({"site/index.html": html})
        self.assertEqual(result, 0)

    def test_query_and_fragment_are_stripped(self) -> None:
        result, _lines = self.run_check(
            {
                "site/report.html": (
                    '<a href="result.html?download=1#result">result</a>'
                    '<a href="?download=1#current">current</a>'
                ),
                "site/result.html": "ok",
            }
        )
        self.assertEqual(result, 0)

    def test_xhtml_pages_are_scanned(self) -> None:
        result, lines = self.run_check({"site/page.xhtml": '<a href="missing.html">missing</a>'})
        self.assertEqual(result, 1)
        self.assertEqual(lines[-1], "site links: 1 pages scanned, 1 broken links")

    def test_src_attributes_are_checked(self) -> None:
        html = """<img src="image.png">
<script src="app.js"></script>
<iframe src="frame.html"></iframe>
<source src="movie.mp4">"""
        result, lines = self.run_check({"site/index.html": html})
        self.assertEqual(result, 1)
        self.assertEqual(len(lines), 5)
        self.assertEqual([line.split(":", 2)[1] for line in lines[:-1]], ["1", "2", "3", "4"])

    def test_custom_site_root_is_supported(self) -> None:
        result, lines = self.run_check({"public/index.html": "ok"}, site_root="public")
        self.assertEqual(result, 0)
        self.assertEqual(lines[-1], "site links: 1 pages scanned, 0 broken links")


if __name__ == "__main__":
    unittest.main()
