"""Regression tests for the repository's HTML/XHTML sc-compose report templates.

sc-compose HTML-escapes every variable rendered into an ``.html``/``.xhtml``
output. The smoke runners build their evidence panes as already-escaped HTML
fragments, so the templates must pass those fragments through with ``safe``;
otherwise the committed evidence renders the markup as literal text (every
smoke and read-benchmark evidence set from 2026-08-20 to 2026-09-12 shipped
that way).
"""
from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest
import xml.dom.minidom

REPO_ROOT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.sc_compose_render import render_file
TEMPLATES = REPO_ROOT / "templates/smoke-report"
JUST_TEMPLATES = REPO_ROOT / ".just/templates"
FRAGMENT = '<h1>host — smoke</h1><table><tr class="pass"><td>✓</td><td>a &amp; b</td></tr></table>'


def render(template: Path, variables: dict[str, str], output: Path) -> str:
    try:
        rendered = render_file(template, variables, root=REPO_ROOT, strict=False)
    except Exception as error:
        raise AssertionError(f"sc-compose wheel render failed: {error}") from error
    output.write_text(rendered, encoding="utf-8")
    return rendered


class FragmentPassThroughTests(unittest.TestCase):
    def test_pane_body_is_markup_not_text(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            page = render(TEMPLATES / "inbound-peer-pane.xhtml.j2",
                          {"title": "t", "host": "h", "generated_at": "now", "body_html": FRAGMENT},
                          Path(tmp) / "pane.xhtml")
        self.assertIn(f"<body>{FRAGMENT}</body>", page)
        self.assertNotIn("&lt;", page)
        xml.dom.minidom.parseString(page.encode("utf-8"))

    def test_review_pages_embed_pane_markup(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            html_page = render(TEMPLATES / "inbound-peer-review.html.j2",
                               {"title": "t", "generated_at": "now", "pane_html": FRAGMENT},
                               Path(tmp) / "index.html")
            xhtml_page = render(TEMPLATES / "inbound-peer-review.xhtml.j2",
                                {"title": "t", "generated_at": "now", "pane_html": FRAGMENT},
                                Path(tmp) / "review.xhtml")
        self.assertIn(f"<main>{FRAGMENT}</main>", html_page)
        self.assertIn(f"{FRAGMENT}</body>", xhtml_page)
        xml.dom.minidom.parseString(xhtml_page.encode("utf-8"))

    def test_view_report_and_panel_embed_fragments(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report = render(JUST_TEMPLATES / "view-report.html.j2",
                            {"output_path": "o", "json_output_path": "j", "title": "t", "status": "PASS",
                             "status_class": "status-pass", "summary_html": FRAGMENT, "sections_html": FRAGMENT,
                             "recommendations_html": FRAGMENT, "footer_html": FRAGMENT},
                            Path(tmp) / "report.html")
            panel = render(JUST_TEMPLATES / "view-panel.xhtml.j2",
                           {"title": "t", "header_color": "#000", "accent_color": "#fff", "fragment_source": "src",
                            "copy_json": "{}", "copy_context": "ctx", "body_html": FRAGMENT},
                           Path(tmp) / "panel.xhtml")
        self.assertEqual(report.count(FRAGMENT), 4)
        self.assertIn(FRAGMENT, panel)
        self.assertNotIn("&lt;h1", report + panel)

    def test_scalar_variables_stay_escaped(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            page = render(TEMPLATES / "inbound-peer-frame.html.j2",
                          {"title": "<script>", "generated_at": "now", "pane_src": "p.xhtml"},
                          Path(tmp) / "frame.html")
        self.assertIn("<title>&lt;script&gt;</title>", page)
        self.assertNotIn("<script>", page)


if __name__ == "__main__":
    unittest.main()
