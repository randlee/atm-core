"""Regression tests for the smoke-report sc-compose templates.

sc-compose HTML-escapes every variable rendered into an ``.html``/``.xhtml``
output. The smoke runners build their evidence panes as already-escaped HTML
fragments, so the templates must pass those fragments through with ``safe``;
otherwise the committed evidence renders the markup as literal text (every
evidence set from 2026-08-20 to 2026-09-12 shipped that way).
"""
from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import xml.dom.minidom

REPO_ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = REPO_ROOT / "templates/smoke-report"
FRAGMENT = '<h1>host — smoke</h1><table><tr class="pass"><td>✓</td><td>a &amp; b</td></tr></table>'


def render(template: Path, variables: dict[str, str], output: Path) -> str:
    with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
        json.dump(variables, handle)
    completed = subprocess.run(
        ["sc-compose", "render", "--root", str(REPO_ROOT), "--file", str(template),
         "--var-file", handle.name, "--output", str(output)],
        capture_output=True, text=True, check=False,
    )
    Path(handle.name).unlink()
    if completed.returncode != 0:
        raise AssertionError(f"sc-compose render failed: {completed.stderr.strip() or completed.stdout.strip()}")
    return output.read_text(encoding="utf-8")


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

    def test_scalar_variables_stay_escaped(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            page = render(TEMPLATES / "inbound-peer-frame.html.j2",
                          {"title": "<script>", "generated_at": "now", "pane_src": "p.xhtml"},
                          Path(tmp) / "frame.html")
        self.assertIn("<title>&lt;script&gt;</title>", page)
        self.assertNotIn("<script>", page)


if __name__ == "__main__":
    unittest.main()
