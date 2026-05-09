from __future__ import annotations

from pathlib import Path
import sys
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from view_deps import analyze_command
from view_deps import visualize_command


class ViewDepsTests(unittest.TestCase):
    def test_visualize_command_uses_output_path_and_repo_root(self) -> None:
        output_path = Path("/tmp/report/index.html")
        repo_root = Path("/tmp/repo")
        self.assertEqual(
            visualize_command(output_path, repo_root),
            [
                "cargo",
                "dep-insight",
                "visualize",
                "--no-open",
                "--out",
                str(output_path),
                str(repo_root),
            ],
        )

    def test_analyze_command_uses_html_and_json_paths(self) -> None:
        html_path = Path("/tmp/report/report.html")
        json_path = Path("/tmp/report/report.json")
        repo_root = Path("/tmp/repo")
        self.assertEqual(
            analyze_command(html_path, json_path, repo_root),
            [
                "cargo",
                "dep-insight",
                "analyze",
                "--html",
                str(html_path),
                "--json",
                str(json_path),
                str(repo_root),
            ],
        )


if __name__ == "__main__":
    unittest.main()
