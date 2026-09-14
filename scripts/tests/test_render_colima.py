"""Render path for colima integration runs: every historical payload shape, aggregate verdicts, index discovery."""

from __future__ import annotations

import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
import xml.dom.minidom

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
if str(ROOT / ".just") not in sys.path:
    sys.path.insert(0, str(ROOT / ".just"))

from generate_report_index import build_pages  # noqa: E402
from scripts.integration import render_colima  # noqa: E402

COLIMA = ROOT / "site/reports/integration/colima"
# One committed run per historical payload shape (bb4, bb5, bb6, colima-hermes-skills).
SHAPES = {
    "task-start": "20260913T080203865232Z",
    "assignment": "20260913T084626438924Z",
    "prompt-handoffs": "20260913T080436184071Z",
    "hermes-skills": "20260912T111004Z",
}
FAILED_HERMES_SKILLS = "20260909T001110Z"


def make_root(tempdir: str) -> Path:
    """A reports root with the four colima procedures at one dated revision and the real templates."""
    root = Path(tempdir)
    reports = root / "site/reports"
    procedures = []
    for procedure in (*render_colima.STEP_PROCEDURES.values(), render_colima.SEQUENCE_PROCEDURE):
        page = reports / "procedures" / procedure / "00000000.html"
        page.parent.mkdir(parents=True, exist_ok=True)
        page.write_text(f"<html>{procedure}</html>\n", encoding="utf-8")
        procedures.append({"procedure": procedure, "family": "integration", "revisions": [
            {"rev": "0" * 40, "date": "2026-01-01", "html": f"procedures/{procedure}/00000000.html"},
        ]})
    (reports / "procedures/manifest.json").write_text(json.dumps({"schema_version": 1, "procedures": procedures}), encoding="utf-8")
    shutil.copytree(ROOT / render_colima.TEMPLATES, root / render_colima.TEMPLATES)
    return root


def committed_step(name: str, run: str) -> Path:
    return COLIMA / run / "steps" / f"01-{name}"


class RenderColimaTests(unittest.TestCase):
    def test_every_historical_payload_shape_renders_a_well_formed_panel_and_run(self) -> None:
        for name, run in SHAPES.items():
            with self.subTest(step=name), tempfile.TemporaryDirectory() as tempdir:
                root = make_root(tempdir)
                out = root / "site/reports/integration/colima" / run
                source = committed_step(name, run)
                step_dir = render_colima.add_step(out, name, source / "step.json")
                for sidecar in source.iterdir():
                    if sidecar.name not in {"step.json", "panel.xhtml"}:
                        shutil.copyfile(sidecar, step_dir / sidecar.name)
                self.assertEqual(render_colima.main(["render_colima.py", "--out", str(out), "--root", str(root)]), 0)
                self.assertEqual((step_dir / "step.json").read_bytes(), (source / "step.json").read_bytes())
                panel = step_dir / "panel.xhtml"
                xml.dom.minidom.parseString(panel.read_bytes())
                panel_text = panel.read_text(encoding="utf-8")
                self.assertIn(f'href="../../../../../procedures/{render_colima.STEP_PROCEDURES[name]}/00000000.html"', panel_text)
                index = (out / "index.html").read_text(encoding="utf-8")
                self.assertIn(f'<iframe src="steps/01-{name}/panel.xhtml"', index)
                self.assertIn(f'href="../../../procedures/{render_colima.STEP_PROCEDURES[name]}/00000000.html"', index)
                summary = json.loads((out / "integration.json").read_text(encoding="utf-8"))
                self.assertEqual(summary["status"], "PASS")
                self.assertEqual([step["panel"] for step in summary["steps"]], [f"steps/01-{name}/panel.xhtml"])
                self.assertNotIn("report_html", summary)
                envelope = json.loads((out.parent / f"{run}.envelope.json").read_text(encoding="utf-8"))
                self.assertEqual(envelope["report_type"], "integration")
                self.assertEqual(envelope["report_html"], f"integration/colima/{run}/index.html")
                self.assertEqual(envelope["procedure"], render_colima.STEP_PROCEDURES[name])

    def test_hermes_skills_sidecars_are_shown_in_the_panel(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = make_root(tempdir)
            run = SHAPES["hermes-skills"]
            out = root / "site/reports/integration/colima" / run
            source = committed_step("hermes-skills", run)
            step_dir = render_colima.add_step(out, "hermes-skills", source / "step.json")
            shutil.copyfile(source / "result.txt", step_dir / "result.txt")
            shutil.copyfile(source / "report-1.txt", step_dir / "report-1.txt")
            render_colima.render_run(out, root=root)
            panel = (step_dir / "panel.xhtml").read_text(encoding="utf-8")
            self.assertIn("<h2>result.txt</h2>", panel)
            self.assertIn("<h2>report-1.txt</h2>", panel)
            self.assertIn("ATM TEST REPORT", panel)

    def test_one_failing_step_fails_the_run_and_order_is_kept(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = make_root(tempdir)
            out = root / "site/reports/integration/colima/20260913T000000Z"
            render_colima.add_step(out, "hermes-skills", committed_step("hermes-skills", SHAPES["hermes-skills"]) / "step.json")
            render_colima.add_step(out, "hermes-skills", committed_step("hermes-skills", FAILED_HERMES_SKILLS) / "step.json")
            summary = render_colima.render_run(out, root=root)
            self.assertEqual(summary["status"], "FAIL")
            self.assertEqual([(step["order"], step["status"]) for step in summary["steps"]], [(1, "PASS"), (2, "FAIL")])
            index = (out / "index.html").read_text(encoding="utf-8")
            self.assertLess(index.index("steps/01-hermes-skills/panel.xhtml"), index.index("steps/02-hermes-skills/panel.xhtml"))
            envelope = json.loads((out.parent / "20260913T000000Z.envelope.json").read_text(encoding="utf-8"))
            self.assertEqual(envelope["status"], "FAIL")

    def test_rejects_step_gaps_and_unknown_steps_and_selects_sequence_procedure(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = make_root(tempdir)
            out = root / "site/reports/integration/colima/20260913T000000Z"
            payload = committed_step("hermes-skills", SHAPES["hermes-skills"]) / "step.json"
            with self.assertRaisesRegex(render_colima.RenderError, "unknown step"):
                render_colima.add_step(out, "nope", payload)
            render_colima.add_step(out, "hermes-skills", payload, order=1)
            render_colima.add_step(out, "hermes-skills", payload, order=3)
            with self.assertRaisesRegex(render_colima.RenderError, "without gaps"):
                render_colima.render_run(out, root=root)
            shutil.rmtree(out / "steps/03-hermes-skills")
            task_dir = render_colima.add_step(
                out,
                "task-start",
                committed_step("task-start", SHAPES["task-start"]) / "step.json",
            )
            task_payload = json.loads((task_dir / "step.json").read_text())
            first_payload = json.loads((out / "steps/01-hermes-skills/step.json").read_text())
            task_payload["source_revision"] = first_payload.get("source_revision")
            task_payload["container"] = first_payload.get("host")
            task_payload.pop("atm_version", None)
            (task_dir / "step.json").write_text(json.dumps(task_payload), encoding="utf-8")
            summary = render_colima.render_run(out, root=root)
            self.assertEqual(summary["procedure"], render_colima.SEQUENCE_PROCEDURE)
            envelope = json.loads((out.parent / "20260913T000000Z.envelope.json").read_text())
            self.assertEqual(envelope["procedure"], render_colima.SEQUENCE_PROCEDURE)

    def test_rendered_runs_are_discovered_as_the_integration_family(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = make_root(tempdir)
            reports = root / "site/reports"
            for name, run in (("hermes-skills", FAILED_HERMES_SKILLS), ("assignment", SHAPES["assignment"])):
                out = reports / "integration/colima" / run
                render_colima.add_step(out, name, committed_step(name, run) / "step.json")
                render_colima.render_run(out, root=root)
            pages = build_pages(reports)
            index = pages["index.html"]
            self.assertIn("Integration (colima)", index)
            self.assertIn('href="history/integration.html">2 runs</a>', index)
            history = pages["history/integration.html"]
            self.assertLess(history.index(SHAPES["assignment"]), history.index(FAILED_HERMES_SKILLS))
            self.assertIn('<span class="result fail">FAIL</span>', history)
            run_page = pages[f"integration/colima/{FAILED_HERMES_SKILLS}/index.html"]
            self.assertIn("Result: <strong", run_page)
            self.assertIn("Integration (colima) history", run_page)
            self.assertNotIn("Smoke history", run_page)

    def test_committed_runs_are_the_output_of_this_renderer(self) -> None:
        """Every committed run re-renders identically in place (payloads are the only inputs)."""
        for run in sorted(path.name for path in COLIMA.iterdir() if path.is_dir()):
            with self.subTest(run=run), tempfile.TemporaryDirectory() as tempdir:
                root = Path(tempdir)
                shutil.copytree(ROOT / "site/reports/procedures", root / "site/reports/procedures")
                shutil.copytree(ROOT / render_colima.TEMPLATES, root / render_colima.TEMPLATES)
                out = root / "site/reports/integration/colima" / run
                shutil.copytree(COLIMA / run, out)
                for rendered in (out / "index.html", out / "integration.json"):
                    rendered.unlink()
                for panel in out.glob("steps/*/panel.xhtml"):
                    panel.unlink()
                summary = render_colima.render_run(out, root=root)
                committed = json.loads((COLIMA / run / "integration.json").read_text(encoding="utf-8"))
                # Procedure ancestry needs the real Git checkout; everything else must match exactly.
                for record in (summary, committed):
                    record.pop("procedure_revision", None); record.pop("procedure_revision_inferred", None)
                    for step in record["steps"]:
                        step.pop("procedure_revision", None); step.pop("procedure_revision_inferred", None)
                self.assertEqual(summary, committed)
                self.assertEqual(
                    sorted(path.name for path in (out / "steps").rglob("*")),
                    sorted(path.name for path in (COLIMA / run / "steps").rglob("*")),
                )


if __name__ == "__main__":
    unittest.main()
