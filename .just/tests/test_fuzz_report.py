from __future__ import annotations

from pathlib import Path
import json
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.fuzz.render_report import FuzzReportError
from scripts.fuzz.render_report import normalize_campaign
from scripts.fuzz.render_report import render_campaign


FIXTURE = ROOT / ".just/fixtures/fuzz/ai48-report-mixed.json"


class FuzzReportTests(unittest.TestCase):
    def fixture(self) -> dict:
        return json.loads(FIXTURE.read_text(encoding="utf-8"))

    def test_normalizes_success_failure_timeout_and_incomplete_workers(self) -> None:
        session = normalize_campaign(self.fixture())
        self.assertEqual([worker["agent_id"] for worker in session["workers"]], [
            "shape-probe", "template-probe", "boundary-probe", "differential-probe"
        ])
        self.assertEqual([worker["classification"] for worker in session["workers"]], [
            "pass", "confirmed_bug", "inconclusive", "inconclusive"
        ])
        self.assertNotIn("worktree_path", session["campaign"])
        self.assertEqual(session["outcome_ledger"], {
            "confirmed_bug": [], "non_repro": [], "benign": [], "inconclusive": []
        })

    def test_normalizes_complete_candidate_outcome_ledger(self) -> None:
        payload = self.fixture()
        payload["outcome_ledger"] = {
            "confirmed_bug": [],
            "non_repro": [{"candidate_id": "one", "outcome": "non_repro", "detail": "three replays"}],
            "benign": [{"candidate_id": "two", "outcome": "benign", "detail": "expected parse"}],
            "inconclusive": [{"candidate_id": "three", "outcome": "inconclusive", "detail": "outside scope"}],
        }
        session = normalize_campaign(payload)
        self.assertEqual(len(session["outcome_ledger"]["non_repro"]), 1)
        self.assertEqual(len(session["outcome_ledger"]["benign"]), 1)
        self.assertEqual(len(session["outcome_ledger"]["inconclusive"]), 1)

    def test_rejects_invalid_worker_envelope(self) -> None:
        payload = self.fixture()
        payload["workers"][0]["status"] = "unknown"
        with self.assertRaisesRegex(FuzzReportError, "invalid status"):
            normalize_campaign(payload)

    def test_synthesized_evidence_is_in_the_panel_and_copy_payload(self) -> None:
        payload = self.fixture()
        worker = payload["workers"][1]
        worker["findings"] = []
        worker.pop("test_inputs", None)

        normalized = normalize_campaign(payload)
        rendered = normalized["workers"][1]
        copied = json.loads(rendered["copy_json"])

        self.assertEqual(copied["findings"], rendered["findings"])
        self.assertEqual(copied["test_inputs"], rendered["test_inputs"])

    def test_renders_all_worker_outcomes_and_relative_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            report_root = Path(tempdir)
            report = render_campaign(self.fixture(), "20260801-1-fuzz-report", report_root, invoke_index=False)
            report_path = report_root / "20260801-1-fuzz-report.html"
            evidence_dir = report_root / "20260801-1-fuzz-report"
            self.assertTrue(report_path.is_file())
            self.assertEqual(len(report["sections"]), 4)
            self.assertNotIn("/Users/example", (evidence_dir / "20260801-1-fuzz-report.json").read_text())
            for section in report["sections"]:
                panel = report_root / section["xhtml_path"]
                self.assertTrue(panel.is_file())
                ET.parse(panel)
            self.assertIn("confirmed_bug", (evidence_dir / "20260801-1-fuzz-report.json").read_text())

    def test_non_repro_or_inconclusive_candidates_render_as_info_not_pass(self) -> None:
        payload = self.fixture()
        payload["outcome_ledger"] = {
            "confirmed_bug": [],
            "non_repro": [{"candidate_id": "one", "outcome": "non_repro", "detail": "replay recovered"}],
            "benign": [],
            "inconclusive": [],
        }
        with tempfile.TemporaryDirectory() as tempdir:
            report_root = Path(tempdir)
            report = render_campaign(payload, "20260801-info-fuzz-report", report_root, invoke_index=False)
            self.assertEqual(report["status"], "INFO")
            sidecar = report_root / "20260801-info-fuzz-report" / "20260801-info-fuzz-report.json"
            self.assertIn('"outcome_ledger"', sidecar.read_text(encoding="utf-8"))

    def test_reports_index_is_invoked_after_artifacts_are_written(self) -> None:
        real_run = subprocess.run

        def run_with_index_mock(*args, **kwargs):
            command = args[0]
            if command[:2] == ["just", "reports-index"]:
                return mock.Mock(returncode=0, stdout="", stderr="")
            return real_run(*args, **kwargs)

        with tempfile.TemporaryDirectory() as tempdir, mock.patch(
            "scripts.fuzz.render_report.subprocess.run", side_effect=run_with_index_mock
        ) as run:
            render_campaign(self.fixture(), "20260801-2-fuzz-report", Path(tempdir), invoke_index=True)
            self.assertEqual(run.call_args.args[0], ["just", "reports-index"])


if __name__ == "__main__":
    unittest.main()
