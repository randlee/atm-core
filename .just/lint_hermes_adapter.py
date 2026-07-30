#!/usr/bin/env python3
"""Guard the Hermes adapter against retired normal-message ingress symbols."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import sys

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import monotonic_now
from lint_common import print_report


LINT_NAME = "hermes-adapter"
ADAPTER_SOURCE_ROOT = Path("crates/atm-graft-python/python")
RETIRED_NORMAL_INGRESS_SYMBOLS = (
    "MessageEvent",
    "SessionSource",
    "internal=False",
    "inject_user_message",
    "BasePlatformAdapter",
    "register_platform",
)


@dataclass(frozen=True)
class HermesAdapterViolation:
    path: str
    line_number: int
    symbol: str
    line: str

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: retired symbol {self.symbol!r}: {self.line}"


def collect_violations(
    repo_root: Path,
    *,
    retired_symbols: tuple[str, ...] = RETIRED_NORMAL_INGRESS_SYMBOLS,
) -> list[HermesAdapterViolation]:
    source_root = repo_root / ADAPTER_SOURCE_ROOT
    violations: list[HermesAdapterViolation] = []
    for source_path in sorted(source_root.rglob("*.py")):
        relative_path = source_path.relative_to(repo_root).as_posix()
        for line_number, line in enumerate(source_path.read_text(encoding="utf-8").splitlines(), start=1):
            for symbol in retired_symbols:
                if symbol in line:
                    violations.append(
                        HermesAdapterViolation(
                            path=relative_path,
                            line_number=line_number,
                            symbol=symbol,
                            line=line.strip(),
                        )
                    )
    return violations


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Reject retired normal-message ingress symbols in the Hermes adapter source."
    )
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    started_at = datetime.now(timezone.utc)
    start_time = monotonic_now()
    violations = collect_violations(repo_root)
    duration_seconds = monotonic_now() - start_time
    findings = [violation.render() for violation in violations]
    passed = not violations
    summary = (
        "retired normal-message ingress symbols found in Hermes adapter source"
        if violations
        else "Hermes adapter uses the steer boundary without retired normal-message ingress symbols"
    )
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=passed,
        summary=summary,
        findings=findings,
        transcript_lines=["findings:", *(findings or ["none"])],
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print_report(report, repo_root=repo_root, preview_limit=0, direct_threshold=0)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
