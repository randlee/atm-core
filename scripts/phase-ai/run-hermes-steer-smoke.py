#!/usr/bin/env python3
"""Emit deterministic Hermes safe-boundary steer evidence for AI.38."""

from __future__ import annotations

import argparse
import asyncio
import json
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[2]
FIXTURE_ROOT = ROOT / "crates" / "atm-graft-python" / "tests"
if str(FIXTURE_ROOT) not in sys.path:
    sys.path.insert(0, str(FIXTURE_ROOT))

from hermes_steer_fixture import fixture_evidence  # noqa: E402


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--fixture",
        action="store_true",
        help="run the checked-in HermesSteerFixture reference profile",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if not args.fixture:
        raise SystemExit("--fixture is required; downstream Hermes is operational evidence only")
    evidence = asyncio.run(fixture_evidence())
    required = {
        "profile",
        "chat_id",
        "wake_kind",
        "steer_accepted",
        "normal_message_handler_called",
        "current_task_interrupted",
        "mailbox_mutated_by_wake",
    }
    if {row["wake_kind"] for row in evidence} != {"live_nudge", "recovery_summary"}:
        raise AssertionError("fixture must prove both live and recovery wake kinds")
    for row in evidence:
        if set(row) != required or not row["steer_accepted"]:
            raise AssertionError(f"invalid steer evidence row: {row}")
        if any(row[field] for field in (
            "normal_message_handler_called",
            "current_task_interrupted",
            "mailbox_mutated_by_wake",
        )):
            raise AssertionError(f"unsafe steer evidence row: {row}")
    print(json.dumps(evidence, sort_keys=True))


if __name__ == "__main__":
    main()
