from __future__ import annotations

from contextlib import redirect_stdout
import io
from pathlib import Path
import json
import sys
import tempfile
import unittest
from unittest.mock import patch


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_fuzz import FuzzInputError
from run_fuzz import build_result
from run_fuzz import main
from run_fuzz import validate_campaign
from run_fuzz import validate_worker_result


FIXTURES = JUST_DIR / "fixtures/fuzz"


class FuzzRunnerTests(unittest.TestCase):
    def campaign_fixture(self, name: str) -> dict:
        payload = json.loads((FIXTURES / name).read_text())
        # Keep the fixture repository-independent: CI checks out to a different
        # absolute path on every host, while the contract requires an absolute
        # approved worktree path.
        if "worktree_path" in payload:
            payload["worktree_path"] = str(Path.cwd())
        return payload

    def test_success_campaign_is_deterministic_and_four_workers(self) -> None:
        campaign = validate_campaign(self.campaign_fixture("success.json"), Path.cwd())
        first = build_result(campaign)
        second = build_result(campaign)
        self.assertEqual(first, second)
        self.assertEqual([item["correlation_id"] for item in first["workers"]], [
            "shape-probe", "template-probe", "boundary-probe", "differential-probe"
        ])
        self.assertEqual(first["schema_version"], "adversarial-fuzzing/v1")

    def test_timeout_result_is_preserved_as_structured_failure(self) -> None:
        result = json.loads((FIXTURES / "timeout.json").read_text())
        validated = validate_worker_result(result)
        self.assertEqual(validated["status"], "timed_out")
        self.assertEqual(validated["error"]["code"], "worker_timeout")

    def test_malformed_result_fails_closed(self) -> None:
        result = json.loads((FIXTURES / "malformed-result.json").read_text())
        with self.assertRaisesRegex(FuzzInputError, "missing worker result fields"):
            validate_worker_result(result)

    def test_unsafe_worktree_fails_closed(self) -> None:
        payload = json.loads((FIXTURES / "unsafe-path.json").read_text())
        with tempfile.TemporaryDirectory() as tempdir:
            # The fixture uses a rooted UNC spelling, which pathlib recognizes
            # as absolute on both POSIX and Windows while remaining outside
            # this temporary repository.
            with self.assertRaisesRegex(FuzzInputError, "inside the repository"):
                validate_campaign(payload, Path(tempdir))

    def test_unknown_target_fails_closed(self) -> None:
        payload = self.campaign_fixture("success.json")
        payload["target"] = "unknown-target"
        with self.assertRaisesRegex(FuzzInputError, "target must be one of"):
            validate_campaign(payload, Path.cwd())

    def test_cli_forwards_dry_run_flag(self) -> None:
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(main(["run_fuzz.py"]), 0)
        self.assertEqual(json.loads(output.getvalue())["execution_mode"], "contract-only")

        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(main(["run_fuzz.py", "--dry-run"]), 0)
        self.assertEqual(json.loads(output.getvalue())["execution_mode"], "dry-run")

    def test_worker_cap_rejects_more_than_four(self) -> None:
        payload = self.campaign_fixture("success.json")
        payload["max_workers"] = 5
        with self.assertRaisesRegex(FuzzInputError, "max_workers"):
            validate_campaign(payload, Path.cwd())

    def test_local_http_framing_target_selects_all_contract_workers(self) -> None:
        payload = self.campaign_fixture("success.json")
        payload["target"] = "local-http-framing"
        campaign = validate_campaign(payload, Path.cwd())
        self.assertEqual([worker["correlation_id"] for worker in build_result(campaign)["workers"]], [
            "shape-probe", "template-probe", "boundary-probe", "differential-probe"
        ])

    def test_checked_emission_target_selects_all_contract_workers(self) -> None:
        payload = self.campaign_fixture("success.json")
        payload["target"] = "atm-template-checked-emission"
        campaign = validate_campaign(payload, Path.cwd())
        self.assertEqual([worker["correlation_id"] for worker in build_result(campaign)["workers"]], [
            "shape-probe", "template-probe", "boundary-probe", "differential-probe"
        ])

    @patch("run_fuzz.subprocess.run")
    def test_checked_emission_execution_runs_only_fixed_worker_contracts(self, run: object) -> None:
        run.return_value.returncode = 0  # type: ignore[attr-defined]
        payload = self.campaign_fixture("success.json")
        payload["target"] = "atm-template-checked-emission"
        campaign = validate_campaign(payload, Path.cwd())
        result = build_result(campaign, dry_run=False, execute=True)
        self.assertEqual(result["execution_mode"], "executed")
        self.assertTrue(result["summary"]["all_successful"])
        self.assertEqual(run.call_count, 4)  # type: ignore[attr-defined]
        for call in run.call_args_list:  # type: ignore[attr-defined]
            self.assertEqual(call.args[0][0:3], ("cargo", "test", "-p"))
            self.assertNotIn("sc-compose", call.args[0])

    @patch("run_fuzz.subprocess.run", side_effect=__import__("subprocess").TimeoutExpired("cargo", 120))
    def test_checked_emission_timeout_is_a_structured_candidate(self, _run: object) -> None:
        payload = self.campaign_fixture("success.json")
        payload["target"] = "atm-template-checked-emission"
        campaign = validate_campaign(payload, Path.cwd())
        result = build_result(campaign, dry_run=False, execute=True)
        self.assertEqual(result["summary"]["failed_workers"], 4)
        self.assertTrue(all(worker["error"]["code"] == "worker_timeout" for worker in result["workers"]))

    def test_real_execution_is_fail_closed_for_unapproved_targets(self) -> None:
        campaign = validate_campaign(self.campaign_fixture("success.json"), Path.cwd())
        with self.assertRaisesRegex(FuzzInputError, "only for atm-template-checked-emission"):
            build_result(campaign, dry_run=False, execute=True)

    def test_checked_emission_requires_the_full_bounded_campaign_shape(self) -> None:
        payload = self.campaign_fixture("success.json")
        payload["target"] = "atm-template-checked-emission"
        payload["max_workers"] = 3
        with self.assertRaisesRegex(FuzzInputError, "exactly four workers"):
            validate_campaign(payload, Path.cwd())
        payload["max_workers"] = 4
        payload["cases_per_worker"] = 99
        with self.assertRaisesRegex(FuzzInputError, "at least 100 cases"):
            validate_campaign(payload, Path.cwd())
        payload["cases_per_worker"] = 100
        payload["per_worker_timeout_s"] = 119
        with self.assertRaisesRegex(FuzzInputError, "120-second"):
            validate_campaign(payload, Path.cwd())


if __name__ == "__main__":
    unittest.main()
