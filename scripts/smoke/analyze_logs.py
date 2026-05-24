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
    warning_records: list[str]
    error_records: list[str]

    @property
    def passed(self) -> bool:
        return not self.missing_events and not self.warning_records and not self.error_records


def analyze_log_text(text: str, expected_events: list[str]) -> LogAnalysisResult:
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    missing = [event for event in expected_events if event not in text]
    warnings: list[str] = []
    errors: list[str] = []
    for line in lines:
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            lowered = line.lower()
            if " error " in f" {lowered} " or lowered.startswith("error"):
                errors.append(line)
            elif " warn " in f" {lowered} " or lowered.startswith("warn"):
                warnings.append(line)
            continue

        level = str(payload.get("level", "")).lower()
        if level in {"error", "fatal"}:
            errors.append(line)
        elif level == "warn":
            warnings.append(line)

    return LogAnalysisResult(
        expected_events=expected_events,
        missing_events=missing,
        warning_records=warnings,
        error_records=errors,
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
                "warning_records": result.warning_records,
                "error_records": result.error_records,
            },
            indent=2,
        )
    )
    return 0 if result.passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
