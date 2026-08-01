from __future__ import annotations

from pathlib import Path
import importlib.util
import json
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "fuzz" / "run_local_http_framing_campaign.py"
SPEC = importlib.util.spec_from_file_location("run_local_http_framing_campaign", SCRIPT)
assert SPEC and SPEC.loader
campaign = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = campaign
SPEC.loader.exec_module(campaign)


def successful_attempt(name: str) -> dict:
    return {
        "ok": True,
        "timed_out": False,
        "command": ["cargo", "test", name],
        "stdout": "",
        "stderr": "",
    }


def failed_attempt(name: str, *, timed_out: bool = False, case_index: int | None = None) -> dict:
    return {
        "ok": False,
        "timed_out": timed_out,
        "command": ["cargo", "test", name],
        "stdout": "" if case_index is None else f"AI51 case {case_index}",
        "stderr": "reader mismatch",
    }


class LocalHttpFramingCampaignTests(unittest.TestCase):
    def test_command_for_accepts_only_the_test_name(self) -> None:
        self.assertEqual(campaign.command_for("shape-probe")[0:2], ["cargo", "test"])

    def test_build_campaign_uses_real_reader_probe_contract_and_all_outcomes(self) -> None:
        with mock.patch.object(campaign, "run_test", side_effect=lambda name, *_: successful_attempt(name)), mock.patch.object(
            campaign, "cpu_features", return_value=["neon"]
        ):
            payload = campaign.build_campaign(17, 12, 30, "baseline-sha")

        self.assertEqual(payload["schema_version"], "adversarial-fuzzing/v1")
        self.assertEqual(payload["campaign"]["seed"], 17)
        self.assertEqual(payload["campaign"]["max_workers"], 4)
        self.assertEqual(payload["campaign"]["cpu_features"], ["neon"])
        self.assertEqual([worker["correlation_id"] for worker in payload["workers"]], [
            "shape-probe", "template-probe", "boundary-probe", "differential-probe"
        ])
        self.assertEqual(payload["workers"][1]["cases_run"], 12)
        self.assertEqual(len(payload["workers"][1]["attempts"]), 1)
        self.assertEqual(payload["outcome_ledger"]["confirmed_bug"], [])
        self.assertEqual(payload["outcome_ledger"]["non_repro"], [])
        self.assertEqual(len(payload["outcome_ledger"]["benign"]), 4)
        self.assertEqual(payload["outcome_ledger"]["inconclusive"], [])

    def test_failed_reader_probe_is_retained_as_confirmed_bug_evidence(self) -> None:
        attempts = [
            successful_attempt("ok"),
            failed_attempt("bad", case_index=3),
            failed_attempt("bad"),
            failed_attempt("bad"),
            failed_attempt("bad"),
            failed_attempt("bad"),
            successful_attempt("ok"),
            successful_attempt("ok"),
        ]
        with mock.patch.object(campaign, "run_test", side_effect=attempts), mock.patch.object(
            campaign, "cpu_features", return_value=[]
        ):
            payload = campaign.build_campaign(17, 4, 30, "baseline-sha")

        candidate = payload["workers"][1]
        self.assertEqual(candidate["status"], "failed")
        self.assertEqual(candidate["classification"], "confirmed_bug")
        self.assertEqual(candidate["finding_ids"], ["AI51-TEMPLATE-PROBE-001"])
        self.assertEqual(len(payload["outcome_ledger"]["confirmed_bug"]), 1)

    def test_candidate_is_replayed_exactly_three_times_and_retained_as_non_repro(self) -> None:
        attempts = [
            {**failed_attempt("candidate", case_index=4), "stage": "initial", "cases": 7, "case_start": 0},
            {**failed_attempt("candidate"), "stage": "minimize", "cases": 1, "case_start": 4},
            {**successful_attempt("candidate"), "stage": "replay", "cases": 1, "case_start": 4},
            {**successful_attempt("candidate"), "stage": "replay", "cases": 1, "case_start": 4},
            {**successful_attempt("candidate"), "stage": "replay", "cases": 1, "case_start": 4},
        ]
        candidate = campaign.worker(
            "template-probe",
            "candidate replay",
            attempts,
            7,
            "candidate benign summary",
        )

        self.assertEqual(candidate["status"], "success")
        self.assertEqual(candidate["classification"], "pass")
        self.assertEqual(candidate["candidate_outcome"], "non_repro")
        self.assertEqual(candidate["cases_run"], 11)
        self.assertEqual(candidate["passed"], 3)
        self.assertEqual(candidate["failed"], 8)
        self.assertIsNone(candidate["error"])
        self.assertEqual(candidate["finding_ids"], ["AI51-TEMPLATE-PROBE-001"])

    def test_run_probe_replays_only_observed_candidates(self) -> None:
        with mock.patch.object(
            campaign,
            "run_test",
            side_effect=[
                failed_attempt("candidate", case_index=42),
                failed_attempt("candidate"),
                successful_attempt("candidate"),
                successful_attempt("candidate"),
                successful_attempt("candidate"),
            ],
        ) as run:
            attempts = campaign.run_probe("candidate", 17, 5, 30)
        self.assertEqual(len(attempts), 5)
        self.assertEqual(run.call_count, 5)
        self.assertEqual([attempt["stage"] for attempt in attempts], ["initial", "minimize", "replay", "replay", "replay"])
        self.assertTrue(all(call.args[4] == 42 for call in run.call_args_list[1:]))

    def test_unminimizable_candidate_remains_inconclusive_not_pass(self) -> None:
        candidate = campaign.worker(
            "shape-probe",
            "fragmented request",
            [{**failed_attempt("candidate"), "stage": "initial", "cases": 7, "case_start": 0}],
            7,
            "candidate benign summary",
        )
        ledger = campaign.outcome_ledger([candidate])

        self.assertEqual(candidate["classification"], "inconclusive")
        self.assertEqual(candidate["candidate_outcome"], "inconclusive")
        self.assertEqual(candidate["finding_ids"], ["AI51-SHAPE-PROBE-001"])
        self.assertEqual(len(ledger["inconclusive"]), 1)

    def test_cli_renders_the_campaign_and_respects_validation_bounds(self) -> None:
        payload = {
            "session_id": "ai51-test",
            "campaign": {},
        }
        with mock.patch.object(campaign, "build_campaign", return_value=payload) as build, mock.patch.object(
            campaign, "render_campaign", return_value={"output_path": "site/reports/test.html"}
        ) as render, mock.patch.object(campaign, "validate_with_ai48_contract", return_value={"workflow": "just fuzz"}), mock.patch.object(sys, "stdout") as stdout:
            result = campaign.main([
                "campaign",
                "--seed", "19",
                "--cases", "8",
                "--timeout-seconds", "30",
                "--stem", "20260801-ai51-test",
                "--no-index",
            ])

        self.assertEqual(result, 0)
        build.assert_called_once_with(19, 8, 30, "integrate/phase-ai-31-33")
        self.assertTrue(render.call_args.kwargs["invoke_index"] is False)
        written = "".join(call.args[0] for call in stdout.write.call_args_list)
        self.assertIn("site/reports/test.html", written)

    def test_repeat_comparison_requires_matching_classifications(self) -> None:
        payload = {
            "campaign": {"target": "local-http-framing", "baseline_ref": "base", "seed": 17, "cases_per_worker": 8},
            "workers": [{"correlation_id": "shape-probe", "candidate_outcome": "benign", "classification": "pass"}],
        }
        previous = {
            "campaign": payload["campaign"],
            "sections": [{"id": "shape-probe", "json_payload": payload["workers"][0]}],
        }
        with tempfile.TemporaryDirectory() as tempdir:
            sidecar = Path(tempdir) / "previous.json"
            sidecar.write_text(json.dumps(previous), encoding="utf-8")
            comparison = campaign.compare_repeat(sidecar, payload)
        self.assertTrue(comparison["classifications_match"])

    def test_ai48_contract_requires_the_four_local_framing_workers(self) -> None:
        payload = {
            "campaign": {
                "worktree_path": str(ROOT), "target": "local-http-framing", "baseline_ref": "base", "seed": 17,
                "max_workers": 4, "cases_per_worker": 8, "per_worker_timeout_s": 30,
                "promote_regressions": True, "notes": "test",
            }
        }
        stdout = json.dumps({
            "schema_version": "adversarial-fuzzing/v1",
            "workers": [{"correlation_id": worker} for worker in ["shape-probe", "template-probe", "boundary-probe", "differential-probe"]],
        })
        with mock.patch.object(campaign.subprocess, "run", return_value=mock.Mock(returncode=0, stdout=stdout, stderr="")):
            result = campaign.validate_with_ai48_contract(payload)
        self.assertEqual(result["workflow"], "just fuzz")

    def test_cli_reserves_capacity_for_minimization_and_three_replays(self) -> None:
        with self.assertRaises(SystemExit) as raised:
            campaign.main(["campaign", "--cases", "997", "--stem", "20260801-invalid"])
        self.assertEqual(raised.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
