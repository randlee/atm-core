from __future__ import annotations

from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]


def is_allowed_report_change(status: str, path: str) -> bool:
    if status == "A" or path == "site/reports/index.html":
        return True
    # Rand's 2026-09-13 directive permits the site-links gate to repair links
    # in rendered pages; runner-written evidence remains strictly add-only.
    return status == "M" and Path(path).suffix.lower() in {".html", ".xhtml"}


class SiteReportsDiffTests(unittest.TestCase):
    def test_site_reports_diff_is_generated_only(self):
        result = subprocess.run(
            ["git", "diff", "--name-status", subprocess.check_output(["git", "merge-base", "origin/develop", "HEAD"], cwd=ROOT, text=True).strip(), "--", "site/reports"],
            cwd=ROOT, capture_output=True, text=True, check=True,
        )
        changed = [tuple(line.split("\t", 1)) for line in result.stdout.splitlines() if line]
        violations = [pair for pair in changed if not is_allowed_report_change(*pair)]
        self.assertEqual(violations, [])


if __name__ == "__main__":
    unittest.main()
