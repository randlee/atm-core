#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import argparse
import json


@dataclass(frozen=True)
class LogAnalysisResult:
    expected_events: list[str]
    missing_events: list[str]
    warning_lines: list[str]
    error_lines: list[str]

    @property
    def passed(self) -> bool:
        return not self.missing_events and not self.warning_lines and not self.error_lines


def analyze_log_text(text: str, expected_events: list[str]) -> LogAnalysisResult:
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    missing = [event for event in expected_events if event not in text]
    warnings = [line for line in lines if " warn " in f" {line.lower()} " or line.lower().startswith("warn")]
    errors = [line for line in lines if " error " in f" {line.lower()} " or line.lower().startswith("error")]
    return LogAnalysisResult(
        expected_events=expected_events,
        missing_events=missing,
        warning_lines=warnings,
        error_lines=errors,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Analyze retained smoke log output.")
    parser.add_argument("log_file", type=Path)
    parser.add_argument("--expect", action="append", default=[])
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    text = args.log_file.read_text(encoding="utf-8")
    result = analyze_log_text(text, args.expect)
    print(
        json.dumps(
            {
                "passed": result.passed,
                "expected_events": result.expected_events,
                "missing_events": result.missing_events,
                "warning_lines": result.warning_lines,
                "error_lines": result.error_lines,
            },
            indent=2,
        )
    )
    return 0 if result.passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
