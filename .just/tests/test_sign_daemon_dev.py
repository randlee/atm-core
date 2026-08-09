from __future__ import annotations

from pathlib import Path
import importlib.util
from unittest import mock
import subprocess
import sys
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "sign_daemon_dev.py"
SPEC = importlib.util.spec_from_file_location("sign_daemon_dev", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
sign_daemon_dev = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = sign_daemon_dev
SPEC.loader.exec_module(sign_daemon_dev)


class SignDaemonDevTests(unittest.TestCase):
    def test_non_macos_is_a_silent_noop(self) -> None:
        with mock.patch.object(sign_daemon_dev.sys, "platform", "linux"), mock.patch.object(
            sign_daemon_dev.subprocess, "run"
        ) as run:
            self.assertEqual(sign_daemon_dev.main(), 0)
        run.assert_not_called()

    def test_missing_identity_is_a_silent_noop(self) -> None:
        security = subprocess.CompletedProcess(
            ["security"], 0, stdout="     0 valid identities found\n", stderr=""
        )
        with mock.patch.object(sign_daemon_dev.sys, "platform", "darwin"), mock.patch.object(
            sign_daemon_dev.subprocess, "run", return_value=security
        ) as run:
            self.assertEqual(sign_daemon_dev.main(), 0)
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args.args[0], ["security", "find-identity", "-v", "-p", "codesigning"])

    def test_exact_identity_signs_each_existing_daemon_binary(self) -> None:
        security = subprocess.CompletedProcess(
            ["security"], 0, stdout='  1) ABCD "atm-daemon-dev"\n', stderr=""
        )
        codesign = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        with mock.patch.object(sign_daemon_dev.sys, "platform", "darwin"), mock.patch.object(
            sign_daemon_dev.subprocess, "run", side_effect=[security, codesign, codesign]
        ) as run, mock.patch.object(
            sign_daemon_dev.Path, "is_file", side_effect=[True, True]
        ):
            self.assertEqual(sign_daemon_dev.main(), 0)

        commands = [call.args[0] for call in run.call_args_list]
        self.assertEqual(commands[0], ["security", "find-identity", "-v", "-p", "codesigning"])
        self.assertEqual(
            commands[1:],
            [
                ["codesign", "-s", "atm-daemon-dev", "--force", str(sign_daemon_dev.DAEMON_TARGETS[0])],
                ["codesign", "-s", "atm-daemon-dev", "--force", str(sign_daemon_dev.DAEMON_TARGETS[1])],
            ],
        )

    def test_similar_identity_name_does_not_match(self) -> None:
        security = subprocess.CompletedProcess(
            ["security"], 0, stdout='  1) ABCD "atm-daemon-dev-old"\n', stderr=""
        )
        with mock.patch.object(sign_daemon_dev.sys, "platform", "darwin"), mock.patch.object(
            sign_daemon_dev.subprocess, "run", return_value=security
        ) as run:
            self.assertEqual(sign_daemon_dev.main(), 0)
        self.assertEqual(run.call_count, 1)

    def test_security_and_codesign_failures_are_swallowed(self) -> None:
        with mock.patch.object(sign_daemon_dev.sys, "platform", "darwin"), mock.patch.object(
            sign_daemon_dev.subprocess, "run", side_effect=OSError("security unavailable")
        ):
            self.assertEqual(sign_daemon_dev.main(), 0)

        security = subprocess.CompletedProcess(
            ["security"], 0, stdout='  1) ABCD "atm-daemon-dev"\n', stderr=""
        )
        with mock.patch.object(sign_daemon_dev.sys, "platform", "darwin"), mock.patch.object(
            sign_daemon_dev.subprocess, "run", side_effect=[security, subprocess.CalledProcessError(1, "codesign")]
        ), mock.patch.object(sign_daemon_dev.Path, "is_file", side_effect=[True, False]):
            self.assertEqual(sign_daemon_dev.main(), 0)

    def test_build_recipe_runs_signing_hook_after_cargo(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn("build:\n    cargo build --workspace\n    {{python_cmd}} .just/sign_daemon_dev.py", justfile)

    def test_benchmark_recipe_builds_its_feature_gated_daemon(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn(
            "benchmark *args:\n"
            "    cargo build --release -p agent-team-mail -p atm-daemon\n"
            "    # The isolated capacity runner launches this feature-gated bootstrap binary.\n"
            "    cargo build --release -p atm-daemon-bootstrap --features benchmark-harness --bin atm-daemon-benchmark\n"
            "    {{python_cmd}} .just/sign_daemon_dev.py",
            justfile,
        )

    def test_benchmark_recipe_publishes_the_canonical_report(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn(
            "    {{python_cmd}} scripts/smoke/run_admission_capacity.py {{args}}\n"
            "    # Publish all captured variants into the canonical report site.\n"
            "    {{python_cmd}} scripts/smoke/benchmark_report.py --rebuild",
            justfile,
        )


if __name__ == "__main__":
    unittest.main()
