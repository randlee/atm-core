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
            "agent-team-mail-core",
            "send::hook::tests::graft_post_send_emitter_delegates_to_graft_port",
            "--",
            "--exact",
        ],
        [
            "cargo",
            "test",
            "-p",
            "agent-team-mail-core",
            "send::hook::tests::graft_post_send_emitter_surfaces_port_failure",
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
