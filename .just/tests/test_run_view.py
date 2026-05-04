from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_view import build_tasks
from run_view import main
from run_view import resolve_task_names


class RunViewTests(unittest.TestCase):
    def test_resolve_task_names_all_includes_expected_targets(self) -> None:
        self.assertEqual(resolve_task_names("all"), ["boundaries", "modules", "deps", "unsafe"])

    def test_resolve_task_names_rejects_unknown_target(self) -> None:
        with self.assertRaises(ValueError):
            resolve_task_names("unknown")

    def test_build_tasks_contains_expected_scripts(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            tasks = build_tasks(repo_root)
            self.assertEqual(tasks["boundaries"].command[-1], str(repo_root / ".just/view_boundaries.py"))
            self.assertEqual(tasks["modules"].command[-1], str(repo_root / ".just/view_modules.py"))
            self.assertEqual(tasks["deps"].command[-1], str(repo_root / ".just/view_deps.py"))
            self.assertEqual(tasks["unsafe"].command[-1], str(repo_root / ".just/view_unsafe.py"))

    @mock.patch("run_view.print_result")
    @mock.patch("run_view.run_task")
    @mock.patch("run_view.build_site")
    @mock.patch("run_view.discover_repo_root")
    def test_main_builds_site_after_running_targets(
        self,
        discover_repo_root_mock: mock.Mock,
        build_site_mock: mock.Mock,
        run_task_mock: mock.Mock,
        print_result_mock: mock.Mock,
    ) -> None:
        repo_root = Path("/tmp/repo")
        discover_repo_root_mock.return_value = repo_root
        build_site_mock.return_value = repo_root / "artifacts/view/index.html"
        task = build_tasks(repo_root)["boundaries"]
        run_task_mock.return_value = mock.Mock(returncode=0, task=task, stdout="ok\n", stderr="", duration_seconds=0.1)
        result = main(["run_view.py", "boundaries"])
        self.assertEqual(result, 0)
        build_site_mock.assert_called_once_with(repo_root)


if __name__ == "__main__":
    unittest.main()
