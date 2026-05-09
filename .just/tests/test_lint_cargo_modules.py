from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_cargo_modules import module_cycle_command
from lint_cargo_modules import run


class LintCargoModulesTests(unittest.TestCase):
    def test_module_cycle_command_includes_acyclic(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            manifest_path = repo_root / "crates" / "atm-core" / "Cargo.toml"
            manifest_path.parent.mkdir(parents=True)
            manifest_path.write_text(
                '[package]\nname = "agent-team-mail-core"\nversion = "1.1.2"\n[lib]\nname = "atm_core"\n',
                encoding="utf-8",
            )
            command = module_cycle_command(repo_root, manifest_path, "agent-team-mail-core")
            self.assertIn("--acyclic", command)
            self.assertIn("--layout", command)

    @mock.patch("lint_cargo_modules.build_report")
    @mock.patch("lint_cargo_modules.subprocess.run")
    def test_run_reports_pass_when_all_crates_are_acyclic(
        self,
        subprocess_run_mock: mock.Mock,
        build_report_mock: mock.Mock,
    ) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/atm-core"]\nresolver = "2"\n', encoding="utf-8")
            crate_dir = repo_root / "crates" / "atm-core"
            crate_dir.mkdir(parents=True)
            (crate_dir / "Cargo.toml").write_text('[package]\nname = "agent-team-mail-core"\nversion = "1.1.2"\n[lib]\nname = "atm_core"\n', encoding="utf-8")
            subprocess_run_mock.return_value = mock.Mock(returncode=0, stdout="ok\n", stderr="")
            self.assertEqual(run(repo_root), 0)
            self.assertTrue(build_report_mock.called)
