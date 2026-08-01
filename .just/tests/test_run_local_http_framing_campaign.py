from __future__ import annotations

from pathlib import Path
import importlib.util
import sys
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


class LocalHttpFramingCampaignTests(unittest.TestCase):
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
        self.assertEqual(payload["workers"][1]["cases_run"], 36)
        self.assertEqual(len(payload["workers"][1]["attempts"]), 3)
        self.assertEqual(payload["outcome_ledger"]["confirmed_bug"], [])
        self.assertEqual(len(payload["outcome_ledger"]["non_repro"]), 1)
        self.assertEqual(len(payload["outcome_ledger"]["benign"]), 2)
        self.assertEqual(len(payload["outcome_ledger"]["inconclusive"]), 1)

    def test_failed_reader_probe_is_retained_as_confirmed_bug_evidence(self) -> None:
        attempts = [successful_attempt("ok") for _ in range(8)]
        for index in (1, 2, 3):
            attempts[index] = {
            "ok": False,
            "timed_out": False,
            "command": ["cargo", "test", "bad"],
            "stdout": "",
            "stderr": "reader mismatch",
            }
        with mock.patch.object(campaign, "run_test", side_effect=attempts), mock.patch.object(
            campaign, "cpu_features", return_value=[]
        ):
            payload = campaign.build_campaign(17, 4, 30, "baseline-sha")

        candidate = payload["workers"][1]
        self.assertEqual(candidate["status"], "failed")
        self.assertEqual(candidate["classification"], "confirmed_bug")
        self.assertEqual(candidate["finding_ids"], ["AI51-TEMPLATE-PROBE-001"])
        self.assertEqual(len(payload["outcome_ledger"]["confirmed_bug"]), 1)

    def test_cli_renders_the_campaign_and_respects_validation_bounds(self) -> None:
        with mock.patch.object(campaign, "build_campaign", return_value={"session_id": "ai51-test"}) as build, mock.patch.object(
            campaign, "render_campaign", return_value={"output_path": "site/reports/test.html"}
        ) as render, mock.patch.object(sys, "stdout") as stdout:
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


if __name__ == "__main__":
    unittest.main()
