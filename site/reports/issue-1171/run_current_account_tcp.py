#!/usr/bin/env python3
"""Collect #1171 TCP f16/64 evidence after the wrapper snapshot preflight fails.

This uses the candidate's existing direct-peer daemon and profile routines.  It
does not bypass the TCP product path; it only omits the wrapper's incompatible
profile-store snapshot/restore lifecycle.  The caller records that limitation
beside every result.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import time


ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke import run_admission_capacity as capacity
from scripts.smoke.benchmark_account import clear_benchmark_database_state
from scripts.smoke.daemon_lifecycle import require_clean_host_daemon_state


def main() -> int:
    profile = Path(os.environ["USERPROFILE"]).resolve()
    home = profile / ".atm"
    report_dir = ROOT / "site" / "reports" / "issue-1171"
    report_dir.mkdir(parents=True, exist_ok=True)
    daemon = capacity.release_binary("atm-daemon")
    atm = capacity.release_binary("atm")
    results: list[dict[str, object]] = []

    for campaign in range(1, 4):
        require_clean_host_daemon_state(smoke_label="issue-1171 current-account TCP evidence")
        clear_benchmark_database_state()
        roster = capacity.CapacityRoster.unique()
        env = capacity.runtime_environment(home, roster)
        port = capacity.allocate_direct_peer_port()
        env[capacity.CAPACITY_DIRECT_PEER_PORT_ENV] = str(port)
        process = None
        output = None
        started = time.perf_counter()
        try:
            process, output = capacity.start_capacity_daemon(
                daemon, home, env, "plaintext-test",
            )
            doctor = capacity.command_result(
                [str(atm), "doctor", "--json"],
                timeout=15.0,
                env=capacity.host_runtime_client_environment(env),
            )
            capacity.prepare_capacity_roster(atm, env, home, roster)
            profile_result = capacity.run_profile(
                capacity.direct_peer_endpoint(port), home, 16, 1_000,
                capacity.INTERVALS, 64, roster=roster,
            )
            results.append(
                {
                    "campaign": campaign,
                    "daemon_port": port,
                    "doctor": capacity.benchmark_doctor_payload(doctor),
                    "profile": profile_result,
                    "daemon_output": output.evidence(),
                    "elapsed_seconds": time.perf_counter() - started,
                }
            )
        finally:
            if process is not None:
                capacity.reap_owned_daemon(process)
                output.join()

    payload = {
        "candidate_sha": capacity.source_revision(),
        "host": os.environ.get("COMPUTERNAME", "unknown"),
        "account": os.environ.get("USERNAME", "unknown"),
        "profile_root": str(profile),
        "runtime_home": str(home),
        "host_label": os.environ.get("ATM_CAPACITY_HOST_LABEL", "unknown"),
        "execution_mode": "current-account; scheduled-task isolation unavailable to unelevated token",
        "measurement": "candidate direct-peer TCP profile f16/64 using existing harness routines",
        "lifecycle_limitation": "wrapper snapshot preflight cannot reconcile ATM_HOME store with USERPROFILE store",
        "campaigns": results,
    }
    path = report_dir / "tcp-f16-64-current-account.json"
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
