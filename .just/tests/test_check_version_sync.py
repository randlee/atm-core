from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_version_sync import replace_version_occurrences
from check_version_sync import specifier_admits
from check_version_sync import validate_python_dependency_pins
from lint_common import discover_repo_root
from prerelease_tag import copy_tracked_files
from prerelease_tag import sync_python_version
from prerelease_tag import workspace_version


class CheckVersionSyncTests(unittest.TestCase):
    def test_replace_version_occurrences_rejects_adjacent_version_component(self) -> None:
        self.assertEqual(
            replace_version_occurrences(
                "https://example.invalid/atm_1.1.2.0_x86_64.zip", "1.1.2", "1.1.3"
            ),
            "https://example.invalid/atm_1.1.2.0_x86_64.zip",
        )

    @mock.patch("prerelease_tag.subprocess.run")
    def test_sync_python_version_propagates_owner_failure(self, run: mock.Mock) -> None:
        run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="owner failed"
        )
        with self.assertRaisesRegex(SystemExit, "owner failed"):
            sync_python_version(Path("/tmp/atm"), Path("/tmp/atm/crates/example/pyproject.toml"))

    @unittest.skipUnless(hasattr(os, "symlink"), "symlink support is required")
    def test_copy_tracked_files_excludes_untracked_content_and_preserves_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            source = Path(tempdir) / "source"
            destination = Path(tempdir) / "destination"
            source.mkdir()
            (source / "tracked.txt").write_text("tracked", encoding="utf-8")
            (source / "target.txt").write_text("target", encoding="utf-8")
            (source / "link.txt").symlink_to("target.txt")
            (source / "secret.txt").write_text("untracked", encoding="utf-8")
            subprocess.run(["git", "init", "-q"], cwd=source, check=True)
            subprocess.run(["git", "add", "tracked.txt", "target.txt", "link.txt"], cwd=source, check=True)
            copy_tracked_files(source, destination)
            self.assertTrue((destination / "tracked.txt").is_file())
            self.assertFalse((destination / "secret.txt").exists())
            self.assertTrue((destination / "link.txt").is_symlink())
            self.assertEqual(os.readlink(destination / "link.txt"), "target.txt")

    def test_specifier_admits_evaluates_comma_separated_bounds(self) -> None:
        self.assertTrue(specifier_admits("1.5.3", ">=1.5,<1.6"))
        self.assertFalse(specifier_admits("1.5.3", ">=1.4,<1.5"))
        self.assertTrue(specifier_admits("1.5.3", "==1.5.3"))
        self.assertFalse(specifier_admits("1.5.3", "!=1.5.3"))

    def _write_pyproject(
        self,
        path: Path,
        name: str,
        *,
        dynamic_version: bool,
        dependencies: list[str] | None = None,
        version: str = "0.1.0",
    ) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        lines = ["[project]", f'name = "{name}"']
        if dynamic_version:
            lines.append('dynamic = ["version"]')
        else:
            lines.append(f'version = "{version}"')
        if dependencies is not None:
            deps = ", ".join(f'"{dep}"' for dep in dependencies)
            lines.append(f"dependencies = [{deps}]")
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    def test_validate_python_dependency_pins_rejects_stale_specifier(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self._write_pyproject(
                repo_root / "crates/atm-graft-python/pyproject.toml",
                "atm-graft",
                dynamic_version=True,
            )
            self._write_pyproject(
                repo_root / "crates/hermes-atm/pyproject.toml",
                "hermes-atm",
                dynamic_version=False,
                dependencies=["atm-graft>=1.4,<1.5"],
            )

            with self.assertRaises(SystemExit) as error:
                validate_python_dependency_pins(repo_root, "1.5.3")

            message = str(error.exception)
            self.assertIn("crates/hermes-atm/pyproject.toml", message)
            self.assertIn("does not admit", message)

    def test_validate_python_dependency_pins_accepts_tracking_specifier(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self._write_pyproject(
                repo_root / "crates/atm-graft-python/pyproject.toml",
                "atm-graft",
                dynamic_version=True,
            )
            self._write_pyproject(
                repo_root / "crates/hermes-atm/pyproject.toml",
                "hermes-atm",
                dynamic_version=False,
                dependencies=["atm-graft>=1.5,<1.6"],
            )

            self.assertTrue(validate_python_dependency_pins(repo_root, "1.5.3"))

    def _write_python_distributions(self, repo_root: Path, names: list[str]) -> None:
        manifest = repo_root / "release/publish-artifacts.toml"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        blocks = [f'[[python_distributions]]\nname = "{name}"\nsource = "crates/{name}"' for name in names]
        manifest.write_text("\n\n".join(blocks) + "\n", encoding="utf-8")

    def test_validate_python_dependency_pins_tracks_static_workspace_distribution(self) -> None:
        # The publish kit requires a static [project].version; the stale-pin gate
        # must keep covering atm-graft through the manifest's python_distributions.
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self._write_python_distributions(repo_root, ["atm-graft"])
            self._write_pyproject(
                repo_root / "crates/atm-graft-python/pyproject.toml",
                "atm-graft",
                dynamic_version=False,
                version="1.6.0",
            )
            self._write_pyproject(
                repo_root / "crates/hermes-atm/pyproject.toml",
                "hermes-atm",
                dynamic_version=False,
                dependencies=["atm-graft>=1.5,<1.6"],
            )

            with self.assertRaises(SystemExit) as error:
                validate_python_dependency_pins(repo_root, "1.6.0")

            message = str(error.exception)
            self.assertIn("crates/hermes-atm/pyproject.toml", message)
            self.assertIn("does not admit", message)

    def test_validate_python_dependency_pins_accepts_static_tracking_specifier(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self._write_python_distributions(repo_root, ["atm-graft"])
            self._write_pyproject(
                repo_root / "crates/atm-graft-python/pyproject.toml",
                "atm-graft",
                dynamic_version=False,
                version="1.6.0",
            )
            self._write_pyproject(
                repo_root / "crates/hermes-atm/pyproject.toml",
                "hermes-atm",
                dynamic_version=False,
                dependencies=["atm-graft>=1.6,<1.7"],
            )

            self.assertTrue(validate_python_dependency_pins(repo_root, "1.6.0"))

    def test_validate_python_dependency_pins_rejects_static_distribution_version_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self._write_python_distributions(repo_root, ["atm-graft"])
            self._write_pyproject(
                repo_root / "crates/atm-graft-python/pyproject.toml",
                "atm-graft",
                dynamic_version=False,
                version="1.5.9",
            )

            with self.assertRaises(SystemExit) as error:
                validate_python_dependency_pins(repo_root, "1.6.0")

            message = str(error.exception)
            self.assertIn("crates/atm-graft-python/pyproject.toml", message)
            self.assertIn("does not match the workspace version", message)

    def test_validate_python_dependency_pins_covers_real_workspace_distributions(self) -> None:
        # Non-synthetic: the real repository must still check hermes-atm's atm-graft pin.
        repo_root = discover_repo_root()
        self.assertTrue(validate_python_dependency_pins(repo_root, workspace_version(repo_root)))


if __name__ == "__main__":
    unittest.main()
