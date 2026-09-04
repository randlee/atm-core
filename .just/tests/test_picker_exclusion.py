"""Hermetic dead/idle exclusion tests for every Send-To picker (PRD R4).

R4 (Must): dead/idle members must be genuinely non-selectable in every
picker, not merely labeled. Each backend's own affordances differ --
`osascript`'s "choose from list" has no disabled-row concept, zenity's
`--checklist` cannot render a non-interactive row either, `fzf` filters its
own input, and `Out-GridView` selects from whatever rows it is handed -- so
the only backend-agnostic way to satisfy R4 is to never offer a dead/idle
member as a choice in the first place.

This module feeds the committed `PickerInput` fixture
(`docs/plans/phase-aq/fixtures/picker-input-v1.json`, one `active`, one
`idle`, one `dead` member) through every picker adapter via the existing
`ATM_SEND_TO_SELECTION` headless test seam and asserts the idle and dead
members are never present in `PickerOutput.recipients`, even when explicitly
requested by id.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import unittest
from pathlib import Path
from unittest import mock

JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_common import resolve_posix_shell

ROOT = Path(__file__).resolve().parents[2]
FIXTURE_PATH = ROOT / "docs/plans/phase-aq/fixtures/picker-input-v1.json"
PICKER_PY = ROOT / "scripts/send-to/picker.py"
PICKER_MACOS = ROOT / "scripts/send-to/picker-macos.sh"
PICKER_LINUX = ROOT / "scripts/send-to/picker-linux.sh"
PICKER_WINDOWS = ROOT / "scripts/send-to/picker-windows.ps1"

FIXTURE_INPUT = json.loads(FIXTURE_PATH.read_text(encoding="utf-8"))

# The fixture's three members: cipher (active), fenix (idle), offline (dead).
ALL_MEMBER_IDS = "cipher@atm-dev,fenix@atm-dev,offline@atm-dev"
ACTIVE_MEMBER_ID = "cipher@atm-dev"
PICKER_COMMAND_TIMEOUT_SECONDS = 10


def sh_command(script: Path) -> list[str]:
    """Argv to run a `.sh` script portably."""
    if os.name == "nt":
        shell = resolve_posix_shell()
        if shell is None:  # pragma: no cover - guarded by class-level skip
            raise AssertionError("a POSIX shell must be available on Windows")
        return [shell, str(script)]
    return [str(script)]


def run(command: list[str], extra_env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    # Every picker adapter checks this seam before touching any real UI
    # backend, so this stays hermetic on every platform: no osascript
    # dialog, zenity window, fzf prompt, or Out-GridView grid ever opens.
    env["ATM_SEND_TO_SELECTION"] = ALL_MEMBER_IDS
    if extra_env:
        env.update(extra_env)
    return subprocess.run(
        command,
        input=json.dumps(FIXTURE_INPUT),
        text=True,
        env=env,
        capture_output=True,
        timeout=PICKER_COMMAND_TIMEOUT_SECONDS,
    )


class PickerExclusionAssertions:
    """Shared assertions mixed into every picker's exclusion test case."""

    def assert_dead_and_idle_are_excluded(self, result: subprocess.CompletedProcess[str]) -> None:
        self.assertEqual(result.returncode, 0, result.stderr)  # type: ignore[attr-defined]
        output = json.loads(result.stdout)
        self.assertEqual(output["recipients"], [ACTIVE_MEMBER_ID])  # type: ignore[attr-defined]
        self.assertNotIn("fenix@atm-dev", output["recipients"])  # type: ignore[attr-defined]
        self.assertNotIn("offline@atm-dev", output["recipients"])  # type: ignore[attr-defined]
        # The exclusion must still be visible to the human as a notice, even
        # though it is not a selectable row.
        self.assertIn("fenix", result.stderr)  # type: ignore[attr-defined]
        self.assertIn("offline", result.stderr)  # type: ignore[attr-defined]
        self.assertIn("unavailable", result.stderr)  # type: ignore[attr-defined]


class PickerSubprocessTests(unittest.TestCase):
    def test_picker_runner_has_finite_timeout(self) -> None:
        completed = subprocess.CompletedProcess(args=[], returncode=0)
        with mock.patch("subprocess.run", return_value=completed) as run_mock:
            self.assertIs(run(["picker"]), completed)

        self.assertEqual(run_mock.call_args.kwargs["timeout"], PICKER_COMMAND_TIMEOUT_SECONDS)


class PickerPyExclusionTests(PickerExclusionAssertions, unittest.TestCase):
    def test_reference_picker_excludes_dead_and_idle(self) -> None:
        # sys.executable, not a bare "python3": guaranteed to resolve to
        # the interpreter actually running this test, on every platform.
        result = run([sys.executable, str(PICKER_PY)])
        self.assert_dead_and_idle_are_excluded(result)


@unittest.skipIf(
    os.name == "nt" and resolve_posix_shell() is None,
    "no POSIX shell (bash/sh) found on PATH to run this .sh wrapper on Windows",
)
class PickerMacosExclusionTests(PickerExclusionAssertions, unittest.TestCase):
    def test_macos_osascript_adapter_excludes_dead_and_idle(self) -> None:
        # picker-macos.sh always delegates to picker.py --backend osascript;
        # ATM_SEND_TO_SELECTION is consulted first, so osascript is never
        # actually invoked -- safe to run on any host with a POSIX shell to
        # execute the wrapper itself (see `resolve_posix_shell`/`sh_command`).
        result = run(sh_command(PICKER_MACOS))
        self.assert_dead_and_idle_are_excluded(result)


@unittest.skipUnless(
    shutil.which("zenity") or shutil.which("fzf"),
    "picker-linux.sh requires zenity or fzf on PATH before it will even "
    "launch picker.py, regardless of ATM_SEND_TO_SELECTION",
)
@unittest.skipIf(
    os.name == "nt" and resolve_posix_shell() is None,
    "no POSIX shell (bash/sh) found on PATH to run this .sh wrapper on Windows",
)
class PickerLinuxExclusionTests(PickerExclusionAssertions, unittest.TestCase):
    def test_linux_adapter_excludes_dead_and_idle(self) -> None:
        result = run(sh_command(PICKER_LINUX))
        self.assert_dead_and_idle_are_excluded(result)


# `picker-windows.ps1` is a Windows adapter.  On non-Windows hosts an
# opportunistically installed PowerShell is not a supported execution
# environment: the Homebrew PowerShell 7.5.4 runtime on macOS 15.5 crashes
# in .NET's MulticoreJitProfilePlayer before the script starts (SIGSEGV 11).
# Keep this test on the platform whose adapter it exercises; otherwise a
# broken foreign runtime turns an unrelated host lint run into a false failure.
@unittest.skipUnless(
    sys.platform == "win32" and shutil.which("pwsh"),
    "picker-windows.ps1 is validated on Windows only; unsupported pwsh host",
)
class PickerWindowsExclusionTests(PickerExclusionAssertions, unittest.TestCase):
    def test_windows_adapter_excludes_dead_and_idle(self) -> None:
        # ATM_SEND_TO_SELECTION is consulted before Out-GridView, so this is
        # hermetic on the Windows runner without opening a UI.
        result = run(["pwsh", "-NoProfile", "-File", str(PICKER_WINDOWS)])
        self.assert_dead_and_idle_are_excluded(result)


class PickerFunctionUnitTests(unittest.TestCase):
    """Direct unit coverage for the row-filtering helpers picker.py exports."""

    def setUp(self) -> None:
        sys.path.insert(0, str(PICKER_PY.parent))
        import picker as picker_module  # noqa: PLC0415

        self.picker_module = picker_module

    def tearDown(self) -> None:
        sys.path.remove(str(PICKER_PY.parent))
        sys.modules.pop("picker", None)

    def rows(self) -> list[dict[str, str]]:
        return [
            {"id": "cipher@atm-dev", "status": "active", "label": "cipher"},
            {"id": "fenix@atm-dev", "status": "idle", "label": "fenix"},
            {"id": "offline@atm-dev", "status": "dead", "label": "offline"},
        ]

    def test_selectable_rows_keeps_only_active(self) -> None:
        selectable = self.picker_module.selectable_rows(self.rows())
        self.assertEqual([row["id"] for row in selectable], ["cipher@atm-dev"])

    def test_unavailable_rows_keeps_dead_and_idle(self) -> None:
        unavailable = self.picker_module.unavailable_rows(self.rows())
        self.assertEqual(
            sorted(row["id"] for row in unavailable),
            ["fenix@atm-dev", "offline@atm-dev"],
        )


if __name__ == "__main__":
    unittest.main()
