#!/usr/bin/env python3
"""Run the bounded AI.51 campaign against the real local HTTP frame reader."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
import platform
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.fuzz.render_report import FuzzReportError, render_campaign


SCHEMA_VERSION = "adversarial-fuzzing/v1"
DEFAULT_SEED = 51_051
DEFAULT_CASES = 128
MAX_WORKERS = 4
MAX_CASES = 996
TEST_PREFIX = "api::http_frame_reader::ai51_campaign::"
AI48_CAMPAIGN_FIELDS = (
    "worktree_path",
    "target",
    "baseline_ref",
    "seed",
    "max_workers",
    "cases_per_worker",
    "per_worker_timeout_s",
    "promote_regressions",
    "notes",
)


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


CASE_INDEX = re.compile(r"AI51 case (\d+)")


def run_test(test_name: str, seed: int, cases: int, timeout_s: int, case_start: int = 0) -> dict[str, Any]:
    command = command_for(test_name, seed, cases)
    environment = {
        "ATM_AI51_SEED": str(seed),
        "ATM_AI51_CASES": str(cases),
        "ATM_AI51_CASE_START": str(case_start),
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
        attempt = {
            "ok": False,
            "timed_out": False,
            "command": command,
            "stdout": "",
            "stderr": str(error),
        }
        return {**attempt, "cases": cases, "case_start": case_start}
    except subprocess.TimeoutExpired as error:
        attempt = {
            "ok": False,
            "timed_out": True,
            "command": command,
            "stdout": error.stdout or "",
            "stderr": error.stderr or "",
        }
        return {**attempt, "cases": cases, "case_start": case_start}
    attempt = {
        "ok": result.returncode == 0,
        "timed_out": False,
        "command": command,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }
    return {**attempt, "cases": cases, "case_start": case_start}


def run_probe(test_name: str, seed: int, cases: int, timeout_s: int) -> list[dict[str, Any]]:
    """Minimize an observed failure to one deterministic case, then replay it three times."""
    attempts = [{**run_test(test_name, seed, cases, timeout_s), "stage": "initial"}]
    if attempts[0]["ok"]:
        return attempts
    output = f"{attempts[0]['stdout']}\n{attempts[0]['stderr']}"
    match = CASE_INDEX.search(output)
    if match is None:
        return attempts
    case_start = int(match.group(1))
    minimized = run_test(test_name, seed, 1, timeout_s, case_start)
    attempts.append({**minimized, "stage": "minimize"})
    if minimized["ok"]:
        return attempts
    for _ in range(3):
        attempts.append({**run_test(test_name, seed, 1, timeout_s, case_start), "stage": "replay"})
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
    attempts: list[dict[str, Any]],
    cases: int,
    benign_summary: str,
) -> dict[str, Any]:
    candidate_observed = not attempts[0]["ok"]
    all_ok = all(attempt["ok"] for attempt in attempts)
    minimized = next((attempt for attempt in attempts if attempt.get("stage") == "minimize"), None)
    replays = [attempt for attempt in attempts if attempt.get("stage") == "replay"]
    consistently_failed = minimized is not None and not minimized["ok"] and len(replays) == 3 and all(not attempt["ok"] for attempt in replays)
    timed_out = any(attempt["timed_out"] for attempt in attempts)
    replayed_cleanly = minimized is not None and not minimized["ok"] and len(replays) == 3 and all(attempt["ok"] for attempt in replays)
    if consistently_failed:
        status, classification, outcome_kind = "failed", "confirmed_bug", "confirmed_bug"
        outcome = "Candidate reproduced in three independent real-reader executions."
    elif timed_out:
        status, classification, outcome_kind = "timed_out", "inconclusive", "inconclusive"
        outcome = "Candidate replay was inconclusive because at least one bounded execution timed out."
    elif minimized is not None and minimized["ok"]:
        status, classification, outcome_kind = "success", "pass", "non_repro"
        outcome = "Candidate did not reproduce when minimized to its single deterministic case."
    elif replayed_cleanly:
        status, classification, outcome_kind = "success", "pass", "non_repro"
        outcome = "Minimized candidate did not reproduce in three independent replays."
    elif candidate_observed:
        status, classification, outcome_kind = "success", "inconclusive", "inconclusive"
        outcome = "Candidate replay produced mixed results and remains inconclusive."
    else:
        status, classification, outcome_kind = "success", "pass", "benign"
        outcome = benign_summary
    cases_run = sum(attempt.get("cases", cases) for attempt in attempts)
    passed = sum(attempt.get("cases", cases) for attempt in attempts if attempt["ok"])
    result: dict[str, Any] = {
        "correlation_id": worker_id,
        "target": "local-http-framing",
        "status": status,
        "classification": classification,
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
                "minimal_input": (
                    f"case_start={attempts[-1].get('case_start', 0)}; "
                    f"{len(attempts)} independent deterministic execution(s)"
                ),
                "passed": all_ok,
                "outcome": outcome,
            }
        ],
        "candidate_outcome": outcome_kind,
        "attempts": attempts,
    }
    if candidate_observed:
        result["finding_ids"] = [f"AI51-{worker_id.upper()}-001"]
        if outcome_kind != "non_repro":
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
        ledger[kind].append({"candidate_id": item["correlation_id"], "outcome": kind, "detail": item["summary"]})
    return ledger


def build_campaign(seed: int, cases: int, timeout_s: int, baseline_ref: str) -> dict[str, Any]:
    benign = run_probe("benign_fragment_and_coalesce", seed, cases, timeout_s)
    replay_attempts = run_probe("candidate_replay", seed, cases, timeout_s)
    boundary = run_probe("known_boundaries", seed, 4, timeout_s)
    differential = run_probe("optimized_scalar_parity", seed, cases, timeout_s)
    workers = [
        worker(
            "shape-probe",
            "valid frames fragmented and coalesced at deterministic byte boundaries",
            benign,
            cases,
            f"{cases} valid framed requests accepted by the real reader.",
        ),
        worker(
            "template-probe",
            "prior-candidate replay: delimiter/body boundary split across independent reader executions",
            replay_attempts,
            cases,
            f"{cases} delimiter/body boundary cases preserved the next pipelined request.",
        ),
        worker(
            "boundary-probe",
            "malformed Content-Length, duplicate header, invalid UTF-8, and truncated frame boundaries",
            boundary,
            4,
            "Known malformed inputs returned their documented typed errors; no defect candidate remains.",
        ),
        worker(
            "differential-probe",
            "optimized memmem delimiter finder compared with the scalar reference reader",
            differential,
            cases,
            f"{cases} deterministic cases had identical optimized and scalar reader results.",
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


def validate_with_ai48_contract(payload: dict[str, Any]) -> dict[str, Any]:
    """Validate the actual campaign configuration through the shared `just fuzz` contract."""
    contract = {field: payload["campaign"][field] for field in AI48_CAMPAIGN_FIELDS}
    with tempfile.TemporaryDirectory(prefix="atm-ai51-fuzz-") as temporary:
        input_path = Path(temporary) / "campaign.json"
        input_path.write_text(json.dumps(contract, sort_keys=True), encoding="utf-8")
        result = subprocess.run(
            ["just", "fuzz", "--campaign", str(input_path), "--dry-run"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
    if result.returncode != 0:
        raise CampaignError(f"AI.48 fuzz contract rejected campaign: {result.stderr.strip() or result.stdout.strip()}")
    try:
        planned = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise CampaignError("AI.48 fuzz contract emitted invalid JSON") from error
    worker_ids = [worker.get("correlation_id") for worker in planned.get("workers", [])]
    if planned.get("schema_version") != SCHEMA_VERSION or worker_ids != [
        "shape-probe",
        "template-probe",
        "boundary-probe",
        "differential-probe",
    ]:
        raise CampaignError("AI.48 fuzz contract did not validate the four local framing workers")
    return {
        "workflow": "just fuzz",
        "mode": "dry-run configuration validation",
        "worker_ids": worker_ids,
    }


def compare_repeat(previous_path: Path, payload: dict[str, Any]) -> dict[str, Any]:
    """Require a same-seed/baseline repeat to retain the same classifications."""
    try:
        previous = json.loads(previous_path.read_text(encoding="utf-8"))
        previous_campaign = previous["campaign"]
        previous_workers = {
            section["id"]: section["json_payload"]
            for section in previous["sections"]
        }
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise CampaignError(f"unable to read repeat comparison artifact: {previous_path}") from error
    current_campaign = payload["campaign"]
    for field in ("target", "baseline_ref", "seed", "cases_per_worker"):
        if previous_campaign.get(field) != current_campaign.get(field):
            raise CampaignError(f"repeat comparison requires matching {field}")
    current_workers = {worker["correlation_id"]: worker for worker in payload["workers"]}
    if set(previous_workers) != set(current_workers):
        raise CampaignError("repeat comparison has different worker identities")
    for worker_id, current in current_workers.items():
        previous_worker = previous_workers[worker_id]
        if (previous_worker.get("candidate_outcome"), previous_worker.get("classification")) != (
            current["candidate_outcome"],
            current["classification"],
        ):
            raise CampaignError(f"repeat comparison changed classification for {worker_id}")
    return {
        "compared_to": previous_path.name,
        "same_seed_baseline": True,
        "classifications_match": True,
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run the bounded AI.51 real local HTTP framing campaign.")
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--cases", type=int, default=DEFAULT_CASES)
    parser.add_argument("--timeout-seconds", type=int, default=120)
    parser.add_argument("--baseline-ref", default="integrate/phase-ai-31-33")
    parser.add_argument("--stem", required=True)
    parser.add_argument("--reports-root", type=Path)
    parser.add_argument("--compare-with", type=Path, help="prior same-seed report JSON sidecar")
    parser.add_argument("--no-index", action="store_true")
    args = parser.parse_args(argv[1:])
    if not 0 <= args.seed <= 2**63 - 1:
        parser.error("--seed must be between 0 and 2^63-1")
    if not 1 <= args.cases <= MAX_CASES:
        parser.error(f"--cases must be between 1 and {MAX_CASES}; four exact candidate replays remain within the public worker cap")
    if not 1 <= args.timeout_seconds <= 600:
        parser.error("--timeout-seconds must be between 1 and 600")
    try:
        payload = build_campaign(args.seed, args.cases, args.timeout_seconds, args.baseline_ref)
        payload["campaign"]["ai48_contract"] = validate_with_ai48_contract(payload)
        if args.compare_with is not None:
            payload["campaign"]["repeat_comparison"] = compare_repeat(args.compare_with, payload)
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
