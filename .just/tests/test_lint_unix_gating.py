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

from lint_unix_gating import command
from lint_unix_gating import run


class LintUnixGatingTests(unittest.TestCase):
    def test_command_runs_sc_lint_boundary_portability_json(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.assertEqual(
                command(repo_root),
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

    @mock.patch("lint_unix_gating.print_report")
    @mock.patch("lint_unix_gating.build_report")
    @mock.patch("lint_unix_gating.subprocess.run")
    def test_run_filters_non_unix_portability_findings(
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
                    {
                        "status": "fail",
                        "findings": [
                            {"rule_id": "PORT-003", "message": "ignore me"},
                            {"rule_id": "PORT-004", "message": "relevant"},
                        ],
                    }
                ),
                stderr="",
            )
            build_report_mock.return_value = mock.Mock(
                log_path=repo_root / ".just/logs/example.log",
                passed=False,
            )

            self.assertEqual(run(repo_root), 1)
            _, kwargs = build_report_mock.call_args
            self.assertEqual(kwargs["findings"], ["relevant"])
            print_report_mock.assert_called_once()


if __name__ == "__main__":
    unittest.main()
