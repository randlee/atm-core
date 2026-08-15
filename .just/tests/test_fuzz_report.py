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
JUST_ROOT = ROOT / ".just"
for path in (ROOT, JUST_ROOT):
    if str(path) not in sys.path:
        sys.path.insert(0, str(path))

from run_fuzz import build_result
from run_fuzz import default_campaign
from run_fuzz import validate_campaign
from scripts.fuzz.render_report import FuzzReportError
from scripts.fuzz.render_report import normalize_campaign
from scripts.fuzz.render_report import render_campaign


V1_FIXTURE = ROOT / ".just/fixtures/fuzz/ai48-report-mixed.json"


class FuzzReportTests(unittest.TestCase):
    def v2_report(self) -> dict:
        return build_result(validate_campaign(default_campaign(ROOT), ROOT, require_campaign_id=True))

    def test_v2_contract_probe_normalizes_without_synthetic_workers(self) -> None:
        session = normalize_campaign(self.v2_report())
        self.assertEqual(session["schema_version"], "adversarial-fuzzing/v2")
        self.assertEqual([worker["agent_id"] for worker in session["workers"]], [
            "shape-probe", "template-probe", "boundary-probe", "differential-probe",
        ])
        self.assertTrue(all(worker["target_invocation"]["proofs"] for worker in session["workers"]))

    def test_v1_is_rejected_before_rendering(self) -> None:
        payload = json.loads(V1_FIXTURE.read_text(encoding="utf-8"))
        with self.assertRaisesRegex(FuzzReportError, "expected schema_version adversarial-fuzzing/v2"):
            normalize_campaign(payload)

    def test_zero_or_missing_product_proof_is_rejected_by_shared_validator(self) -> None:
        payload = self.v2_report()
        payload["workers"][0]["target_invocation"]["proofs"][0]["invocation_count"] = 0
        with self.assertRaisesRegex(FuzzReportError, "invalid v2 fuzz report:.*invocation_count"):
            normalize_campaign(payload)

    def test_renders_v2_proof_and_negative_contract_artifacts(self) -> None:
        payload = self.v2_report()
        issue = {
            "issue_id": "fuzz-render-deferred-example",
            "case_id": "documented-deferred-case",
            "category": "tooling",
            "observed_evidence": "retained example issue proves the HTML issue section is visible",
            "disposition": "deferred",
            "owner": "atm-dev",
            "tracking_ref": "GH-EXAMPLE",
            "defer_reason": "fixture coverage only",
        }
        payload["workers"][0]["encountered_issues"] = [issue]
        payload["campaign_issues"] = [{**issue, "worker_correlation_id": "shape-probe"}]
        payload["summary"]["open_campaign_issues"] = 1
        with tempfile.TemporaryDirectory() as tempdir:
            report_root = Path(tempdir)
            report = render_campaign(payload, "v2-contract-report", report_root, invoke_index=False)
            report_path = report_root / "v2-contract-report.html"
            evidence_dir = report_root / "v2-contract-report"
            self.assertTrue(report_path.is_file())
            self.assertEqual(len(report["sections"]), 4)
            rendered = report_path.read_text(encoding="utf-8")
            self.assertIn("Verified target invocation", rendered)
            self.assertIn("Negative diagnostic contracts", rendered)
            self.assertIn("Encountered issues", rendered)
            self.assertIn("fuzz-render-deferred-example", rendered)
            self.assertIn("run_fuzz.validate_worker_result", rendered)
            for section in report["sections"]:
                panel = report_root / section["xhtml_path"]
                self.assertTrue(panel.is_file())
                ET.parse(panel)
            sidecar = evidence_dir / "v2-contract-report.json"
            self.assertIn('"schema_version": "adversarial-fuzzing/v2"', sidecar.read_text(encoding="utf-8"))

    def test_reports_index_is_invoked_after_v2_artifacts_are_written(self) -> None:
        real_run = subprocess.run

        def run_with_index_mock(*args, **kwargs):
            if args[0][:2] == ["just", "reports-index"]:
                return mock.Mock(returncode=0, stdout="", stderr="")
            return real_run(*args, **kwargs)

        with tempfile.TemporaryDirectory() as tempdir, mock.patch(
            "scripts.fuzz.render_report.subprocess.run", side_effect=run_with_index_mock
        ) as run:
            render_campaign(self.v2_report(), "v2-contract-index", Path(tempdir), invoke_index=True)
            self.assertEqual(run.call_args.args[0], ["just", "reports-index"])


if __name__ == "__main__":
    unittest.main()
