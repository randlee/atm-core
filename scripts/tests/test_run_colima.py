"""Tests for the thin prompt-driven colima integration driver."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from scripts.integration import run_colima as driver


class DriverTests(unittest.TestCase):
    def run_prompt(self, missing: bool = False) -> tuple[dict[str, object], str]:
        report = {
            "finished_at": "2026-09-14T00:01:00Z",
            "atm_versions": {"atm": "1.5.17"},
            "verdict": "pass",
            "steps": [{"name": "observable", "status": "pass", "detail": "public CLI"}],
        }
        with tempfile.TemporaryDirectory() as temp, mock.patch.object(driver, "command") as command:
            step_dir = Path(temp)

            def invoke(argv, *_args, **_kwargs):
                if argv[:2] == ["docker", "cp"] and not missing:
                    Path(argv[-1]).write_text(json.dumps(report), encoding="utf-8")
                return mock.Mock(returncode=int(missing and argv[:2] == ["docker", "cp"]), stdout="out\n", stderr="err\n")

            command.side_effect = invoke
            payload = driver.run_prompt("AT9", "task-start", step_dir, "a" * 40)
            return payload, (step_dir / "run-prompts.log").read_text()

    def test_prompt_report_becomes_step_payload_with_raw_log(self) -> None:
        payload, log = self.run_prompt()
        self.assertEqual((payload["status"], payload["cases"][0]["status"]), ("PASS", "pass"))
        self.assertEqual(log, "out\nerr\n")

    def test_missing_prompt_report_is_one_failure(self) -> None:
        payload, log = self.run_prompt(missing=True)
        self.assertEqual(payload["status"], "FAIL")
        self.assertEqual(len(payload["cases"]), 1)
        self.assertEqual(log, "out\nerr\nout\nerr\n")


if __name__ == "__main__":
    unittest.main()
