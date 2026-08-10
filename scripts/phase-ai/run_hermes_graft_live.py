#!/usr/bin/env python3
"""Run the managed-profile ``just smoke graft-hermes`` lane with retained evidence.

The generic graft bridge is exercised only against the operator-selected
Tokio/Axum runtime pair. This runner never starts, stops, switches, or
repairs a daemon: `/daemon-switch` owns that lifecycle before smoke begins.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SMOKE_ROOT = ROOT / "scripts" / "smoke"
if str(SMOKE_ROOT) not in sys.path:
    sys.path.insert(0, str(SMOKE_ROOT))

import run_feature_smoke as feature_smoke


BACKEND = ROOT / "scripts" / "phase-ai" / "run-hermes-graft-smoke.py"


def parse_args() -> list[str]:
    """Leave the public graft-hermes arguments owned by its focused backend."""
    parser = argparse.ArgumentParser(description=__doc__, add_help=False)
    parser.add_argument("--help", action="store_true")
    known, remaining = parser.parse_known_args()
    if known.help:
        print("usage: just smoke graft-hermes [--sender AGENT] [--agent AGENT] [--team TEAM] [--chat-id ID] [--workspace-root PATH] [--timeout SECONDS]")
        raise SystemExit(0)
    return remaining


def doctor_case(cases: list[dict[str, Any]]) -> bool:
    """Require the already-selected, matched runtime pair before graft I/O."""
    try:
        atm, _identity, _team = feature_smoke.require_environment()
        report = feature_smoke.parse_json(feature_smoke.command([atm, "doctor", "--json"]), "doctor")
        expected_version = feature_smoke.branch_version()
        ready = feature_smoke.doctor_ready(report, expected_version)
        daemon_version = report.get("daemon_context", {}).get("version") if isinstance(report, dict) else None
        detail = (
            f"READY · ATM {daemon_version}"
            if ready
            else f"expected={expected_version}; doctor did not report the selected pair ready"
        )
        feature_smoke.add_case(cases, "doctor", ready, detail)
        return ready
    except feature_smoke.SmokeError as error:
        feature_smoke.add_case(cases, "doctor", False, str(error))
        return False


def run_live(arguments: list[str]) -> int:
    """Run one real generic-graft outbound write after daemon-switch preflight."""
    cases: list[dict[str, Any]] = []
    if doctor_case(cases):
        completed = subprocess.run(
            [sys.executable, str(BACKEND), *arguments],
            cwd=ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        if completed.stdout.strip():
            print(completed.stdout.rstrip())
        if completed.stderr.strip():
            print(completed.stderr.rstrip(), file=sys.stderr)
        if completed.returncode == 0:
            feature_smoke.add_case(
                cases,
                "graft outbound durable write and receiver round trip",
                True,
                "installed atm-graft completed send, nudge, read, and acknowledgement through the selected runtime",
            )
        else:
            detail = completed.stderr.strip() or completed.stdout.strip() or "graft smoke backend failed without output"
            feature_smoke.add_case(cases, "graft outbound durable write and receiver round trip", False, detail)

    report = feature_smoke.write_report("graft-hermes", cases)
    passed = all(case["status"] == "PASS" for case in cases)
    print(f"{'PASS' if passed else 'FAIL'} evidence: {report}")
    return 0 if passed else 1


def main() -> int:
    return run_live(parse_args())


if __name__ == "__main__":
    raise SystemExit(main())
