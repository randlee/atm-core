from __future__ import annotations

from html.parser import HTMLParser
from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
HOME = REPO_ROOT / "site/index.html"
WORKFLOW = REPO_ROOT / ".github/workflows/pages.yml"


class LinkParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.links: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag == "a":
            self.links.extend(value for name, value in attrs if name == "href" and value is not None)


class PagesSiteTests(unittest.TestCase):
    def test_home_links_to_generated_reports_index(self) -> None:
        parser = LinkParser()
        parser.feed(HOME.read_text(encoding="utf-8"))
        self.assertIn("reports/", parser.links)
        self.assertIn("announcements/", parser.links)
        self.assertTrue((HOME.parent / "reports/index.html").is_file())

    def test_pages_workflow_validates_and_uploads_only_site(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("fetch-depth: 0", workflow)
        self.assertIn("python3 .just/check_site_generated.py", workflow)
        self.assertIn("python3 .just/check_site_links.py", workflow)
        gate = (REPO_ROOT / ".just/check_site_generated.py").read_text(encoding="utf-8")
        self.assertIn('("scripts/procedures/render_procedure_pages.py", "--check", "just procedures")', gate)
        self.assertIn('(".just/generate_report_index.py", "--check", "just reports-index")', gate)
        self.assertIn("actions/upload-pages-artifact@v3", workflow)
        self.assertIn("path: site", workflow)
        self.assertIn("actions/deploy-pages@v4", workflow)
        self.assertIn("pages: write", workflow)
        self.assertIn("id-token: write", workflow)

    def test_pages_workflow_is_the_only_pages_publisher(self) -> None:
        workflow_text = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (REPO_ROOT / ".github/workflows").glob("*.y*ml")
        )
        self.assertEqual(workflow_text.count("actions/upload-pages-artifact@"), 1)
        self.assertEqual(workflow_text.count("actions/deploy-pages@"), 1)
        self.assertNotIn("peaceiris/actions-gh-pages", workflow_text)

    def test_workflow_has_develop_site_trigger_and_manual_trigger(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("      - develop\n", workflow)
        self.assertIn('      - "site/**"\n', workflow)
        self.assertNotIn("integrate/phase-ai-31-33", workflow)
        self.assertIn("workflow_dispatch:", workflow)


if __name__ == "__main__":
    unittest.main()
