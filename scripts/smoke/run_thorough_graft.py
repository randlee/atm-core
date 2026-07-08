#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys


def graft_commands() -> list[list[str]]:
    return [
        [
            "cargo",
            "test",
            "-p",
            "atm-daemon",
            "tests_post_send_graft_warning::dispatcher_send_delivers_direct_graft_nudge_without_warning",
            "--",
            "--exact",
        ],
        [
            "cargo",
            "test",
            "-p",
            "atm-daemon",
            "tests_post_send_graft_warning::dispatcher_send_surfaces_typed_warning_when_graft_receiver_path_is_unavailable",
            "--",
            "--exact",
        ],
        [
            "cargo",
            "test",
            "-p",
            "atm-daemon",
            "tests_post_send_graft_warning::dispatcher_ack_surfaces_typed_warning_when_graft_reply_target_is_unavailable",
            "--",
            "--exact",
        ],
    ]


def main() -> int:
    results: list[dict[str, object]] = []
    for command in graft_commands():
        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        results.append(
            {
                "command": command,
                "exit_code": completed.returncode,
                "stdout": completed.stdout.strip(),
                "stderr": completed.stderr.strip(),
            }
        )
        if completed.returncode != 0:
            print(json.dumps({"status": "failed", "results": results}, indent=2))
            return 1
    print(json.dumps({"status": "passed", "results": results}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
