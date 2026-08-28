from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/send-to" / ("atm-send-to.ps1" if os.name == "nt" else "atm-send-to.sh")
PICKER = ROOT / "scripts/send-to/picker.py"
COMMAND_WRAPPER = ROOT / "scripts/send-to/atm-send-to.command"
NAUTILUS_WRAPPER = ROOT / "scripts/send-to/nautilus-atm-send-to.sh"

PICKER_INPUT = {
    "schema_version": 1,
    "teams": [{
        "id": "atm-dev",
        "name": "atm-dev",
        "members": [
            {"id": "cipher@atm-dev", "name": "cipher", "host": "m4", "cwd": "/work", "status": "active"},
            {"id": "fenix@atm-dev", "name": "fenix", "host": "m5", "cwd": "/work", "status": "idle"},
        ],
    }],
}


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
            output = json.dumps({"schema_version": 1, "recipients": ["cipher@atm-dev", "fenix@atm-dev"]})
            picker = self.make_picker(directory, output)
            result = self.run_surface(picker, [directory / "one file.txt", directory / "two.txt"], directory, log)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(log.read_text()), {"schema_version": 1, "recipients": ["cipher@atm-dev", "fenix@atm-dev"]})
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
                            body = "wyvern 0.5.0"
                        # A real Wyvern wizard's terminal stdout is the full
                        # WizardResult envelope (`{"button":"finish","data":
                        # <PickerOutput>,"stack":[...]}`), not a bare
                        # PickerOutput object -- this stub mirrors that exact
                        # shape so the harness proves the real contract
                        # (docs/plans/phase-aq/fixtures/
                        # wyvern-pick-member-contract.md), not the illustrative
                        # bare-stdout sketch the PRD started from.
                        wizard_result = (
                            '{"button":"finish","data":{"schema_version":99,"recipients":["cipher@atm-dev"]},"stack":[]}'
                            if case == "unknown-schema"
                            else '{"button":"finish","data":{"schema_version":1,"recipients":["cipher@atm-dev"]},"stack":[]}'
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


if __name__ == "__main__":
    unittest.main()
