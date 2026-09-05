from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/send-to" / ("atm-send-to.ps1" if os.name == "nt" else "atm-send-to.sh")
PICKER = ROOT / "scripts/send-to/picker.py"
COMMAND_WRAPPER = ROOT / "scripts/send-to/atm-send-to.command"
NAUTILUS_WRAPPER = ROOT / "scripts/send-to/nautilus-atm-send-to.sh"
FIXTURES = ROOT / "docs/plans/phase-aq/fixtures"
VALIDATE_RELEASE = ROOT / "scripts/validate_release.py"

# Test-only seam in atm-send-to.command / nautilus-atm-send-to.sh that
# replaces the wrapper's desktop notification (`osascript display
# notification` / `notify-send`) on the failure path. Before this seam
# existed, every `just test` / `just lint` / `just validate` run popped a
# real macOS notification from
# test_command_wrapper_propagates_exit_code_and_forwards_stderr_on_failure.
NOTIFIER_SEAM = "ATM_SEND_TO_NOTIFIER"
# The desktop-notification binaries the wrappers reach for by default. Every
# wrapper run in this suite shadows them on PATH with a stub that records a
# marker; tearDown asserts the marker was never written.
DESKTOP_NOTIFIER_BINARIES = ("osascript", "notify-send")

# Load the committed PickerInput/PickerOutput fixtures verbatim (rather than
# inline-shaped equivalents) so this suite exercises the exact bytes the
# fixture docs (wyvern-pick-member-contract.md, validation-evidence.md) claim
# are under test.
PICKER_INPUT = json.loads((FIXTURES / "picker-input-v1.json").read_text())
PICKER_OUTPUT_V1 = json.loads((FIXTURES / "picker-output-v1.json").read_text())
PICKER_OUTPUT_UNKNOWN_SCHEMA = json.loads((FIXTURES / "picker-output-unknown-schema.json").read_text())


def executable(path: Path) -> None:
    if os.name != "nt":
        path.chmod(path.stat().st_mode | stat.S_IXUSR)


def fixture_path(directory: Path, stem: str) -> Path:
    return directory / (f"{stem}.cmd" if os.name == "nt" else stem)


def invoke_script(script: Path, *args: Path) -> list[str]:
    if os.name == "nt":
        return [shutil.which("pwsh") or "pwsh", "-NoProfile", "-File", str(script), *(str(arg) for arg in args)]
    return [str(script), *(str(arg) for arg in args)]


class SendToSurfaceTests(unittest.TestCase):
    def setUp(self) -> None:
        # Shadow the real desktop-notification binaries for every wrapper
        # run so a regression in the notifier seam fails this suite loudly
        # instead of silently popping a notification on the developer's
        # desktop. The stub only records its argv; it never shows UI.
        self._ui_stub_dir = tempfile.TemporaryDirectory()
        self.ui_marker = Path(self._ui_stub_dir.name) / "desktop-notification.marker"
        if os.name != "nt":
            for name in DESKTOP_NOTIFIER_BINARIES:
                stub = Path(self._ui_stub_dir.name) / name
                stub.write_text(
                    "#!/bin/sh\n"
                    f"printf '%s\\n' \"$0\" \"$@\" >> '{self.ui_marker}'\n",
                    encoding="utf-8",
                )
                executable(stub)

    def tearDown(self) -> None:
        leaked = self.ui_marker.read_text(encoding="utf-8") if self.ui_marker.exists() else ""
        self._ui_stub_dir.cleanup()
        self.assertEqual(
            leaked,
            "",
            "a Send-To wrapper reached a real desktop-notification binary; the "
            f"{NOTIFIER_SEAM} seam must keep every test in this suite UI-free:\n{leaked}",
        )

    def make_atm(self, directory: Path, log: Path) -> Path:
        path = fixture_path(directory, "atm")
        if os.name == "nt":
            # `type nul > log` (an earlier draft) only ever created an empty
            # file -- it never reads the PickerOutput JSON piped in on stdin
            # (`$pickerOutput | & $atm @sendArgs` in atm-send-to.ps1), so the
            # log was always empty and the test's json.loads on it failed on
            # every real Windows run. `more`'s output-redirected-to-a-file
            # mode is cmd.exe's documented stdin-to-file idiom (it disables
            # the pager and copies straight through when stdout is not a
            # console), which is what's needed to capture that JSON body --
            # mirroring the POSIX fake's `cat > {log}` below.
            path.write_text(
                "@echo off\r\n"
                'if "%~1"=="teams" type "' + str(directory / "input.json") + '"\r\n'
                'if "%~1"=="send" (\r\n'
                '  echo %* > "' + str(log) + '.args"\r\n'
                '  more > "' + str(log) + '"\r\n'
                "  echo send-called 1>&2\r\n"
                ")\r\n",
                encoding="utf-8",
            )
        else:
            path.write_text(
                "#!/bin/sh\n"
                "if [ \"$1\" = teams ]; then\n"
                f"  cat {directory / 'input.json'}\n"
                "elif [ \"$1\" = send ]; then\n"
                f"  printf '%s\\n' \"$@\" > {log}.args\n"
                f"  cat > {log}\n"
                "  echo send-called >&2\n"
                "fi\n",
                encoding="utf-8",
            )
        executable(path)
        return path

    def make_picker(self, directory: Path, output: str, code: int = 0) -> Path:
        path = fixture_path(directory, "picker-ok" if code == 0 else "picker-cancel")
        if os.name == "nt":
            path.write_text(
                "@echo off\r\n"
                f"echo {output}\r\n"
                f"exit /b {code}\r\n",
                encoding="utf-8",
            )
        else:
            path.write_text(
                f"#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{output}'\nexit {code}\n",
                encoding="utf-8",
            )
        executable(path)
        return path

    def run_surface(self, picker: Path | None, files: list[Path], directory: Path, log: Path, extra: dict[str, str] | None = None, script: Path = SCRIPT) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env["ATM_BIN"] = str(directory / "atm")
        if picker is not None:
            env["ATM_SEND_TO_PICKER"] = str(picker)
        else:
            env.pop("ATM_SEND_TO_PICKER", None)
        # No test here may raise a real desktop notification: suppress the
        # wrappers' failure-path notification by default (a test that wants
        # to observe it overrides the seam via `extra`), and put the
        # marker-recording stubs ahead of the real binaries on PATH.
        env[NOTIFIER_SEAM] = "none"
        env["PATH"] = self._ui_stub_dir.name + os.pathsep + env.get("PATH", "")
        if extra:
            env.update(extra)
        return subprocess.run(invoke_script(script, *files), cwd=ROOT, env=env, text=True, capture_output=True)

    def test_cancel_exits_without_invoking_send(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            (directory / "input.json").write_text(json.dumps(PICKER_INPUT))
            log = directory / "send.log"
            atm = self.make_atm(directory, log)
            picker = self.make_picker(directory, "", code=17)
            result = self.run_surface(picker, [directory / "one.txt"], directory, log)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(log.exists(), result.stderr)
            self.assertTrue(atm.exists())

    def test_multiple_files_and_recipients_reach_one_final_send(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            (directory / "input.json").write_text(json.dumps(PICKER_INPUT))
            log = directory / "send.log"
            self.make_atm(directory, log)
            output = json.dumps(PICKER_OUTPUT_V1)
            picker = self.make_picker(directory, output)
            result = self.run_surface(picker, [directory / "one file.txt", directory / "two.txt"], directory, log)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(log.read_text()), PICKER_OUTPUT_V1)
            self.assertIn("--attach", (log.with_suffix(".log.args")).read_text())

    def test_reference_picker_emits_versioned_output(self) -> None:
        env = os.environ.copy()
        # fenix is `idle` in PICKER_INPUT and therefore non-selectable (R4);
        # requesting it alongside cipher (`active`) still returns only
        # cipher, and the picker notes the exclusion on stderr.
        env["ATM_SEND_TO_SELECTION"] = "fenix@atm-dev,cipher@atm-dev"
        result = subprocess.run(["python3", str(PICKER)], input=json.dumps(PICKER_INPUT), text=True, env=env, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["schema_version"], 1)
        self.assertEqual(json.loads(result.stdout)["recipients"], ["cipher@atm-dev"])
        self.assertIn("fenix", result.stderr)
        self.assertIn("unavailable", result.stderr)

    def test_generated_wizard_json_matches_contract_shape(self) -> None:
        # docs/plans/phase-aq/fixtures/wyvern-pick-member-contract.md's "Real
        # Wyvern invocation" shape: Wyvern has no `--picker` flag, so
        # PickerInput travels verbatim as the generated wizard command's
        # `config`, and the page id/title/html match the vendored
        # scripts/send-to/pick-member.html asset atm-send-to.sh copies into
        # the wizard `--ui-root` directory it builds per invocation.
        result = subprocess.run(
            ["python3", str(PICKER), "--make-wizard-json"],
            input=json.dumps(PICKER_INPUT),
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        wizard = json.loads(result.stdout)
        self.assertEqual(wizard["type"], "wizard")
        self.assertEqual(
            wizard["page"],
            {"id": "pick-member", "title": "ATM Send-To", "html": "pages/pick-member.html"},
        )
        self.assertEqual(wizard["config"], PICKER_INPUT)

    def test_wyvern_degradation_cases_fall_back_and_still_send(self) -> None:
        cases = ("absent", "below", "unparsable", "hang", "missing-asset", "unknown-schema")
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            (directory / "input.json").write_text(json.dumps(PICKER_INPUT))
            log = directory / "send.log"
            self.make_atm(directory, log)
            asset = directory / "pick-member.html"
            asset.write_text("external Wyvern asset")
            # Every case in this loop degrades to the native fallback picker.
            # Route it through a deterministic fixture (never the host's real
            # zenity/fzf/osascript) so this harness proves the degradation
            # contract without depending on desktop tooling being installed
            # on the CI runner.
            native_picker = self.make_picker(
                directory, '{"schema_version":1,"recipients":["cipher@atm-dev"]}'
            )
            for case in cases:
                with self.subTest(case=case):
                    log.unlink(missing_ok=True)
                    wyvern = fixture_path(directory, f"wyvern-{case}")
                    if case != "absent":
                        if case == "below":
                            body = "wyvern 0.4.0"
                        elif case == "unparsable":
                            body = "wyvern development"
                        elif case == "hang":
                            # Comfortably exceeds probe_wyvern.py's PROBE_SECONDS
                            # (1.5s) bounded deadline; subprocess.run's timeout
                            # kills this before it ever completes, so a larger
                            # margin costs nothing but removes any chance of a
                            # loaded CI runner racing the deadline closed.
                            body = None
                        else:
                            body = "wyvern 0.6.0"
                        # A real Wyvern wizard's terminal stdout is the full
                        # WizardResult envelope (`{"button":"finish","data":
                        # <PickerOutput>,"stack":[...]}`), not a bare
                        # PickerOutput object -- this stub mirrors that exact
                        # shape so the harness proves the real contract
                        # (docs/plans/phase-aq/fixtures/
                        # wyvern-pick-member-contract.md), not the illustrative
                        # bare-stdout sketch the PRD started from.
                        wizard_result = json.dumps(
                            {
                                "button": "finish",
                                "data": (
                                    PICKER_OUTPUT_UNKNOWN_SCHEMA
                                    if case == "unknown-schema"
                                    # No committed fixture covers this shape (a
                                    # single-recipient, note-less PickerOutput);
                                    # PICKER_OUTPUT_V1 is two recipients plus a
                                    # note, so this case stays inline.
                                    else {"schema_version": 1, "recipients": ["cipher@atm-dev"]}
                                ),
                                "stack": [],
                            },
                            separators=(",", ":"),
                        )
                        if os.name == "nt":
                            if body is None:
                                version_command = 'pwsh -NoProfile -Command "Start-Sleep -Seconds 30"'
                            else:
                                version_command = f"echo {body}"
                            wyvern.write_text(
                                "@echo off\r\n"
                                'if "%~1"=="--version" (' + version_command + ') else (\r\n'
                                f"echo {wizard_result}\r\n"
                                ")\r\n",
                                encoding="utf-8",
                            )
                        else:
                            if body is None:
                                version_command = "sleep 30"
                            else:
                                version_command = f"printf '%s\\n' '{body}'"
                            wyvern.write_text(
                                "#!/bin/sh\nif [ \"$1\" = --version ]; then\n"
                                f"{version_command}\nelse\nprintf '%s\\n' '{wizard_result}'\nfi\n",
                                encoding="utf-8",
                            )
                        executable(wyvern)
                    extra = {
                        "ATM_SEND_TO_WYVERN_BIN": str(wyvern),
                        "ATM_SEND_TO_WYVERN_ASSET": str(asset if case != "missing-asset" else directory / "absent.html"),
                        "ATM_SEND_TO_NATIVE_PICKER": str(native_picker),
                        "ATM_SEND_TO_SELECTION": "cipher@atm-dev",
                    }
                    result = self.run_surface(None, [directory / "one.txt"], directory, log, extra)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertTrue(log.exists(), result.stderr)
                    self.assertIn("Wyvern", result.stderr)

    def make_noisy_failing_picker(self, directory: Path, code: int = 7) -> Path:
        """A picker that fails loudly on stderr, like the real `picker.py` does."""
        path = fixture_path(directory, "picker-noisy-failure")
        if os.name == "nt":
            path.write_text(
                "@echo off\r\n"
                "echo send-to picker: at least one recipient must be selected 1>&2\r\n"
                f"exit /b {code}\r\n",
                encoding="utf-8",
            )
        else:
            path.write_text(
                "#!/bin/sh\ncat >/dev/null\n"
                "echo 'send-to picker: at least one recipient must be selected' >&2\n"
                f"exit {code}\n",
                encoding="utf-8",
            )
        executable(path)
        return path

    @unittest.skipIf(os.name == "nt", "the command wrapper is Unix-only")
    def test_command_wrapper_propagates_exit_code_and_forwards_stderr_on_failure(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            (directory / "input.json").write_text(json.dumps(PICKER_INPUT))
            log = directory / "send.log"
            self.make_atm(directory, log)
            picker = self.make_noisy_failing_picker(directory, code=7)
            result = self.run_surface(picker, [directory / "one.txt"], directory, log, script=COMMAND_WRAPPER)
            # Regression guard: an earlier draft of atm-send-to.command used
            # `if cmd; then ...; fi; status=$?`, which reads 0 (not the
            # tested command's real status) once the `if` has no matching
            # branch to run -- silently turning every wrapper failure into a
            # reported success. The wrapper must propagate the real exit
            # code, and stderr must still carry the failure detail (the
            # notification is an addition, never a replacement).
            self.assertEqual(result.returncode, 7, result.stderr)
            self.assertFalse(log.exists(), result.stderr)
            self.assertIn("send-to picker", result.stderr)

    def make_recording_notifier(self, directory: Path, record: Path) -> Path:
        """A notifier command that records its argv (one per line) to `record`."""
        path = fixture_path(directory, "notifier-record")
        path.write_text(
            "#!/bin/sh\n"
            f"printf '%s\\n' \"$@\" > '{record}'\n",
            encoding="utf-8",
        )
        executable(path)
        return path

    @unittest.skipIf(os.name == "nt", "the notifying wrappers are Unix-only")
    def test_wrapper_notification_seam_replaces_the_desktop_notification_on_failure(self) -> None:
        expected_message = "send-to picker: at least one recipient must be selected"
        for wrapper in (COMMAND_WRAPPER, NAUTILUS_WRAPPER):
            for mode in ("command", "stderr"):
                with self.subTest(wrapper=wrapper.name, mode=mode), tempfile.TemporaryDirectory() as raw:
                    directory = Path(raw)
                    (directory / "input.json").write_text(json.dumps(PICKER_INPUT))
                    log = directory / "send.log"
                    self.make_atm(directory, log)
                    picker = self.make_noisy_failing_picker(directory, code=7)
                    record = directory / "notifier.args"
                    notifier = self.make_recording_notifier(directory, record)
                    extra = {NOTIFIER_SEAM: str(notifier) if mode == "command" else "stderr"}
                    result = self.run_surface(picker, [directory / "one.txt"], directory, log, extra, script=wrapper)
                    # The seam must not change the wrapper's contract: real
                    # exit code, no send, full stderr detail still forwarded.
                    self.assertEqual(result.returncode, 7, result.stderr)
                    self.assertFalse(log.exists(), result.stderr)
                    self.assertIn(expected_message, result.stderr)
                    if mode == "command":
                        # The notification travels through the seam as
                        # `<command> "ATM Send-To" "<last stderr line>"` ...
                        self.assertEqual(record.read_text(encoding="utf-8").splitlines(), ["ATM Send-To", expected_message])
                    else:
                        self.assertFalse(record.exists())
                        self.assertIn(f"ATM Send-To: {expected_message}", result.stderr)
                    # ... and never through osascript / notify-send (the PATH
                    # stubs installed by setUp would have recorded a marker).
                    self.assertFalse(self.ui_marker.exists(), "the wrapper reached a real desktop-notification binary")

    @unittest.skipIf(os.name == "nt", "the command wrapper is Unix-only")
    def test_command_wrapper_exits_zero_on_success(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            (directory / "input.json").write_text(json.dumps(PICKER_INPUT))
            log = directory / "send.log"
            self.make_atm(directory, log)
            output = json.dumps({"schema_version": 1, "recipients": ["cipher@atm-dev"]})
            picker = self.make_picker(directory, output)
            result = self.run_surface(picker, [directory / "one.txt"], directory, log, script=COMMAND_WRAPPER)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(log.exists(), result.stderr)

    @unittest.skipIf(os.name == "nt", "the Nautilus wrapper is Unix-only")
    def test_nautilus_wrapper_propagates_exit_code_and_forwards_stderr_on_failure(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            (directory / "input.json").write_text(json.dumps(PICKER_INPUT))
            log = directory / "send.log"
            self.make_atm(directory, log)
            picker = self.make_noisy_failing_picker(directory, code=7)
            result = self.run_surface(picker, [directory / "one.txt"], directory, log, script=NAUTILUS_WRAPPER)
            self.assertEqual(result.returncode, 7, result.stderr)
            self.assertFalse(log.exists(), result.stderr)
            self.assertIn("send-to picker", result.stderr)


# RBQA-F103: ATM_SEND_TO_PICKER / ATM_SEND_TO_NATIVE_PICKER are test-only
# override seams shipped inside atm-send-to.sh/.ps1, and ATM_SEND_TO_NOTIFIER
# is the test-only notification seam shipped inside atm-send-to.command /
# nautilus-atm-send-to.sh, with no mechanical enforcement that they stay
# documented as test-only or stay confined to their host scripts (plus their
# own test coverage). This is that enforcement.
SEND_TO_TEST_ONLY_ENV_VARS = ("ATM_SEND_TO_PICKER", "ATM_SEND_TO_NATIVE_PICKER", NOTIFIER_SEAM)
SEND_TO_SCRIPTS = (
    ROOT / "scripts/send-to/atm-send-to.sh",
    ROOT / "scripts/send-to/atm-send-to.ps1",
)
SEND_TO_NOTIFYING_WRAPPERS = (COMMAND_WRAPPER, NAUTILUS_WRAPPER)
# Which shipped script(s) host each seam: every seam must be read by exactly
# these files and documented as test-only right where it is read.
SEND_TO_SEAM_HOSTS = {
    "ATM_SEND_TO_PICKER": SEND_TO_SCRIPTS,
    "ATM_SEND_TO_NATIVE_PICKER": SEND_TO_SCRIPTS,
    NOTIFIER_SEAM: SEND_TO_NOTIFYING_WRAPPERS,
}
# Files other than the seam hosts and `test_*` files that legitimately
# reference a seam today. Kept as an explicit allowlist (rather than a
# broader glob) so any new reference is a deliberate, reviewed addition
# rather than an accidental leak into a shipped path.
SEND_TO_SEAM_REFERENCE_ALLOWLIST = (
    ROOT / "scripts/phase-aq/run_aq5_wyvern_degradation_evidence.py",
    # RBQA-F103's own release-environment guard (see
    # validate_send_to_test_seams in scripts/validate_release.py).
    VALIDATE_RELEASE,
    # Operator-facing note that the notifier seam is test-only.
    ROOT / "scripts/send-to/README.md",
)


class SendToTestOnlySeamLintTests(unittest.TestCase):
    def test_every_seam_has_a_host_script_and_the_release_guard_lists_it(self) -> None:
        self.assertEqual(set(SEND_TO_SEAM_HOSTS), set(SEND_TO_TEST_ONLY_ENV_VARS))
        spec = importlib.util.spec_from_file_location("validate_release_for_seam_lint", VALIDATE_RELEASE)
        assert spec is not None and spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        # dataclasses resolves the defining module through sys.modules at
        # class-creation time, so the module must be registered before exec.
        sys.modules[spec.name] = module
        spec.loader.exec_module(module)
        # The release-environment guard must block the exact same set of
        # seams this lint confines, so a seam can never be added to one list
        # without the other.
        self.assertEqual(set(module.SEND_TO_TEST_ONLY_ENV_VARS), set(SEND_TO_TEST_ONLY_ENV_VARS))

    def test_host_scripts_document_the_seam_as_test_only(self) -> None:
        for var, hosts in SEND_TO_SEAM_HOSTS.items():
            for script in hosts:
                text = script.read_text(encoding="utf-8")
                self.assertIn(var, text, f"{script}: expected a reference to {var}")
                index = text.index(var)
                # The documenting comment must sit close to the variable, not
                # merely appear somewhere else in the file.
                window = text[max(0, index - 600) : index]
                self.assertIn(
                    "test-only",
                    window.lower(),
                    f"{script}: {var} must be documented as a test-only seam near its use",
                )

    def test_no_other_shipped_script_or_workflow_references_the_seam(self) -> None:
        candidate_roots = (
            ROOT / ".github" / "workflows",
            ROOT / "scripts",
            ROOT / ".just",
            # The seams live in the shell wrappers only; production Rust must
            # never read them.
            ROOT / "crates",
        )
        allowed = {*SEND_TO_SCRIPTS, *SEND_TO_NOTIFYING_WRAPPERS, *SEND_TO_SEAM_REFERENCE_ALLOWLIST}
        stray: list[str] = []
        for root in candidate_roots:
            if not root.exists():
                continue
            for path in sorted(root.rglob("*")):
                if not path.is_file() or path in allowed:
                    continue
                if path.name.startswith("test_"):
                    continue
                try:
                    text = path.read_text(encoding="utf-8")
                except (UnicodeDecodeError, OSError):
                    continue
                for var in SEND_TO_TEST_ONLY_ENV_VARS:
                    if var in text:
                        stray.append(f"{path.relative_to(ROOT)}: references {var}")
        self.assertEqual(
            stray,
            [],
            "the Send-To test-only seams must stay confined to their host "
            "scripts under scripts/send-to/, their tests, and the explicit "
            "SEND_TO_SEAM_REFERENCE_ALLOWLIST entries:\n" + "\n".join(stray),
        )


if __name__ == "__main__":
    unittest.main()
