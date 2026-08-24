#!/usr/bin/env python3
"""Stage only validated benchmark-report artifacts for the operator's commit."""
from __future__ import annotations

from pathlib import Path
import subprocess
import sys
from typing import Callable, Sequence


ROOT = Path(__file__).resolve().parents[2]
REPORT_ARTIFACTS = (
    Path("site/reports/send-message-benchmark"),
    Path("site/reports/send-message-benchmark.json"),
    Path("site/reports/index.html"),
)
Run = Callable[..., subprocess.CompletedProcess[str]]


def publish(root: Path = ROOT, run: Run = subprocess.run) -> None:
    """Validate the report index, then stage the fixed public report surface."""
    checked = run(
        ["just", "reports-index", "--check"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if checked.returncode:
        detail = checked.stderr.strip() or checked.stdout.strip() or "reports-index check failed"
        raise RuntimeError(detail)
    run(
        ["git", "add", "--", *(path.as_posix() for path in REPORT_ARTIFACTS)],
        cwd=root,
        check=True,
        text=True,
    )


def main(argv: Sequence[str] | None = None) -> int:
    if argv:
        print("benchmark-publish: no arguments are accepted", file=sys.stderr)
        return 2
    try:
        publish()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as exc:
        print(f"benchmark-publish: {exc}", file=sys.stderr)
        return 1
    print("benchmark report artifacts staged; commit and push the reviewed campaign.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
