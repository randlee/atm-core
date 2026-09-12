from __future__ import annotations

import importlib.util
from html.parser import HTMLParser
from pathlib import Path
import tempfile
import unittest
from unittest import mock


class _MarkupProbe(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.svg = 0
        self.tables = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag == "svg":
            self.svg += 1
        elif tag == "table":
            self.tables += 1


class _TableProbe(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.rows: list[list[str]] = []
        self._row: list[str] | None = None
        self._cell: list[str] | None = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag == "tr":
            self._row = []
        elif self._row is not None and tag in {"td", "th"}:
            self._cell = []

    def handle_data(self, data: str) -> None:
        if self._cell is not None:
            self._cell.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag in {"td", "th"} and self._row is not None and self._cell is not None:
            self._row.append("".join(self._cell).strip())
            self._cell = None
        elif tag == "tr" and self._row is not None:
            self.rows.append(self._row)
            self._row = None


def table_rows(path: Path) -> list[list[str]]:
    parser = _TableProbe()
    parser.feed(path.read_text(encoding="utf-8"))
    return parser.rows


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/procedures/render_procedure_pages.py"
spec = importlib.util.spec_from_file_location("render_procedure_pages", SCRIPT)
assert spec and spec.loader
MODULE = importlib.util.module_from_spec(spec)
spec.loader.exec_module(MODULE)


class ProcedurePageTests(unittest.TestCase):
    def setUp(self) -> None:
        self.manifest = MODULE.json.loads((ROOT / "site/reports/procedures/manifest.json").read_text())

    def test_renders_one_page_per_revision_entry(self):
        for procedure in self.manifest["procedures"]:
            pages = list((ROOT / "site/reports" / procedure["revisions"][0]["html"]).parent.glob("*.html"))
            self.assertEqual(len(pages), len(procedure["revisions"]) + 1)

    def test_two_revisions_render_distinct_step_tables(self):
        procedure = next(item for item in self.manifest["procedures"] if item["procedure"] == "smoke-thorough")
        revisions = {item["rev"][:8]: item for item in procedure["revisions"]}
        older = table_rows(ROOT / "site/reports" / revisions["93458691"]["html"])[1:]
        newer = table_rows(ROOT / "site/reports" / revisions["513b3374"]["html"])[1:]
        self.assertEqual(len(older), 26)
        self.assertEqual(len(newer), 27)
        self.assertNotEqual({row[1] for row in older}, {row[1] for row in newer})
        self.assertIn("Execute `GRAFT-001`", {row[1] for row in newer})
        self.assertNotIn("Execute `GRAFT-001`", {row[1] for row in older})

    def test_revision_entry_without_section_is_a_render_error(self):
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            docs = root / "docs/procedures"
            docs.mkdir(parents=True)
            (docs / "broken.md").write_text(
                "---\nprocedure: broken\nfamily: smoke\nrunner: runner.py\nrevisions:\n"
                "  - rev: " + "a" * 40 + "\n    date: 2026-01-01\n    note: broken\n---\n"
                "\n## What this test proves\nA fixture.\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(MODULE.ProcedureRenderError, "no matching section"):
                MODULE.render(root)

    def test_inventory_matches_committed_reports_and_runner_targets(self):
        inventory = MODULE.json.loads((ROOT / "docs/procedures/inventory.json").read_text())
        self.assertEqual({item["procedure"] for item in self.manifest["procedures"]}, {item["id"] for item in inventory["procedures"]})

    def test_flow_svg_and_steps_table_are_markup_not_text(self):
        page = next((ROOT / "site/reports/procedures").glob("*/????????.html"))
        probe = _MarkupProbe()
        probe.feed(page.read_text())
        self.assertGreater(probe.svg, 0)
        self.assertGreater(probe.tables, 0)

    def test_scalar_title_stays_escaped(self):
        page = next((ROOT / "site/reports/procedures").glob("*/????????.html"))
        self.assertNotIn("<script>", page.read_text().lower())

    def test_manifest_lists_every_revision_in_front_matter_order(self):
        for item in self.manifest["procedures"]:
            source = (ROOT / "docs/procedures" / f"{item['procedure']}.md").read_text()
            revisions = [line.split(":", 1)[1].strip() for line in source.splitlines() if line.strip().startswith("- rev:")]
            self.assertEqual(revisions, [revision["rev"] for revision in item["revisions"]])

    def test_front_matter_revisions_match_body_stamps(self):
        for path in sorted((ROOT / "docs/procedures").glob("*.md")):
            metadata, body, sections = MODULE.parse_document(path)
            MODULE.validate_revision_stamps(path, metadata, body, sections)

    def test_check_mode_detects_stale_page(self):
        with mock.patch.object(MODULE, "mermaid_svg", side_effect=AssertionError("check mode rendered Mermaid")):
            self.assertEqual(MODULE.render(ROOT, check=True), 0)
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            docs = root / "docs/procedures"
            docs.mkdir(parents=True)
            revision = "a" * 40
            (docs / "example.md").write_text(
                "---\nprocedure: example\nfamily: smoke\nrunner: runner.py\n"
                f"revisions:\n  - rev: {revision}\n    date: 2026-01-01\n    note: current\n---\n"
                "\n## What this test proves\nA fixture.\n\n## Revision aaaaaaaa (2026-01-01)\n\n"
                "The runner revision aaaaaaaa is current.\n\n```mermaid\nflowchart LR\n  a --> b\n```\n\n## Steps\n"
                "| step | action | observable | evidence |\n| --- | --- | --- | --- |\n"
                "| 1 | Run | PASS at revision `aaaaaaaa` | report |\n",
                encoding="utf-8",
            )
            with mock.patch.object(MODULE, "compose", return_value="<html>generated</html>"), \
                    mock.patch.object(MODULE, "mermaid_svg", return_value="<svg />"):
                self.assertEqual(MODULE.render(root), 0)
            source = docs / "example.md"
            source.write_text(source.read_text(encoding="utf-8").replace("PASS at", "CHANGED at"), encoding="utf-8")
            with mock.patch.object(MODULE, "mermaid_svg", side_effect=AssertionError("check mode rendered Mermaid")):
                self.assertEqual(MODULE.render(root, check=True), 1)


if __name__ == "__main__":
    unittest.main()
