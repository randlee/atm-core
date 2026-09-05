#!/usr/bin/env python3
"""Collect #1171 Windows FTS fanout evidence with the candidate daemon."""
from __future__ import annotations

from collections import Counter
import json
import os
from pathlib import Path
import sys
import time


ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke import read_benchmark as read
from scripts.smoke import run_admission_capacity as capacity
from scripts.smoke.benchmark_account import clear_benchmark_database_state
from scripts.smoke.daemon_lifecycle import require_clean_host_daemon_state


def error_code(error: str | None) -> str:
    if not error:
        return "none"
    if "ATM_DAEMON_CONNECTION_SATURATED" in error:
        return "ATM_DAEMON_CONNECTION_SATURATED"
    return "other"


def main() -> int:
    profile = Path(os.environ["USERPROFILE"]).resolve()
    home = profile / ".atm"
    report_dir = ROOT / "site" / "reports" / "issue-1171"
    report_dir.mkdir(parents=True, exist_ok=True)
    require_clean_host_daemon_state(smoke_label="issue-1171 current-account FTS evidence")
    clear_benchmark_database_state()
    roster = capacity.CapacityRoster.unique()
    env = capacity.runtime_environment(home, roster)
    port = capacity.allocate_direct_peer_port()
    env[capacity.CAPACITY_DIRECT_PEER_PORT_ENV] = str(port)
    daemon = capacity.release_binary("atm-daemon")
    atm = capacity.release_binary("atm")
    process = None
    output = None
    try:
        process, output = capacity.start_capacity_daemon(daemon, home, env, "plaintext-test")
        doctor_result = capacity.command_result(
            [str(atm), "doctor", "--json"], timeout=15.0,
            env=capacity.host_runtime_client_environment(env),
        )
        doctor = capacity.benchmark_doctor_payload(doctor_result)
        settings = read.load_effective_lane_settings(str(atm), env)
        corpus = read.deterministic_corpus()
        read.prepare_corpus(str(atm), env, corpus)
        family = read.FAMILY_BY_ID["query-fts"]
        read._run_window(str(atm), family, corpus, env, read.WARMUP_SECONDS)
        log_path = home / "logs" / "atm.log.jsonl"
        log_offset = log_path.stat().st_size if log_path.is_file() else 0
        observations, elapsed = read._run_window(
            str(atm), family, corpus, env, read.MEASUREMENT_SECONDS,
        )
        failures = [item.error for item in observations if not item.success]
        time.sleep(0.25)
        log_delta = ""
        if log_path.is_file():
            with log_path.open("r", encoding="utf-8", errors="replace") as handle:
                handle.seek(log_offset)
                log_delta = handle.read()
        payload = {
            "candidate_sha": capacity.source_revision(),
            "host": os.environ.get("COMPUTERNAME", "unknown"),
            "account": os.environ.get("USERNAME", "unknown"),
            "profile_root": str(profile),
            "runtime_home": str(home),
            "host_label": os.environ.get("ATM_CAPACITY_HOST_LABEL", "unknown"),
            "execution_mode": "current-account; scheduled-task isolation unavailable to unelevated token",
            "workload": {
                "family": family.family_id,
                "fanout": read.FANOUT,
                "warmup_seconds": read.WARMUP_SECONDS,
                "measurement_seconds": read.MEASUREMENT_SECONDS,
            },
            "daemon_port": port,
            "reader_lanes": settings,
            "doctor": doctor,
            "requests": {
                "total": len(observations),
                "successful": sum(item.success for item in observations),
                "failed": len(failures),
                "failure_codes": dict(Counter(error_code(item) for item in failures)),
                "failure_samples": [item for item in failures if item][:10],
                "elapsed_seconds": elapsed,
            },
            "saturation_counter": {
                "doctor_exposed": False,
                "reason": "doctor schema reports reader-lane settings but no saturation counter",
                "structured_log_events_in_measurement_window": log_delta.count(
                    "ATM_DAEMON_CONNECTION_SATURATED"
                ),
            },
            "daemon_output": output.evidence(),
            "limitation": "The checked-in read runner rejects Windows before workload execution because POSIX identity capture is unavailable; this invokes its unchanged corpus, fanout, and observation functions directly.",
        }
        path = report_dir / "query-fts-32-way-current-account.json"
        path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(path)
        return 0
    finally:
        if process is not None:
            capacity.reap_owned_daemon(process)
            output.join()


if __name__ == "__main__":
    raise SystemExit(main())
