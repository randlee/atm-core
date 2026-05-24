#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import asdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import json
import subprocess
import sys
import tempfile
import time

from fixtures import current_binary_sha
from fixtures import repo_root
from fixtures import smoke_paths
from fixtures import write_json


ROW_MAP: dict[str, list[tuple[str, str]]] = {
    "fast": [
        ("Z1-001", "build approved smoke baseline"),
        ("Z1-002", "clean-room daemon/runtime bring-up"),
        ("Z1-003", "retained team/member inspection on clean-room baseline"),
        ("Z1-004", "empty-mailbox retained CLI surface"),
        ("Z1-005", "first clean-room send to config-defined recipient"),
        ("FAST-LOG-001", "expected happy-path retained events are present"),
        ("FAST-LOG-002", "retained logs contain no warnings or errors"),
    ],
    "normal": [
        ("Z1-001", "build approved smoke baseline"),
        ("Z1-002", "clean-room daemon/runtime bring-up"),
        ("Z1-003", "retained team/member inspection on clean-room baseline"),
        ("Z1-004", "empty-mailbox retained CLI surface"),
        ("Z1-005", "first clean-room send to config-defined recipient"),
        ("Z1-007", "retained CLI validation and recovery guidance"),
        ("FAST-LOG-001", "expected happy-path retained events are present"),
        ("FAST-LOG-002", "retained logs contain no warnings or errors"),
    ],
    "thorough": [
        ("Z1-001", "build approved smoke baseline"),
        ("Z1-002", "clean-room daemon/runtime bring-up"),
        ("Z1-003", "retained team/member inspection on clean-room baseline"),
        ("Z1-004", "empty-mailbox retained CLI surface"),
        ("Z1-005", "first clean-room send to config-defined recipient"),
        ("Z1-006", "degraded notification after durable send"),
        ("Z1-007", "retained CLI validation and recovery guidance"),
        ("Z1-008", "copied-state durable baseline bring-up"),
        ("Z1-009", "reconcile/runtime retry-visible smoke coverage"),
        ("FAST-LOG-001", "expected happy-path retained events are present"),
        ("FAST-LOG-002", "retained logs contain no warnings or errors"),
    ],
}


@dataclass(frozen=True)
class SmokeRow:
    id: str
    flow: str
    verdict: str
    notes: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Shared Phase Z smoke runner scaffold.")
    parser.add_argument("level", choices=sorted(ROW_MAP))
    parser.add_argument("--binary-sha", default=None)
    parser.add_argument("--write-artifacts", action="store_true")
    parser.add_argument(
        "--status",
        default="scaffold-only",
        help="Runner status label for the scaffolded output.",
    )
    return parser.parse_args()


def build_payload(level: str, status: str, binary_sha: str) -> dict:
    started = time.perf_counter()
    timestamp = datetime.now(timezone.utc).isoformat()
    rows = [
        asdict(
            SmokeRow(
                id=row_id,
                flow=flow,
                verdict="SKIP",
                notes="scaffold-only runner contract; execution steps land in later smoke sprints",
            )
        )
        for row_id, flow in ROW_MAP[level]
    ]
    duration = round(time.perf_counter() - started, 3)
    return {
        "level": level,
        "timestamp": timestamp,
        "binary_sha": binary_sha,
        "duration_secs": duration,
        "status": status,
        "rows": rows,
        "summary": {
            "pass": 0,
            "fail": 0,
            "skip": len(rows),
        },
    }


def render_markdown(payload_path: Path, write_artifacts: bool) -> None:
    command = [sys.executable, str(repo_root() / "scripts" / "smoke" / "render_report.py"), str(payload_path)]
    if write_artifacts:
        command.append("--write-artifacts")
    subprocess.run(command, check=True)


def main() -> int:
    args = parse_args()
    binary_sha = args.binary_sha or current_binary_sha()
    payload = build_payload(args.level, args.status, binary_sha)
    if args.write_artifacts:
        paths = smoke_paths(args.level)
        write_json(paths.timestamped_json, payload)
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2)
            handle.write("\n")
            temp_payload = Path(handle.name)
        try:
            render_markdown(temp_payload, write_artifacts=True)
        finally:
            temp_payload.unlink(missing_ok=True)
    print(json.dumps(payload, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
