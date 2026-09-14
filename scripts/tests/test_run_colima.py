"""Unit tests for the one-container colima integration driver."""

from __future__ import annotations

import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest import mock
import xml.etree.ElementTree as ET

from scripts.integration import render_colima
from scripts.integration import run_colima as DRIVER


def make_root(tempdir: str) -> Path:
    root = Path(tempdir)
    reports = root / "site/reports"
    procedures = []
    names = (*render_colima.STEP_PROCEDURES.values(), render_colima.SEQUENCE_PROCEDURE)
    for procedure in names:
        page = reports / "procedures" / procedure / "00000000.html"
        page.parent.mkdir(parents=True, exist_ok=True)
        page.write_text(f"<html>{procedure}</html>\n", encoding="utf-8")
        procedures.append(
            {
                "procedure": procedure,
                "family": "integration",
                "revisions": [
                    {
                        "rev": "0" * 40,
                        "date": "2026-01-01",
                        "html": f"procedures/{procedure}/00000000.html",
                    }
                ],
            }
        )
    (reports / "procedures/manifest.json").write_text(
        json.dumps({"schema_version": 1, "procedures": procedures}), encoding="utf-8"
    )
    shutil.copytree(DRIVER.ROOT / render_colima.TEMPLATES, root / render_colima.TEMPLATES)
    return root


def runner(name: str, calls: list[str], *, fail: bool = False):
    def run(_container: str, step_dir: Path) -> dict[str, object]:
        calls.append(name)
        if fail:
            raise RuntimeError(f"{name} failed")
        payload = {
            "feature": DRIVER.FEATURES[name],
            "generated_at": "2026-09-13T00:00:00Z",
            "source_revision": "0" * 40,
            "container": DRIVER.DEFAULT_CONTAINER,
            "status": "PASS",
            "cases": [{"name": name, "status": "PASS", "detail": "ran"}],
        }
        step_dir.mkdir(parents=True, exist_ok=True)
        (step_dir / "step.json").write_text(json.dumps(payload), encoding="utf-8")
        return payload

    return run


class DriverTests(unittest.TestCase):
    def test_parse_steps_keeps_declared_order_and_rejects_bad_input(self) -> None:
        self.assertEqual(DRIVER.parse_steps("prompt-handoffs,task-start"), ("task-start", "prompt-handoffs"))
        with self.assertRaisesRegex(DRIVER.DriverError, "unknown step"):
            DRIVER.parse_steps("unknown")
        with self.assertRaisesRegex(DRIVER.DriverError, "only once"):
            DRIVER.parse_steps("assignment,assignment")

    def test_failure_is_recorded_later_steps_run_and_all_panels_parse(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = make_root(tempdir)
            run_dir = root / "site/reports/integration/colima/20260913T000000Z"
            calls: list[str] = []
            runners = {
                name: runner(name, calls, fail=name == "assignment") for name in DRIVER.STEP_ORDER
            }
            with mock.patch.object(DRIVER, "ROOT", root), mock.patch.object(
                DRIVER, "container_running", return_value=True
            ), mock.patch.object(
                DRIVER,
                "failure_payload",
                side_effect=lambda name, container, error: {
                    "feature": DRIVER.FEATURES[name],
                    "generated_at": "2026-09-13T00:00:00Z",
                    "source_revision": "0" * 40,
                    "container": container,
                    "status": "FAIL",
                    "cases": [{"name": name, "status": "FAIL", "detail": str(error)}],
                },
            ):
                summary = DRIVER.run_sequence(
                    run_dir,
                    DRIVER.STEP_ORDER,
                    testbed=Path(tempdir),
                    container=DRIVER.DEFAULT_CONTAINER,
                    runners=runners,
                )
            self.assertEqual(calls, list(DRIVER.STEP_ORDER))
            self.assertEqual(summary["status"], "FAIL")
            self.assertEqual([step["name"] for step in summary["steps"]], list(DRIVER.STEP_ORDER))
            self.assertEqual(summary["steps"][2]["status"], "FAIL")
            self.assertEqual(summary["procedure"], render_colima.SEQUENCE_PROCEDURE)
            for panel in run_dir.glob("steps/*/panel.xhtml"):
                ET.parse(panel)

    def test_subset_starts_fixture_once_and_keeps_subset_order(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = make_root(tempdir)
            run_dir = root / "site/reports/integration/colima/20260913T000001Z"
            calls: list[str] = []
            starts: list[str] = []
            selected = DRIVER.parse_steps("prompt-handoffs,task-start")

            def start(_testbed: Path, container: str) -> dict[str, str]:
                starts.append(container)
                return {"status": "started"}

            with mock.patch.object(DRIVER, "ROOT", root):
                summary = DRIVER.run_sequence(
                    run_dir,
                    selected,
                    testbed=Path(tempdir),
                    container=DRIVER.DEFAULT_CONTAINER,
                    runners={name: runner(name, calls) for name in selected},
                    fixture_start=start,
                )
            self.assertEqual(starts, [DRIVER.DEFAULT_CONTAINER])
            self.assertEqual(calls, ["task-start", "prompt-handoffs"])
            self.assertEqual(summary["status"], "PASS")
            self.assertEqual([step["name"] for step in summary["steps"]], calls)


if __name__ == "__main__":
    unittest.main()
