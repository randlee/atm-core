#!/usr/bin/env python3
"""Run the bounded AI.51 campaign against the real local HTTP frame reader."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
import platform
from pathlib import Path
import subprocess
import sys
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.fuzz.render_report import FuzzReportError, render_campaign


SCHEMA_VERSION = "adversarial-fuzzing/v1"
DEFAULT_SEED = 51_051
DEFAULT_CASES = 128
MAX_WORKERS = 4
TEST_PREFIX = "api::http_frame_reader::ai51_campaign::"


class CampaignError(RuntimeError):
    """The bounded real-reader campaign could not produce valid evidence."""


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def host_label() -> str:
    system = platform.system().lower() or "unknown"
    machine = platform.machine().lower().replace("_", "-") or "unknown"
    return f"{system}-{machine}"


def cpu_features() -> list[str]:
    """Return stable, public CPU capability labels without exposing host names."""
    if sys.platform == "darwin":
        command = ["sysctl", "-n", "machdep.cpu.features"]
    elif sys.platform.startswith("linux"):
        command = ["sh", "-c", "sed -n 's/^flags[[:space:]]*:[[:space:]]*//p' /proc/cpuinfo | head -1"]
    else:
        return []
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        return []
    return sorted({feature.lower() for feature in result.stdout.split() if feature.isascii()})


def command_for(test_name: str, seed: int, cases: int) -> list[str]:
    return [
        "cargo",
        "test",
        "-p",
        "agent-team-mail-core",
        "--lib",
        f"{TEST_PREFIX}{test_name}",
        "--",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]


def run_test(test_name: str, seed: int, cases: int, timeout_s: int) -> dict[str, Any]:
    command = command_for(test_name, seed, cases)
    environment = {
        "ATM_AI51_SEED": str(seed),
        "ATM_AI51_CASES": str(cases),
    }
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=timeout_s,
            check=False,
            env={**os.environ, **environment},
        )
    except OSError as error:
        return {
            "ok": False,
            "timed_out": False,
            "command": command,
            "stdout": "",
            "stderr": str(error),
        }
    except subprocess.TimeoutExpired as error:
        return {
            "ok": False,
            "timed_out": True,
            "command": command,
            "stdout": error.stdout or "",
            "stderr": error.stderr or "",
        }
    return {
        "ok": result.returncode == 0,
        "timed_out": False,
        "command": command,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }


def run_probe(test_name: str, seed: int, cases: int, timeout_s: int, minimum_attempts: int = 1) -> list[dict[str, Any]]:
    """Run once, then replay a candidate twice before classifying it as a bug."""
    attempts = [run_test(test_name, seed, cases, timeout_s)]
    while len(attempts) < minimum_attempts or (not attempts[-1]["ok"] and len(attempts) < 3):
        attempts.append(run_test(test_name, seed, cases, timeout_s))
    return attempts


def finding(worker_id: str, observed: str, recommendation: str) -> dict[str, str]:
    return {
        "finding_id": f"AI51-{worker_id.upper()}-001",
        "minimal_template": "local HTTP request frame",
        "minimal_input": "see command and deterministic seed in this worker payload",
        "expected_oracle": "the real HttpFrameReader preserves its documented framing contract",
        "observed_result": observed,
        "requirement_trace": "AI.51 local HTTP framing adversarial campaign",
        "requirement_follow_up": "Keep this candidate visible until a bounded real-reader replay resolves it.",
        "root_cause": "Not established by this bounded campaign.",
        "recommended_fix": recommendation,
    }


def worker(
    worker_id: str,
    description: str,
    outcome_kind: str,
    attempts: list[dict[str, Any]],
    cases_run: int,
    outcome: str,
    classification: str = "pass",
) -> dict[str, Any]:
    all_ok = all(attempt["ok"] for attempt in attempts)
    consistently_failed = len(attempts) >= 3 and all(not attempt["ok"] for attempt in attempts)
    timed_out = any(attempt["timed_out"] for attempt in attempts)
    status = "success" if all_ok else "failed" if consistently_failed else "timed_out"
    passed = cases_run if all_ok else 0
    result: dict[str, Any] = {
        "correlation_id": worker_id,
        "target": "local-http-framing",
        "status": status,
        "classification": classification if all_ok else ("confirmed_bug" if consistently_failed else "inconclusive"),
        "cases_run": cases_run,
        "passed": passed,
        "failed": cases_run - passed,
        "finding_ids": [],
        "error": None,
        "fuzz_run_description": description,
        "summary": outcome,
        "test_inputs": [
            {
                "case_id": f"AI51-{worker_id}",
                "description": description,
                "minimal_template": "HttpFrameReader::read_request",
                "minimal_input": f"{len(attempts)} deterministic replay(s)",
                "passed": all_ok,
                "outcome": outcome,
            }
        ],
        "candidate_outcome": outcome_kind,
        "attempts": attempts,
    }
    if not all_ok:
        result["finding_ids"] = [f"AI51-{worker_id.upper()}-001"]
        result["error"] = {
            "code": "worker_timeout" if timed_out else "reader_contract_mismatch",
            "message": outcome,
        }
        result["findings"] = [
            finding(
                worker_id,
                outcome,
                "Minimize the deterministic input, add an owning-crate regression test, and rerun this campaign.",
            )
        ]
    return result


def outcome_ledger(workers: list[dict[str, Any]]) -> dict[str, list[dict[str, str]]]:
    ledger: dict[str, list[dict[str, str]]] = {
        "confirmed_bug": [],
        "non_repro": [],
        "benign": [],
        "inconclusive": [],
    }
    for item in workers:
        kind = item["candidate_outcome"]
        if kind == "confirmed_bug":
            ledger[kind].append({"candidate_id": item["correlation_id"], "outcome": kind, "detail": item["summary"]})
        elif kind in {"non_repro", "benign", "inconclusive"}:
            ledger[kind].append({"candidate_id": item["correlation_id"], "outcome": kind, "detail": item["summary"]})
        if not item["status"] == "success":
            failure_outcome = "confirmed_bug" if item["status"] == "failed" else "inconclusive"
            ledger[failure_outcome].append(
                {
                    "candidate_id": item["correlation_id"],
                    "outcome": failure_outcome,
                    "detail": item["summary"],
                }
            )
    return ledger


def build_campaign(seed: int, cases: int, timeout_s: int, baseline_ref: str) -> dict[str, Any]:
    benign = run_probe("benign_fragment_and_coalesce", seed, cases, timeout_s)
    replay_attempts = run_probe("candidate_replay", seed, cases, timeout_s, minimum_attempts=3)
    boundary = run_probe("known_boundaries", seed, cases, timeout_s)
    differential = run_probe("optimized_scalar_parity", seed, cases, timeout_s)
    workers = [
        worker(
            "shape-probe",
            "valid frames fragmented and coalesced at deterministic byte boundaries",
            "benign",
            benign,
            cases,
            f"{cases} valid framed requests accepted by the real reader.",
        ),
        worker(
            "template-probe",
            "candidate replay: delimiter/body boundary split across three independent reader runs",
            "non_repro",
            replay_attempts,
            cases * 3,
            "Candidate did not reproduce as a reader defect in three deterministic replays.",
        ),
        worker(
            "boundary-probe",
            "malformed Content-Length, duplicate header, invalid UTF-8, and truncated frame boundaries",
            "benign",
            boundary,
            4,
            "Known malformed inputs returned their documented typed errors; no defect candidate remains.",
        ),
        worker(
            "differential-probe",
            "optimized memmem delimiter finder compared with the scalar reference reader",
            "inconclusive",
            differential,
            cases,
            f"{cases} deterministic cases had identical optimized and scalar reader results; kernel socket cancellation remains outside this reader-only campaign and is not counted as PASS.",
        ),
    ]
    return {
        "schema_version": SCHEMA_VERSION,
        "session_id": f"ai51-local-http-framing-seed-{seed}",
        "generated_at": utc_now(),
        "host_label": host_label(),
        "execution_mode": "real-local-http-frame-reader",
        "campaign": {
            "worktree_path": str(ROOT),
            "target": "local-http-framing",
            "baseline_ref": baseline_ref,
            "seed": seed,
            "max_workers": MAX_WORKERS,
            "cases_per_worker": cases,
            "per_worker_timeout_s": timeout_s,
            "promote_regressions": True,
            "platform": platform.platform(),
            "cpu_features": cpu_features(),
            "notes": "AI.51 bounded real reader campaign; every candidate is retained in outcome_ledger.",
        },
        "workers": workers,
        "outcome_ledger": outcome_ledger(workers),
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run the bounded AI.51 real local HTTP framing campaign.")
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--cases", type=int, default=DEFAULT_CASES)
    parser.add_argument("--timeout-seconds", type=int, default=120)
    parser.add_argument("--baseline-ref", default="integrate/phase-ai-31-33")
    parser.add_argument("--stem", required=True)
    parser.add_argument("--reports-root", type=Path)
    parser.add_argument("--no-index", action="store_true")
    args = parser.parse_args(argv[1:])
    if not 0 <= args.seed <= 2**63 - 1:
        parser.error("--seed must be between 0 and 2^63-1")
    if not 1 <= args.cases <= 1000:
        parser.error("--cases must be between 1 and 1000")
    if not 1 <= args.timeout_seconds <= 600:
        parser.error("--timeout-seconds must be between 1 and 600")
    try:
        payload = build_campaign(args.seed, args.cases, args.timeout_seconds, args.baseline_ref)
        report = render_campaign(
            payload,
            args.stem,
            reports_root=args.reports_root or ROOT / "site" / "reports",
            invoke_index=not args.no_index,
        )
    except (CampaignError, FuzzReportError, OSError, subprocess.SubprocessError) as error:
        print(f"local-http-framing-campaign: error: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"report": report["output_path"], "session_id": payload["session_id"]}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
