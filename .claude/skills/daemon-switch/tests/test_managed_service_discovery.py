"""Managed-service discovery stays unique, owned, and selector-bound."""

from __future__ import annotations

import argparse
from contextlib import redirect_stderr
import importlib.util
import io
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPTS = Path(__file__).parents[1] / "scripts"
REPO_SCRIPTS = Path(__file__).resolve().parents[4] / "scripts"
for scripts_directory in (SCRIPTS, REPO_SCRIPTS):
    if str(scripts_directory) not in sys.path:
        sys.path.insert(0, str(scripts_directory))


def load_script(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SERVICE = load_script("managed_service_control", SCRIPTS / "service_control.py")
SWITCH = load_script("managed_service_switch", SCRIPTS / "daemon-switch.py")


def completed(stdout: str = "", returncode: int = 0, stderr: str = ""):
    return subprocess.CompletedProcess([], returncode, stdout, stderr)


class ManagedServiceDiscoveryTests(unittest.TestCase):
    def assert_refuses(self, candidates, pattern: str) -> None:
        with self.assertRaisesRegex(SERVICE.SwitchError, pattern):
            SERVICE._unique_service(candidates, Path("/selectors/atm-daemon"))

    def test_darwin_unique_zero_ambiguous_and_foreign_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            selector = root / "selectors" / "atm-daemon"
            selector.parent.mkdir()

            def write(label: str, executable: Path) -> Path:
                path = root / f"{label}.plist"
                path.write_bytes(
                    plistlib.dumps({"Label": label, "ProgramArguments": [str(executable)]})
                )
                return path

            native = write("com.example.native", selector)
            fixture = write("com.atm.daemon.crosshost-smoke", root / "fixture-daemon")

            def loaded(identifier: str) -> Path | None:
                label = identifier.rsplit("/", 1)[-1]
                return {
                    "com.example.native": native.resolve(),
                    "com.atm.daemon.crosshost-smoke": fixture.resolve(),
                }.get(label)

            # os.getuid does not exist on Windows; supply one there so the
            # launchctl domain lookup runs on every CI runner. POSIX keeps the
            # real uid, which the plist ownership check compares against.
            uid = mock.patch.object(os, "getuid", new=getattr(os, "getuid", lambda: 501), create=True)
            uid.start()
            self.addCleanup(uid.stop)
            candidates = SERVICE._macos_candidates(selector, loaded, root)
            self.assertEqual(
                SERVICE._unique_service(candidates, selector),
                ("com.example.native", native.resolve()),
            )
            self.assertNotIn("com.atm.daemon.crosshost-smoke", [name for name, _path in candidates])
            self.assert_refuses([], "no current-user managed service")
            second = write("com.example.second", selector)
            candidates = SERVICE._macos_candidates(
                selector,
                lambda identifier: second.resolve() if identifier.endswith("second") else loaded(identifier),
                root,
            )
            self.assert_refuses(candidates, "multiple current-user managed services")

    def test_linux_unique_zero_ambiguous_and_foreign_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            selector = root / "selectors" / "atm-daemon"
            selector.parent.mkdir()
            fragments = {}
            executions = {}
            for unit, executable in (
                ("native.service", selector),
                ("second.service", selector),
                ("crosshost-smoke.service", root / "fixture-daemon"),
            ):
                fragment = root / unit
                fragment.write_text("[Service]\n", encoding="utf-8")
                fragments[unit] = fragment
                executions[unit] = f"{{ path={executable} ; argv[]={executable} ; }}"

            def runner(command, *, timeout):
                del timeout
                if "list-unit-files" in command:
                    return completed("native.service enabled\ncrosshost-smoke.service enabled\n")
                unit, property_name = command[3], command[4]
                value = fragments[unit] if property_name == "--property=FragmentPath" else executions[unit]
                return completed(f"{value}\n")

            with mock.patch.object(SERVICE, "run", side_effect=runner):
                candidates = SERVICE._linux_candidates(selector)
            self.assertEqual(candidates, [("native.service", None)])
            self.assert_refuses([], "no current-user managed service")
            with mock.patch.object(
                SERVICE,
                "run",
                side_effect=lambda command, timeout: (
                    completed("native.service enabled\nsecond.service enabled\n")
                    if "list-unit-files" in command
                    else runner(command, timeout=timeout)
                ),
            ):
                candidates = SERVICE._linux_candidates(selector)
            self.assert_refuses(candidates, "multiple current-user managed services")

    def test_windows_unique_zero_ambiguous_and_foreign_fixture(self) -> None:
        selector = Path(r"C:\atm-active\atm-daemon.exe")
        statuses = {
            "native": {
                "registered": True,
                "command": str(selector),
                "user_id": r"DOMAIN\agent",
            },
            "second": {
                "registered": True,
                "command": str(selector),
                "user_id": r"DOMAIN\agent",
            },
            "crosshost-smoke": {
                "registered": True,
                "command": r"C:\fixtures\atm-daemon.exe",
                "user_id": r"DOMAIN\agent",
            },
        }
        with mock.patch.object(
            SERVICE,
            "_windows_task_names",
            return_value=["native", "crosshost-smoke"],
        ):
            candidates = SERVICE._windows_candidates(
                selector,
                statuses.__getitem__,
                {r"domain\agent"},
            )
        self.assertEqual(candidates, [("native", None)])
        self.assertNotIn("crosshost-smoke", [name for name, _path in candidates])
        self.assert_refuses([], "no current-user managed service")
        with mock.patch.object(
            SERVICE,
            "_windows_task_names",
            return_value=["native", "second"],
        ):
            candidates = SERVICE._windows_candidates(
                selector,
                statuses.__getitem__,
                {r"domain\agent"},
            )
        self.assert_refuses(candidates, "multiple current-user managed services")

    def test_parser_rejects_discovery_with_explicit_service_or_plist(self) -> None:
        with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            SWITCH.parser().parse_args(
                ["switch", "--prerelease", "1.5.18", "--service", "native", "--discover-managed-service"]
            )
        args = SWITCH.parser().parse_args(
            ["switch", "--prerelease", "1.5.18", "--launch-agent-plist", "/tmp/native.plist", "--discover-managed-service"]
        )
        with self.assertRaisesRegex(SWITCH.SwitchError, "cannot be combined"):
            SWITCH.resolve_managed_service(args)

    def test_zero_candidate_refuses_before_pair_or_service_mutation(self) -> None:
        argv = [
            "daemon-switch.py",
            "switch",
            "--prerelease",
            "1.5.18",
            "--yes",
            "--discover-managed-service",
        ]
        with (
            mock.patch.object(SWITCH.sys, "argv", argv),
            mock.patch.object(SWITCH, "resolve_managed_service", side_effect=SWITCH.SwitchError("no current-user managed service")),
            mock.patch.object(SWITCH, "resolve_prerelease_pair") as resolve_pair,
            mock.patch.object(SWITCH, "replace_link") as replace_link,
            mock.patch.object(SWITCH, "run_service") as run_service,
            redirect_stderr(io.StringIO()) as stderr,
        ):
            self.assertEqual(SWITCH.main(), 2)
        self.assertIn("no current-user managed service", stderr.getvalue())
        resolve_pair.assert_not_called()
        replace_link.assert_not_called()
        run_service.assert_not_called()


if __name__ == "__main__":
    unittest.main()
