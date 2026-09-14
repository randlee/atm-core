#!/usr/bin/env python3
"""Fail when committed generated site pages differ from what Pages would publish.

This is the single definition of the Pages build gate: `.github/workflows/pages.yml`,
`just lint site-generated`, and the pre-push hook all run this script, so a
stale procedure page or report index is refused before merge, not after.
"""
from __future__ import annotations

from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
CHECKS = (
    ("scripts/procedures/render_procedure_pages.py", "--check", "just procedures"),
    (".just/generate_report_index.py", "--check", "just reports-index"),
)


def main() -> int:
    for script, flag, regenerate in CHECKS:
        result = subprocess.run([sys.executable, str(ROOT / script), flag], cwd=ROOT, check=False)
        if result.returncode:
            print(f"site-generated: {script} {flag} failed (run `{regenerate}`, then commit)", file=sys.stderr)
            return result.returncode
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
