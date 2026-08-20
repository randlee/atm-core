#!/usr/bin/env python3
"""Translate a sc-lint-boundary JSON report into a process exit status."""

from __future__ import annotations

import json
import sys
from pathlib import Path


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: check_boundary_lint_status.py <report.json>", file=sys.stderr)
        return 2

    report_path = Path(argv[1])
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print(f"failed to read boundary-lint report {report_path}: {error}", file=sys.stderr)
        return 2

    status = report.get("status") if isinstance(report, dict) else None
    if status == "pass":
        return 0
    if status == "fail":
        return 1

    print(
        f"boundary-lint report {report_path} has invalid status: {status!r}",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
