from __future__ import annotations

from pathlib import Path
import json
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_sc_portability import command
from lint_sc_portability import run


class LintScPortabilityTests(unittest.TestCase):
    def test_command_runs_sc_lint_boundary_portability_json(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            cmd = command(repo_root)
            self.assertEqual(
                cmd,
                [
                    "cargo",
                    "run",
                    "-q",
                    "-p",
                    "sc-lint-boundary",
                    "--",
                    "analyze",
                    "--root",
                    str(repo_root),
                    "--rule",
                    "portability",
                    "--format",
                    "json",
                ],
            )

    @mock.patch("lint_sc_portability.print_report")
    @mock.patch("lint_sc_portability.build_report")
    @mock.patch("lint_sc_portability.subprocess.run")
    def test_run_reports_pass_from_json_payload(
        self,
        subprocess_run_mock: mock.Mock,
        build_report_mock: mock.Mock,
        print_report_mock: mock.Mock,
    ) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text('[workspace]\nmembers=["crates/example"]\nresolver="2"\n', encoding="utf-8")
            crate_dir = repo_root / "crates" / "example"
            crate_dir.mkdir(parents=True)
            (crate_dir / "Cargo.toml").write_text('[package]\nname="example"\nversion="0.1.0"\n', encoding="utf-8")
            subprocess_run_mock.return_value = mock.Mock(
                returncode=0,
                stdout=json.dumps({"status": "pass", "findings": []}),
                stderr="",
            )
            build_report_mock.return_value = mock.Mock(log_path=repo_root / ".just/logs/example.log")

            self.assertEqual(run(repo_root), 0)
            self.assertTrue(build_report_mock.called)
            print_report_mock.assert_called_once()

    @mock.patch("lint_sc_portability.print_report")
    @mock.patch("lint_sc_portability.build_report")
    @mock.patch("lint_sc_portability.subprocess.run")
    def test_run_reports_fail_from_json_payload(
        self,
        subprocess_run_mock: mock.Mock,
        build_report_mock: mock.Mock,
        print_report_mock: mock.Mock,
    ) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text('[workspace]\nmembers=["crates/example"]\nresolver="2"\n', encoding="utf-8")
            crate_dir = repo_root / "crates" / "example"
            crate_dir.mkdir(parents=True)
            (crate_dir / "Cargo.toml").write_text('[package]\nname="example"\nversion="0.1.0"\n', encoding="utf-8")
            subprocess_run_mock.return_value = mock.Mock(
                returncode=0,
                stdout=json.dumps(
                    {"status": "fail", "findings": [{"message": "crates/example/tests/foo.rs:12: PORT-001 hardcoded Unix-only absolute path"}]}
                ),
                stderr="",
            )
            build_report_mock.return_value = mock.Mock(log_path=repo_root / ".just/logs/example.log")

            self.assertEqual(run(repo_root), 1)
            self.assertTrue(build_report_mock.called)
            print_report_mock.assert_called_once()


if __name__ == "__main__":
    unittest.main()
