from __future__ import annotations

from pathlib import Path
import tempfile
import unittest
from unittest import mock

import shell_support


class WindowsPosixShellDiscoveryTests(unittest.TestCase):
    def test_prefers_git_bash_and_rejects_system32_wsl_stub(self) -> None:
        def is_git_bash(candidate: Path) -> bool:
            return str(candidate).replace("/", "\\").casefold() == r"c:\program files\git\bin\bash.exe"

        with (
            mock.patch(
                "shell_support.shutil.which",
                side_effect=lambda name: {
                    "git": r"C:\Program Files\Git\cmd\git.exe",
                    "bash": r"C:\Windows\System32\bash.exe",
                }.get(name),
            ),
            mock.patch("shell_support.Path.is_file", new=is_git_bash),
        ):
            self.assertEqual(
                shell_support.resolve_posix_shell(windows=True, windir=r"C:\Windows"),
                r"C:\Program Files\Git\bin\bash.exe",
            )

    def test_ignores_ambient_git_bash_and_returns_none_without_git_bash(self) -> None:
        with (
            mock.patch.dict("shell_support.os.environ", {"GIT_BASH": r"C:\unsafe"}, clear=False),
            mock.patch("shell_support.shutil.which", return_value=None),
        ):
            self.assertIsNone(shell_support.resolve_posix_shell(windows=True))

    def test_rejects_system32_paths_after_resolution_including_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            system32 = root / "System32"
            system32.mkdir()
            (system32 / "Sub").mkdir()
            literal = system32 / "bash.exe"
            nested = system32 / "Sub" / "bash.exe"
            literal.touch()
            nested.touch()
            linked_system32 = root / "linked-system32"
            try:
                linked_system32.symlink_to(system32, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"symlink support unavailable: {error}")

            linked = linked_system32 / "bash.exe"
            outside = root / "Git" / "bin" / "bash.exe"
            outside.parent.mkdir(parents=True)
            outside.touch()

            self.assertTrue(shell_support._is_system32_wsl_stub(str(literal), str(system32)))
            self.assertTrue(shell_support._is_system32_wsl_stub(str(nested), str(system32)))
            self.assertTrue(shell_support._is_system32_wsl_stub(str(linked), str(system32)))
            self.assertFalse(shell_support._is_system32_wsl_stub(str(outside), str(system32)))


if __name__ == "__main__":
    unittest.main()
