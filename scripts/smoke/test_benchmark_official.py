"""Contract tests for the unattended official benchmark trigger."""
from __future__ import annotations

import inspect
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from scripts.smoke import benchmark_official as OFFICIAL


def completed(command: list[str], code: int = 0, stdout: str = "", stderr: str = "") -> subprocess.CompletedProcess[str]:
    return subprocess.CompletedProcess(command, code, stdout, stderr)


class OfficialBenchmarkTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        (self.root / ".claude/skills/daemon-switch/scripts").mkdir(parents=True)
        (self.root / "scripts/smoke").mkdir(parents=True)
        (self.root / "site/reports/send-message-benchmark").mkdir(parents=True)
        (self.root / "target/release").mkdir(parents=True)
        self.environment = {"ATM_BENCHMARK_DEPLOY_KEY": "/home/atmbench/.ssh/deploy"}

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def runner(self, callback):
        return OFFICIAL.OfficialBenchmark(
            self.root,
            run=callback,
            environ=self.environment,
            write=lambda _: None,
        )

    @staticmethod
    def healthy_doctor() -> str:
        return json.dumps({"doctor": {"summary": {"status": "healthy"}}})

    def preflight_run(self, *, account: str = "atmbench", doctor: str | None = None, dirty: str = "", ahead: str = ""):
        calls: list[tuple[list[str], dict[str, object]]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append((command, kwargs))
            if command == ["whoami"]:
                return completed(command, stdout=account)
            if command[:2] == ["git", "status"]:
                return completed(command, stdout=dirty)
            if Path(command[0]).name.startswith("python") and command[-2:] == ["status", "--doctor"]:
                return completed(command, stdout=doctor or self.healthy_doctor())
            if command == ["git", "branch", "--show-current"]:
                return completed(command, stdout="integrate/phase-ao2\n")
            if command == ["git", "fetch", "origin"]:
                return completed(command)
            if command == ["git", "rev-list", "origin/integrate/phase-ao2..HEAD"]:
                return completed(command, stdout=ahead)
            if command == ["git", "reset", "--hard", "origin/integrate/phase-ao2"]:
                return completed(command)
            self.fail(f"unexpected command: {command}")

        return self.runner(run), calls

    def test_preflight_rejects_wrong_account_before_touching_git(self) -> None:
        runner, calls = self.preflight_run(account="randlee")
        self.assertEqual(runner.execute(), 2)
        self.assertEqual([command for command, _ in calls], [["whoami"]])

    def test_preflight_reads_doctor_json_instead_of_its_zero_exit_status(self) -> None:
        runner, _ = self.preflight_run(doctor=json.dumps({"doctor": {"error": "offline"}}))
        self.assertEqual(runner.execute(), 2)

    def test_preflight_refuses_dirty_tree_before_sync(self) -> None:
        runner, calls = self.preflight_run(dirty=" M unrelated.txt\n")
        self.assertEqual(runner.execute(), 2)
        self.assertNotIn(["git", "fetch", "origin"], [command for command, _ in calls])

    def test_stranded_rejected_push_names_local_and_remote_commits_and_never_resets(self) -> None:
        calls: list[list[str]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append(command)
            if command == ["whoami"]:
                return completed(command, stdout="atmbench\n")
            if command[:2] == ["git", "status"]:
                return completed(command)
            if Path(command[0]).name.startswith("python"):
                return completed(command, stdout=self.healthy_doctor())
            if command == ["git", "branch", "--show-current"]:
                return completed(command, stdout="integrate/phase-ao2\n")
            if command == ["git", "fetch", "origin"]:
                return completed(command)
            if command == ["git", "rev-list", "origin/integrate/phase-ao2..HEAD"]:
                return completed(command, stdout="abcdef012345\n")
            if command == ["git", "rev-parse", "origin/integrate/phase-ao2"]:
                return completed(command, stdout="fedcba987654\n")
            if command == ["git", "push", "origin", "integrate/phase-ao2"]:
                return completed(command, 1, stderr="non-fast-forward")
            self.fail(f"unexpected command: {command}")

        runner = self.runner(run)
        with self.assertRaisesRegex(OFFICIAL.OfficialBenchmarkError, "abcdef012345.*fedcba987654"):
            runner.preflight()
        self.assertNotIn(["git", "reset", "--hard", "origin/integrate/phase-ao2"], calls)

    def test_stranded_commit_is_pushed_before_hard_sync(self) -> None:
        calls: list[tuple[list[str], dict[str, object]]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append((command, kwargs))
            if command == ["whoami"]:
                return completed(command, stdout="atmbench\n")
            if command[:2] == ["git", "status"]:
                return completed(command)
            if Path(command[0]).name.startswith("python"):
                return completed(command, stdout=self.healthy_doctor())
            if command == ["git", "branch", "--show-current"]:
                return completed(command, stdout="integrate/phase-ao2\n")
            if command == ["git", "fetch", "origin"]:
                return completed(command)
            if command == ["git", "rev-list", "origin/integrate/phase-ao2..HEAD"]:
                return completed(command, stdout="abcdef\n")
            if command == ["git", "rev-parse", "origin/integrate/phase-ao2"]:
                return completed(command, stdout="fedcba\n")
            if command == ["git", "push", "origin", "integrate/phase-ao2"]:
                return completed(command)
            if command == ["git", "reset", "--hard", "origin/integrate/phase-ao2"]:
                return completed(command)
            self.fail(f"unexpected command: {command}")

        self.assertEqual(self.runner(run).preflight(), "integrate/phase-ao2")
        commands = [command for command, _ in calls]
        self.assertLess(commands.index(["git", "push", "origin", "integrate/phase-ao2"]), commands.index(["git", "reset", "--hard", "origin/integrate/phase-ao2"]))
        push_kwargs = calls[commands.index(["git", "push", "origin", "integrate/phase-ao2"])][1]
        self.assertIn("-i /home/atmbench/.ssh/deploy", push_kwargs["env"]["GIT_SSH_COMMAND"])

    def test_build_refuses_missing_release_binaries_before_measurement(self) -> None:
        runner = self.runner(lambda command, **kwargs: completed(command))
        with self.assertRaisesRegex(OFFICIAL.OfficialBenchmarkError, "did not produce required"):
            runner.build_and_sign()

    def test_build_requires_cargo_signing_and_both_release_binaries(self) -> None:
        binary_runner = self.runner(lambda command, **kwargs: completed(command))
        for binary in binary_runner.release_binaries():
            binary.touch()
        calls: list[list[str]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append(command)
            return completed(command)

        runner = self.runner(run)
        runner.build_and_sign()
        self.assertEqual(calls[0], ["cargo", "build", "--release", "-p", "agent-team-mail", "-p", "atm-daemon"])
        self.assertTrue(calls[1][-1].endswith(".just/sign_daemon_dev.py"))

    def test_green_campaign_with_publish_failure_is_infrastructure_error(self) -> None:
        runner = self.runner(lambda command, **kwargs: completed(command))
        with (
            mock.patch.object(runner, "preflight", return_value="integrate/phase-ao2"),
            mock.patch.object(runner, "build_and_sign"),
            mock.patch.object(runner, "measure", return_value=OFFICIAL.OfficialOutcome(False, "green")),
            mock.patch.object(runner, "render_publish_and_push", return_value="benchmark publication failed"),
        ):
            self.assertEqual(runner.execute(), 2)

    def test_measured_fail_keeps_exit_one_when_rebuild_or_publish_fails(self) -> None:
        runner = self.runner(lambda command, **kwargs: completed(command))
        with (
            mock.patch.object(runner, "preflight", return_value="integrate/phase-ao2"),
            mock.patch.object(runner, "build_and_sign"),
            mock.patch.object(runner, "measure", return_value=OFFICIAL.OfficialOutcome(True, "tcp p50=1 floor=2")),
            mock.patch.object(runner, "render_publish_and_push", return_value="report rebuild failed"),
        ):
            self.assertEqual(runner.execute(), 1)

    def test_failure_summary_names_target_p50_and_floor(self) -> None:
        campaign = {
            "results": [{
                "target": "tcp-tls", "status": "FAIL",
                "metrics": {"admissions_per_second": {"p50": 13554.63}},
                "baseline": {"p50_floor": 17500.0},
            }],
        }
        path = self.root / "site/reports/send-message-benchmark/campaign.campaign.json"
        path.write_text(json.dumps(campaign), encoding="utf-8")
        summary = self.runner(lambda command, **kwargs: completed(command)).failure_summary()
        self.assertIn("tcp-tls p50=13554.63 floor=17500.00", summary)

    def test_official_path_has_no_interactive_read_calls(self) -> None:
        source = inspect.getsource(OFFICIAL)
        self.assertNotIn("input(", source)
        self.assertNotIn("getpass", source)

    def test_launchd_template_carries_non_login_push_environment(self) -> None:
        template = (Path(__file__).resolve().parents[2] / "tools/com.atm.benchmark-official.plist").read_text(
            encoding="utf-8"
        )
        self.assertIn("GIT_SSH_COMMAND", template)
        self.assertIn("IdentitiesOnly=yes", template)
        self.assertIn("BatchMode=yes", template)
        self.assertIn("PYO3_PYTHON", template)
        self.assertIn("ATM_SIGNING_IDENTITY", template)


if __name__ == "__main__":
    unittest.main()
