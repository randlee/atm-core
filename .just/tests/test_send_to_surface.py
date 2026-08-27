from __future__ import annotations

import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/send-to/atm-send-to.sh"
PICKER = ROOT / "scripts/send-to/picker.py"

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
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


@unittest.skipIf(
    sys.platform == "win32",
    "atm-send-to.sh is the macOS/Linux pipeline (bash + osascript/zenity/fzf); "
    "Windows ships the separate atm-send-to.ps1 + picker-windows.ps1 pipeline, "
    "validated by manual E2E per docs/plans/phase-aq/sprint-AQ5-surface-evidence.md",
)
class SendToSurfaceTests(unittest.TestCase):
    def make_atm(self, directory: Path, log: Path) -> Path:
        path = directory / "atm"
        path.write_text(
            "#!/bin/sh\n"
            "if [ \"$1\" = teams ]; then\n"
            f"  cat {directory / 'input.json'}\n"
            "elif [ \"$1\" = send ]; then\n"
            f"  printf '%s\\n' \"$@\" > {log}.args\n"
            f"  cat > {log}\n"
            "  echo send-called >&2\n"
            "fi\n"
        )
        executable(path)
        return path

    def make_picker(self, directory: Path, output: str, code: int = 0) -> Path:
        path = directory / ("picker-ok" if code == 0 else "picker-cancel")
        path.write_text(f"#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{output}'\nexit {code}\n")
        executable(path)
        return path

    def run_surface(self, picker: Path | None, files: list[Path], directory: Path, log: Path, extra: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env["ATM_BIN"] = str(directory / "atm")
        if picker is not None:
            env["ATM_SEND_TO_PICKER"] = str(picker)
        else:
            env.pop("ATM_SEND_TO_PICKER", None)
        if extra:
            env.update(extra)
        return subprocess.run([str(SCRIPT), *(str(item) for item in files)], cwd=ROOT, env=env, text=True, capture_output=True)

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
        env["ATM_SEND_TO_SELECTION"] = "fenix@atm-dev,cipher@atm-dev"
        result = subprocess.run(["python3", str(PICKER)], input=json.dumps(PICKER_INPUT), text=True, env=env, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["schema_version"], 1)
        self.assertEqual(json.loads(result.stdout)["recipients"], ["cipher@atm-dev", "fenix@atm-dev"])

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
                    wyvern = directory / f"wyvern-{case}"
                    if case != "absent":
                        if case == "below":
                            body = "printf 'wyvern 0.4.0\\n'\n"
                        elif case == "unparsable":
                            body = "printf 'wyvern development\\n'\n"
                        elif case == "hang":
                            # Comfortably exceeds probe_wyvern.py's PROBE_SECONDS
                            # (1.5s) bounded deadline; subprocess.run's timeout
                            # kills this before it ever completes, so a larger
                            # margin costs nothing but removes any chance of a
                            # loaded CI runner racing the deadline closed.
                            body = "sleep 30\n"
                        else:
                            body = "printf 'wyvern 0.5.0\\n'\n"
                        picker_output = (
                            '{"schema_version":99,"recipients":["cipher@atm-dev"]}'
                            if case == "unknown-schema"
                            else '{"schema_version":1,"recipients":["cipher@atm-dev"]}'
                        )
                        wyvern.write_text(
                            f"#!/bin/sh\nif [ \"$1\" = --version ]; then\n{body}"
                            f"elif [ \"$1\" = --picker ]; then\nexit 99\n"
                            f"else\nprintf '%s\\n' '{{\"button\":\"finish\",\"data\":{picker_output},\"stack\":[]}}'\nfi\n"
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


if __name__ == "__main__":
    unittest.main()
