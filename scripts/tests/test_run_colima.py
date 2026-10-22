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
            payload = driver.run_prompt("AT9", "task-start", step_dir, "a" * 40, "1.6.5")
            return payload, (step_dir / "run-prompts.log").read_text()

    def test_prompt_report_becomes_step_payload_with_raw_log(self) -> None:
        payload, log = self.run_prompt()
        self.assertEqual((payload["status"], payload["cases"][0]["status"]), ("PASS", "pass"))
        self.assertEqual(payload["atm_version"], "1.6.5")
        self.assertEqual(log, "out\nerr\n")

    def test_missing_prompt_report_is_one_failure(self) -> None:
        payload, log = self.run_prompt(missing=True)
        self.assertEqual(payload["status"], "FAIL")
        self.assertEqual(len(payload["cases"]), 1)
        self.assertEqual(log, "out\nerr\nout\nerr\n")


class InstalledVersionTests(unittest.TestCase):
    def test_reads_semver_from_container_binary(self) -> None:
        with mock.patch.object(driver, "command", return_value=mock.Mock(returncode=0, stdout="atm 1.6.5\n", stderr="")) as command:
            self.assertEqual(driver.installed_atm_version(), "1.6.5")
        command.assert_called_once_with(["docker", "exec", driver.CONTAINER, "atm", "--version"], timeout=60)

    def test_rejects_unparseable_container_output(self) -> None:
        with mock.patch.object(driver, "command", return_value=mock.Mock(returncode=0, stdout="unknown\n", stderr="")):
            with self.assertRaises(driver.ColimaVersionError):
                driver.installed_atm_version()

    def test_run_reads_version_after_testbed_installs_the_container(self) -> None:
        order: list[str] = []
        with (
            tempfile.TemporaryDirectory() as temp,
            mock.patch.object(driver, "command") as command,
            mock.patch.object(driver, "installed_atm_version", side_effect=lambda: order.append("version") or "1.6.5"),
            mock.patch.object(driver, "run_prompt", return_value={}),
            mock.patch.object(driver, "write_step"),
            mock.patch.object(driver.render_colima, "render_run", return_value={}),
        ):
            def invoke(argv, *_args, **_kwargs):
                if argv[-1].endswith("test.sh"):
                    order.append("test.sh")
                return mock.Mock(returncode=0, stdout="", stderr="")

            command.side_effect = invoke
            driver.run(Path(temp), Path(temp))
        self.assertEqual(order, ["test.sh", "version"])


if __name__ == "__main__":
    unittest.main()
