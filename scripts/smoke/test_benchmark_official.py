"""Contract tests for the unattended official benchmark trigger."""
from __future__ import annotations

import inspect
import json
import os
from pathlib import Path
import subprocess
import sys
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

    def preflight_run(self, *, account: str = "atmbench", dirty: str = "", ahead: str = ""):
        calls: list[tuple[list[str], dict[str, object]]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append((command, kwargs))
            if command == ["whoami"]:
                return completed(command, stdout=account)
            if command[:1] in (["pkill"], ["pgrep"]):
                return completed(command, 1)
            if command[:2] == ["git", "status"]:
                return completed(command, stdout=dirty)
            if command == ["git", "branch", "--show-current"]:
                return completed(command, stdout="integrate/phase-ao2\n")
            if command == ["git", "fetch", "origin"]:
                return completed(command)
            if command == ["git", "rev-list", "origin/integrate/phase-ao2..HEAD"]:
                return completed(command, stdout=ahead)
            if command == ["git", "reset", "--hard", "origin/integrate/phase-ao2"]:
                return completed(command)
            self.fail(f"unexpected command: {command}")

        runner = self.runner(run)
        runner.reset_disposable_account = mock.Mock()
        return runner, calls

    def test_preflight_rejects_wrong_account_before_touching_git(self) -> None:
        runner, calls = self.preflight_run(account="randlee")
        self.assertEqual(runner.execute(), 2)
        self.assertEqual([command for command, _ in calls], [["whoami"]])

    def test_preflight_does_not_require_or_inspect_an_ambient_daemon(self) -> None:
        runner, calls = self.preflight_run()
        self.assertEqual(runner.preflight(), "integrate/phase-ao2")
        self.assertFalse(any("daemon-switch.py" in " ".join(command) for command, _ in calls))

    def test_preflight_resets_the_disposable_account_before_git_sync(self) -> None:
        runner, calls = self.preflight_run()
        runner.preflight()
        runner.reset_disposable_account.assert_called_once_with()
        commands = [command for command, _ in calls]
        self.assertLess(commands.index(["whoami"]), commands.index(["git", "status", "--porcelain"]))

    def test_reset_kills_a_remaining_account_daemon_before_database_cleanup(self) -> None:
        calls: list[list[str]] = []
        pgrep_results = iter((completed(["pgrep"], stdout="42\n"), completed(["pgrep"], 1)))

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append(command)
            if command[:1] == ["pkill"]:
                return completed(command)
            if command == ["pgrep", "-x", "atm-daemon"]:
                return next(pgrep_results)
            self.fail(f"unexpected command: {command}")

        runner = self.runner(run)
        with mock.patch.object(OFFICIAL, "clear_benchmark_database_state") as clear:
            runner.reset_disposable_account()
        self.assertEqual(
            calls,
            [
                ["pkill", "-TERM", "-x", "atm-daemon"],
                ["pgrep", "-x", "atm-daemon"],
                ["pkill", "-KILL", "-x", "atm-daemon"],
                ["pgrep", "-x", "atm-daemon"],
            ],
        )
        clear.assert_called_once_with()

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
        runner.reset_disposable_account = mock.Mock()
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

        runner = self.runner(run)
        runner.reset_disposable_account = mock.Mock()
        self.assertEqual(runner.preflight(), "integrate/phase-ao2")
        commands = [command for command, _ in calls]
        self.assertLess(commands.index(["git", "push", "origin", "integrate/phase-ao2"]), commands.index(["git", "reset", "--hard", "origin/integrate/phase-ao2"]))
        push_kwargs = calls[commands.index(["git", "push", "origin", "integrate/phase-ao2"])][1]
        self.assertIn("-i /home/atmbench/.ssh/deploy", push_kwargs["env"]["GIT_SSH_COMMAND"])

    def test_stranded_unreachable_remote_is_distinct_and_never_resets(self) -> None:
        calls: list[list[str]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append(command)
            if command == ["whoami"]:
                return completed(command, stdout="atmbench\n")
            if command[:2] == ["git", "status"]:
                return completed(command)
            if command == ["git", "branch", "--show-current"]:
                return completed(command, stdout="integrate/phase-ao2\n")
            if command == ["git", "fetch", "origin"]:
                return completed(command)
            if command == ["git", "rev-list", "origin/integrate/phase-ao2..HEAD"]:
                return completed(command, stdout="stranded-sha\n")
            if command == ["git", "rev-parse", "origin/integrate/phase-ao2"]:
                return completed(command, stdout="remote-sha\n")
            if command == ["git", "push", "origin", "integrate/phase-ao2"]:
                return completed(command, 128, stderr="Could not resolve host: github.com")
            self.fail(f"unexpected command: {command}")

        runner = self.runner(run)
        runner.reset_disposable_account = mock.Mock()
        with self.assertRaisesRegex(OFFICIAL.OfficialBenchmarkError, "stranded-sha.*Could not resolve host"):
            runner.preflight()
        self.assertFalse(any(command[:3] == ["git", "reset", "--hard"] for command in calls))

    def test_explicit_branch_override_is_used_for_premerge_validation(self) -> None:
        calls: list[list[str]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append(command)
            if command == ["whoami"]:
                return completed(command, stdout="atmbench\n")
            if command[:2] == ["git", "status"]:
                return completed(command)
            if command == ["git", "fetch", "origin"]:
                return completed(command)
            if command == ["git", "rev-list", "origin/feature/review..HEAD"]:
                return completed(command)
            if command == ["git", "reset", "--hard", "origin/feature/review"]:
                return completed(command)
            self.fail(f"unexpected command: {command}")

        runner = self.runner(run)
        runner.reset_disposable_account = mock.Mock()
        runner.branch_override = "feature/review"
        self.assertEqual(runner.preflight(), "feature/review")
        self.assertNotIn(["git", "branch", "--show-current"], calls)

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

    def test_execute_exercises_real_build_failure_before_measurement(self) -> None:
        calls: list[list[str]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append(command)
            return completed(command)

        runner = self.runner(run)
        with (
            mock.patch.object(runner, "preflight", return_value="integrate/phase-ao2"),
            mock.patch.object(runner, "measure") as measure,
        ):
            self.assertEqual(runner.execute(), 2)
        self.assertEqual(calls[0], ["cargo", "build", "--release", "-p", "agent-team-mail", "-p", "atm-daemon"])
        measure.assert_not_called()

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

    def test_measured_fail_keeps_exit_one_when_post_measurement_step_raises(self) -> None:
        runner = self.runner(lambda command, **kwargs: completed(command))
        with (
            mock.patch.object(runner, "preflight", return_value="integrate/phase-ao2"),
            mock.patch.object(runner, "build_and_sign"),
            mock.patch.object(runner, "measure", return_value=OFFICIAL.OfficialOutcome(True, "tcp p50=1 floor=2")),
            mock.patch.object(runner, "render_publish_and_push", side_effect=OFFICIAL.OfficialBenchmarkError("push failed")),
            mock.patch.object(runner, "notify_team_lead") as notify,
        ):
            self.assertEqual(runner.execute(), 1)
        notify.assert_called_once_with("tcp p50=1 floor=2")

    def test_measured_fail_notifies_team_lead_but_notification_cannot_change_exit_one(self) -> None:
        runner = self.runner(lambda command, **kwargs: completed(command, 1, stderr="offline"))
        with (
            mock.patch.object(runner, "preflight", return_value="integrate/phase-ao2"),
            mock.patch.object(runner, "build_and_sign"),
            mock.patch.object(runner, "measure", return_value=OFFICIAL.OfficialOutcome(True, "tcp p50=1.00 floor=2.00")),
            mock.patch.object(runner, "render_publish_and_push", return_value=None),
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

    def test_actual_script_completes_with_stdin_at_dev_null(self) -> None:
        environment = dict(os.environ)
        environment.update({"HOME": self.temporary.name, "ATM_OFFICIAL_ACCOUNT": "not-the-current-user"})
        result = subprocess.run(
            [sys.executable, str(Path(OFFICIAL.__file__))],
            cwd=self.root,
            env=environment,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("official runs require account", result.stdout)
        self.assertTrue((self.root / "benchmark-logs").is_dir())

    def test_launchd_template_carries_non_login_push_environment(self) -> None:
        template = (Path(__file__).resolve().parents[2] / "tools/com.atm.benchmark-official.plist").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("GIT_SSH_COMMAND", template)
        self.assertIn("ATM_BENCHMARK_DEPLOY_KEY", template)
        self.assertIn("__ATM_HOME__/.local/bin:/opt/homebrew/bin", template)
        self.assertIn("PYO3_PYTHON", template)
        self.assertIn("ATM_SIGNING_IDENTITY", template)

    def test_runbook_documents_ssh_and_safe_launchd_installation(self) -> None:
        runbook = (Path(__file__).resolve().parents[2] / ".claude/skills/benchmark-run/SKILL.md").read_text(
            encoding="utf-8"
        )
        self.assertIn("ssh atmbench@rand-m5.local", runbook)
        self.assertIn("$HOME/.local/bin:/opt/homebrew/bin", runbook)
        self.assertIn("just benchmark-official", runbook)
        self.assertIn("ssh-keyscan -H github.com", runbook)
        self.assertIn("launchctl bootstrap", runbook)
        self.assertIn("launchctl bootout", runbook)


if __name__ == "__main__":
    unittest.main()
