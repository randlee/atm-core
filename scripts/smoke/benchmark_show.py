#!/usr/bin/env python3
"""Open the newest rebuilt benchmark campaign panel in Wyvern."""
from __future__ import annotations

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke.benchmark_report import BenchmarkReportError, preview_latest


def main() -> int:
    try:
        output = preview_latest()
    except (BenchmarkReportError, OSError) as exc:
        print(f"benchmark-show: {exc}", file=sys.stderr)
        return 1
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
