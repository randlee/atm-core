from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


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
        procedure = next(item for item in self.manifest["procedures"] if len(item["revisions"]) >= 2)
        paths = [ROOT / "site/reports" / item["html"] for item in procedure["revisions"]]
        self.assertNotEqual(paths[0].read_text(), paths[1].read_text())
        self.assertIn("<table", paths[0].read_text())

    def test_revision_entry_without_section_is_a_render_error(self):
        with self.assertRaises(MODULE.ProcedureRenderError):
            MODULE.revision_content("No steps here")

    def test_inventory_matches_committed_reports_and_runner_targets(self):
        inventory = MODULE.json.loads((ROOT / "docs/procedures/inventory.json").read_text())
        self.assertEqual({item["procedure"] for item in self.manifest["procedures"]}, {item["id"] for item in inventory["procedures"]})

    def test_flow_svg_and_steps_table_are_markup_not_text(self):
        page = next((ROOT / "site/reports/procedures").glob("*/????????.html"))
        text = page.read_text()
        self.assertIn("<svg", text)
        self.assertIn("<table", text)

    def test_scalar_title_stays_escaped(self):
        page = next((ROOT / "site/reports/procedures").glob("*/????????.html"))
        self.assertNotIn("<script>", page.read_text().lower())

    def test_manifest_lists_every_revision_in_front_matter_order(self):
        for item in self.manifest["procedures"]:
            source = (ROOT / "docs/procedures" / f"{item['procedure']}.md").read_text()
            revisions = [line.split(":", 1)[1].strip() for line in source.splitlines() if line.strip().startswith("- rev:")]
            self.assertEqual(revisions, [revision["rev"] for revision in item["revisions"]])

    def test_check_mode_detects_stale_page(self):
        self.assertEqual(MODULE.render(ROOT, check=True), 0)


if __name__ == "__main__":
    unittest.main()
