from __future__ import annotations

from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]


class SiteReportsDiffTests(unittest.TestCase):
    def test_site_reports_diff_is_generated_only(self):
        result = subprocess.run(
            ["git", "diff", "--name-status", subprocess.check_output(["git", "merge-base", "origin/develop", "HEAD"], cwd=ROOT, text=True).strip(), "--", "site/reports"],
            cwd=ROOT, capture_output=True, text=True, check=True,
        )
        changed = [line.split("\t", 1) for line in result.stdout.splitlines() if line]
        self.assertTrue(all(status == "A" or path == "site/reports/index.html" for status, path in changed))


if __name__ == "__main__":
    unittest.main()
