from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
from copy import deepcopy
import io
import json
from pathlib import Path
import sys
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
FIXTURES = JUST_DIR / "fixtures/fuzz"
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_fuzz import CONTRACT_PROBE_SEAM
from run_fuzz import FuzzInputError
from run_fuzz import SCHEMA_VERSION
from run_fuzz import build_result
from run_fuzz import default_campaign
from run_fuzz import main
from run_fuzz import validate_campaign
from run_fuzz import validate_report
from run_fuzz import validate_worker_result


class FuzzRunnerTests(unittest.TestCase):
    def campaign(self) -> dict:
        return validate_campaign(default_campaign(Path.cwd()), Path.cwd(), require_campaign_id=True)

    def valid_report(self) -> dict:
        return build_result(self.campaign())

    def valid_finding(self) -> dict:
        return {
            "finding_id": "FUZZ-001",
            "worker_correlation_id": "template-probe",
            "classification": "confirmed_bug",
            "command": "python3 .just/run_fuzz.py --contract-probe",
            "minimal_template": "{{ value }}",
            "minimal_input": "value=bad",
            "expected_oracle": "reported diagnostic matches its public contract",
            "observed_result": "diagnostic category differed",
            "diagnostic_contract": {
                "expected_status": "rejected",
                "observed_status": "rejected",
                "expected_code_or_category": "expected_code",
                "observed_code_or_category": "wrong_code",
                "expected_message_family": "expected message",
                "observed_message_family": "wrong message",
                "expected_recovery_family": "fix input",
                "observed_recovery_family": "retry later",
                "sensitive_input_leaked": False,
                "field_matches": {
                    "status": True,
                    "code_or_category": False,
                    "message_family": False,
                    "recovery_family": False,
                    "no_sensitive_leak": True,
                },
            },
            "target_invocation": {
                "seam_id": CONTRACT_PROBE_SEAM,
                "mechanism": "counter",
                "invocation_count": 1,
                "evidence_ref": "contract-probe/template-probe#/run_fuzz.validate_worker_result",
            },
            "reproduction_count": 3,
        }

    def test_contract_probe_runs_all_workers_and_validates_end_to_end(self) -> None:
        report = self.valid_report()
        self.assertEqual(report["schema_version"], SCHEMA_VERSION)
        self.assertEqual(report["execution_mode"], "contract-probe")
        self.assertTrue(report["summary"]["all_successful"])
        self.assertEqual([worker["correlation_id"] for worker in report["workers"]], [
            "shape-probe", "template-probe", "boundary-probe", "differential-probe"
        ])
        for worker in report["workers"]:
            proof = worker["target_invocation"]["proofs"]
            self.assertEqual(proof[0]["seam_id"], CONTRACT_PROBE_SEAM)
            self.assertGreater(proof[0]["invocation_count"], 0)
            self.assertEqual(worker["encountered_issues"], [])
        self.assertEqual(validate_report(report, Path.cwd()), report)

    def test_cli_contract_probe_is_a_real_validator_passing_run(self) -> None:
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(main(["run_fuzz.py", "--contract-probe"]), 0)
        self.assertEqual(validate_report(json.loads(output.getvalue()), Path.cwd())["summary"]["failed_workers"], 0)

    def test_v1_report_is_rejected_before_any_silent_upgrade(self) -> None:
        report = self.valid_report()
        report["schema_version"] = "adversarial-fuzzing/v1"
        with self.assertRaisesRegex(FuzzInputError, "v1 reports are not accepted"):
            validate_report(report, Path.cwd())
        historical_v1 = json.loads((FIXTURES / "ai48-report-mixed.json").read_text())
        with self.assertRaisesRegex(FuzzInputError, "v1 reports are not accepted"):
            validate_report(historical_v1, Path.cwd())

    def test_campaign_requires_a_declared_target_seam(self) -> None:
        campaign = default_campaign(Path.cwd())
        del campaign["target_seams"]
        with self.assertRaisesRegex(FuzzInputError, "missing campaign fields: target_seams"):
            validate_campaign(campaign, Path.cwd(), require_campaign_id=True)

    def test_worker_rejects_missing_or_zero_target_invocation_proof(self) -> None:
        report = self.valid_report()
        worker = deepcopy(report["workers"][0])
        del worker["target_invocation"]
        with self.assertRaisesRegex(FuzzInputError, "missing worker result fields: target_invocation"):
            validate_worker_result(worker, report["campaign"])
        worker = deepcopy(report["workers"][0])
        worker["target_invocation"]["proofs"][0]["invocation_count"] = 0
        with self.assertRaisesRegex(FuzzInputError, "invocation_count must be an integer between 1"):
            validate_worker_result(worker, report["campaign"])

    def test_worker_issues_must_be_copied_to_the_final_report(self) -> None:
        report = self.valid_report()
        issue = {
            "issue_id": "FUZZ-ENV-001",
            "case_id": "template-042",
            "category": "environment",
            "observed_evidence": "missing required environment variable",
            "disposition": "deferred",
            "owner": "team-lead",
            "tracking_ref": "https://github.com/randlee/atm-core/issues/1",
            "defer_reason": "requires a platform policy decision",
        }
        report["workers"][0]["encountered_issues"] = [issue]
        report["summary"]["open_campaign_issues"] = 1
        with self.assertRaisesRegex(FuzzInputError, "campaign_issues must contain every worker encountered issue"):
            validate_report(report, Path.cwd())
        report["campaign_issues"] = [{**issue, "worker_correlation_id": "shape-probe"}]
        self.assertEqual(validate_report(report, Path.cwd())["campaign_issues"][0]["issue_id"], "FUZZ-ENV-001")

    def test_diagnostic_contract_is_required_and_mismatch_is_not_cosmetic(self) -> None:
        report = self.valid_report()
        finding = self.valid_finding()
        report["findings"] = [finding]
        report["workers"][1]["finding_ids"] = ["FUZZ-001"]
        report["summary"]["confirmed_bugs"] = 1
        self.assertEqual(validate_report(report, Path.cwd())["findings"][0]["classification"], "confirmed_bug")
        broken = deepcopy(report)
        del broken["findings"][0]["diagnostic_contract"]["expected_recovery_family"]
        with self.assertRaisesRegex(FuzzInputError, "missing finding.diagnostic_contract fields"):
            validate_report(broken, Path.cwd())
        broken = deepcopy(report)
        broken["findings"][0]["classification"] = "intentional_boundary"
        with self.assertRaisesRegex(FuzzInputError, "diagnostic mismatch must be a confirmed_bug"):
            validate_report(broken, Path.cwd())

    def test_diagnostic_delta_requires_the_differential_worker_and_contract_trace(self) -> None:
        report = self.valid_report()
        finding = self.valid_finding()
        finding["classification"] = "intentional_boundary"
        finding["approved_differential_delta"] = {
            "description": "the new contract rejects malformed JSON",
            "contract_trace": "ADR-046",
        }
        report["findings"] = [finding]
        report["workers"][1]["finding_ids"] = ["FUZZ-001"]
        report["summary"]["intentional_boundaries"] = 1
        with self.assertRaisesRegex(FuzzInputError, "valid only for differential-probe"):
            validate_report(report, Path.cwd())

    def test_negative_case_requires_the_full_diagnostic_contract_and_bug_mapping(self) -> None:
        report = self.valid_report()
        negative_case = {
            "case_id": "negative-001",
            "expected_oracle": "malformed input is rejected with the stable diagnostic",
            "observed_result": "malformed input is rejected",
            "diagnostic_contract": {
                "expected_status": "rejected",
                "observed_status": "rejected",
                "expected_code_or_category": "malformed_input",
                "observed_code_or_category": "malformed_input",
                "expected_message_family": "input is malformed",
                "observed_message_family": "input is malformed",
                "expected_recovery_family": "fix input",
                "observed_recovery_family": "fix input",
                "sensitive_input_leaked": False,
                "field_matches": {
                    "status": True,
                    "code_or_category": True,
                    "message_family": True,
                    "recovery_family": True,
                    "no_sensitive_leak": True,
                },
            },
            "target_invocation": {
                "seam_id": CONTRACT_PROBE_SEAM,
                "mechanism": "counter",
                "invocation_count": 1,
                "evidence_ref": "contract-probe/shape-probe#/run_fuzz.validate_worker_result",
            },
            "finding_id": None,
        }
        report["workers"][0]["negative_cases"] = [negative_case]
        self.assertEqual(validate_report(report, Path.cwd())["workers"][0]["negative_cases"][0]["case_id"], "negative-001")
        broken = deepcopy(report)
        broken["workers"][0]["negative_cases"][0]["diagnostic_contract"]["field_matches"]["code_or_category"] = False
        with self.assertRaisesRegex(FuzzInputError, "requires a confirmed finding"):
            validate_report(broken, Path.cwd())

    def test_cli_requires_an_actual_probe_or_external_report(self) -> None:
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            self.assertEqual(main(["run_fuzz.py"]), 2)
        self.assertIn("select exactly one", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
