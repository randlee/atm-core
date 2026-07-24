#!/usr/bin/env python3
"""Measure a remote ATM peer's HTTP reachability and one canonical send.

This is a smoke harness, not a second transport.  The read probe uses curl
against the daemon's normal HTTP route; the write probe uses the public `atm`
CLI, which enters the local daemon and follows the canonical post-write route.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
from dataclasses import asdict, dataclass


@dataclass
class Sample:
    operation: str
    sample: int
    elapsed_ms: int
    exit_code: int
    stdout: str
    stderr: str


def run(command: list[str], operation: str, sample: int) -> Sample:
    started = time.perf_counter()
    completed = subprocess.run(command, text=True, capture_output=True, check=False)
    elapsed_ms = round((time.perf_counter() - started) * 1000)
    return Sample(
        operation=operation,
        sample=sample,
        elapsed_ms=elapsed_ms,
        exit_code=completed.returncode,
        stdout=completed.stdout.strip(),
        stderr=completed.stderr.strip(),
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True, help="peer IP address or hostname")
    parser.add_argument("--port", type=int, default=43101)
    parser.add_argument("--peer", required=True, help="ATM peer address for the send probe")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--out", type=pathlib.Path, help="write JSON evidence here")
    args = parser.parse_args()

    if args.samples < 1:
        parser.error("--samples must be positive")
    curl = shutil.which("curl") or shutil.which("curl.exe")
    atm = shutil.which("atm") or shutil.which("atm.exe")
    if curl is None or atm is None:
        missing = ", ".join(name for name, value in (("curl", curl), ("atm", atm)) if value is None)
        parser.error(f"required command missing: {missing}")

    endpoint = f"http://{args.host}:{args.port}/v1/atm/doctor"
    samples: list[Sample] = []
    for sample in range(1, args.samples + 1):
        samples.append(
            run(
                [
                    curl,
                    "--silent",
                    "--show-error",
                    "--connect-timeout",
                    "5",
                    "--max-time",
                    "10",
                    "--request",
                    "GET",
                    "--header",
                    "Content-Type: application/json",
                    "--data",
                    "{}",
                    endpoint,
                ],
                "remote_doctor",
                sample,
            )
        )
        samples.append(
            run(
                [
                    atm,
                    "send",
                    args.peer,
                    f"cross-host-latency-sample-{sample}",
                    "--json",
                ],
                "canonical_send",
                sample,
            )
        )

    result = {
        "host": args.host,
        "port": args.port,
        "peer": args.peer,
        "security_mode": os.environ.get("ATM_PEER_TRANSPORT_SECURITY", "mutual-tls"),
        "samples": [asdict(sample) for sample in samples],
    }
    text = json.dumps(result, indent=2)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text + "\n", encoding="utf-8")
    print(text)
    return 0 if all(sample.exit_code == 0 for sample in samples) else 1


if __name__ == "__main__":
    raise SystemExit(main())
