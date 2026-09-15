#!/usr/bin/env python3
"""Reject live runtime stdout/stderr bypasses before retained logging exists."""
from __future__ import annotations

from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / ".just"))
from lint_common import rust_file_test_scope

PRE_BOOTSTRAP_STDERR_ALLOWLIST: frozenset[str] = frozenset({
    # These paths are intentionally empty today. Any future exception must be
    # a pre-logger `file:function` entry and be documented in logging.md.
})
RUNTIME_ROOTS = (
    "crates/atm-daemon-bootstrap/src",
    "crates/atm-http-runtime/src",
    "crates/atm-runtime/src",
)


def violations(root: Path) -> list[str]:
    findings: list[str] = []
    for relative_root in RUNTIME_ROOTS:
        for path in (root / relative_root).rglob("*.rs"):
            if "src/bin" in path.as_posix():
                continue
            relative = path.relative_to(root).as_posix()
            lines = path.read_text().splitlines()
            test_scope = rust_file_test_scope(path, lines)
            for line_number, (line, is_test) in enumerate(zip(lines, test_scope, strict=True), 1):
                if is_test:
                    continue
                if "eprintln!(" in line or "println!(" in line:
                    findings.append(f"{relative}:{line_number}: runtime stdout/stderr bypass")
    return findings


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    findings = violations(root)
    if findings:
        print("runtime stderr gate failed:", file=sys.stderr)
        print("\n".join(findings), file=sys.stderr)
        return 1
    print("runtime stderr gate passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
