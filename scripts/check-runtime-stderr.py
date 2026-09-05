#!/usr/bin/env python3
"""Reject live runtime stdout/stderr bypasses before retained logging exists."""
from __future__ import annotations

from pathlib import Path
import sys

PRE_BOOTSTRAP_STDERR_ALLOWLIST: frozenset[str] = frozenset({
    # These paths are intentionally empty today. Any future exception must be
    # a pre-logger `file:function` entry and be documented in logging.md.
})
RUNTIME_ROOTS = (
    "crates/atm-daemon-bootstrap/src",
    "crates/atm-http-runtime/src",
    "crates/atm-runtime/src",
)


def production_source(text: str) -> str:
    """Tests may print diagnostics; the first test-only section is excluded."""
    return text.split("#[cfg(test)]", 1)[0]


def violations(root: Path) -> list[str]:
    findings: list[str] = []
    for relative_root in RUNTIME_ROOTS:
        for path in (root / relative_root).rglob("*.rs"):
            if "src/bin" in path.as_posix():
                continue
            relative = path.relative_to(root).as_posix()
            for line_number, line in enumerate(production_source(path.read_text()).splitlines(), 1):
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
