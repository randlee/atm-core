from __future__ import annotations

import io
from pathlib import Path
import importlib.util
import tempfile
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
            sign_daemon_dev, "resolve_apple_development_identity"
        ) as resolve:
            self.assertEqual(sign_daemon_dev.main(), 0)
        resolve.assert_not_called()

    def test_windows_warns_and_skips_signing(self) -> None:
        stderr = io.StringIO()
        with (
            mock.patch.object(sign_daemon_dev.sys, "platform", "win32"),
            mock.patch.object(sign_daemon_dev.sys, "stderr", stderr),
            mock.patch.object(sign_daemon_dev, "resolve_apple_development_identity") as resolve,
        ):
            self.assertEqual(sign_daemon_dev.main(), 0)
        self.assertIn("Windows signing not yet implemented", stderr.getvalue())
        resolve.assert_not_called()

    def test_apple_identity_signs_and_strictly_verifies_each_existing_managed_binary(self) -> None:
        identity = sign_daemon_dev.SigningIdentity("FINGERPRINT", "Apple Development: test")
        completed = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        with (
            mock.patch.object(sign_daemon_dev.sys, "platform", "darwin"),
            mock.patch.object(sign_daemon_dev, "unlock_login_keychain") as unlock,
            mock.patch.object(sign_daemon_dev, "resolve_apple_development_identity", return_value=identity),
            mock.patch.object(sign_daemon_dev.subprocess, "run", return_value=completed) as run,
            mock.patch.object(sign_daemon_dev, "verify_signing_identity", return_value=True),
            mock.patch.object(sign_daemon_dev.Path, "is_file", side_effect=[True] * len(sign_daemon_dev.MANAGED_TARGETS)),
        ):
            self.assertEqual(sign_daemon_dev.main(), 0)

        unlock.assert_called_once_with()
        commands = [call.args[0] for call in run.call_args_list]
        self.assertEqual(
            commands,
            [
                [
                    "codesign", "--force", "--sign", "FINGERPRINT", "--identifier", identifier,
                    "--entitlements", str(sign_daemon_dev.ENTITLEMENTS), str(binary),
                ]
                for binary, identifier in sign_daemon_dev.MANAGED_TARGETS
            ],
        )

    def test_identity_resolution_failure_fails_the_macos_build(self) -> None:
        stderr = io.StringIO()
        with (
            mock.patch.object(sign_daemon_dev.sys, "platform", "darwin"),
            mock.patch.object(
                sign_daemon_dev,
                "resolve_apple_development_identity",
                side_effect=sign_daemon_dev.SigningIdentityError("missing Apple identity"),
            ),
            mock.patch.object(sign_daemon_dev.sys, "stderr", stderr),
        ):
            self.assertEqual(sign_daemon_dev.main(), 1)
        self.assertIn("missing Apple identity", stderr.getvalue())

    def test_self_signed_identity_uses_leaf_pin_verification(self) -> None:
        identity = sign_daemon_dev.SigningIdentity("A" * 40, "atm-daemon-dev")
        candidate = Path("/candidate/atm-daemon")
        completed = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        with (
            mock.patch.object(sign_daemon_dev.subprocess, "run", return_value=completed),
            mock.patch.object(sign_daemon_dev, "verify_signing_identity", return_value=True) as verify,
        ):
            sign_daemon_dev.sign_and_verify_binary(
                candidate, sign_daemon_dev.DAEMON_IDENTIFIER, identity
            )
        verify.assert_called_once_with(
            str(candidate),
            sign_daemon_dev.DAEMON_IDENTIFIER,
            identity,
        )

    def test_account_secret_unlocks_only_the_current_login_keychain(self) -> None:
        completed = subprocess.CompletedProcess(["security"], 0, stdout="", stderr="")
        with tempfile.TemporaryDirectory() as temporary_directory:
            secret_file = Path(temporary_directory) / "keychain-secret"
            secret_file.write_text("secret-value\n", encoding="utf-8")
            with mock.patch.object(sign_daemon_dev.subprocess, "run", return_value=completed) as run:
                sign_daemon_dev.unlock_login_keychain(secret_file)

            self.assertEqual(
                run.call_args.args[0],
                ["security", "unlock-keychain", "-p", "secret-value", str(sign_daemon_dev.LOGIN_KEYCHAIN)],
            )
            self.assertEqual(run.call_args.kwargs["stdout"], sign_daemon_dev.subprocess.DEVNULL)
            self.assertEqual(run.call_args.kwargs["stderr"], sign_daemon_dev.subprocess.DEVNULL)

    def test_absent_account_secret_does_not_attempt_an_unlock(self) -> None:
        with (
            mock.patch.dict(sign_daemon_dev.os.environ, {}, clear=True),
            mock.patch.object(
                sign_daemon_dev,
                "BENCHMARK_KEYCHAIN_SECRET_FILE",
                Path("/private/tmp/does-not-exist-atm-keychain-secret"),
            ),
            mock.patch.object(sign_daemon_dev.subprocess, "run") as run,
        ):
            sign_daemon_dev.unlock_login_keychain()
        run.assert_not_called()

    def test_configured_secret_file_overrides_benchmark_account_default(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            home = Path(temporary_directory)
            with mock.patch.dict(
                sign_daemon_dev.os.environ,
                {
                    sign_daemon_dev.KEYCHAIN_SECRET_FILE_ENVIRONMENT_VARIABLE: "~/custom-secret",
                    "HOME": str(home),
                    "USERPROFILE": str(home),
                },
                clear=True,
            ):
                self.assertEqual(sign_daemon_dev.keychain_secret_file(), home / "custom-secret")

    def test_build_recipe_runs_signing_hook_after_cargo(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn("build:\n    cargo build --workspace\n    {{python_cmd}} .just/sign_daemon_dev.py", justfile)

    def test_test_recipe_re_signs_debug_artifacts_after_tests(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn(
            "test mode='default':\n"
            "    {{python_cmd}} .just/run_tests.py {{mode}}\n"
            "    {{python_cmd}} .just/sign_daemon_dev.py",
            justfile,
        )

    def test_benchmark_recipe_builds_its_feature_gated_daemon(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn(
            "benchmark *args:\n"
            "    cargo build --release -p agent-team-mail -p atm-daemon\n"
            "    {{python_cmd}} .just/sign_daemon_dev.py",
            justfile,
        )
        self.assertNotIn("benchmark-harness", justfile)
        self.assertNotIn("atm-daemon-benchmark", justfile)

    def test_benchmark_recipe_publishes_the_canonical_report(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn(
            "    # The wrapper preserves the runner verdict while rebuilding reports on\n"
            "    # both POSIX shells and PowerShell.\n"
            "    {{python_cmd}} .just/run_benchmark.py {{args}}",
            justfile,
        )

    def test_benchmark_show_rebuilds_then_opens_the_html_preview(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn(
            "benchmark-show:\n"
            "    {{python_cmd}} scripts/smoke/benchmark_report.py --rebuild\n"
            "    {{python_cmd}} scripts/smoke/benchmark_show.py",
            justfile,
        )


if __name__ == "__main__":
    unittest.main()
