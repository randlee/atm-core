#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import subprocess
import sys


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def run(command: list[str]) -> None:
    subprocess.run(command, cwd=repo_root(), check=True)


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "default"
    if mode == "default":
        run([sys.executable, "scripts/verify_user_docs.py", "--source-root", "docs/user-documents"])
        run([sys.executable, ".just/run_pytests.py"])
        run(["cargo", "build", "--workspace"])
        # `atm-daemon` is reference-only while Phase AL constructs the
        # replacement Tokio runtime. Its historical unit tests are not
        # replacement acceptance evidence and must not pull new work back into
        # the legacy implementation.
        run(["cargo", "test", "--workspace", "--exclude", "atm-daemon"])
        # The CLI surface inspector is intentionally feature-gated so it is not
        # shipped as a user-facing command. Run its integration contract
        # explicitly: plain workspace testing would otherwise skip it.
        run(
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "--features",
                "cli-surface-dump",
                "--test",
                "cli_surface",
            ]
        )
        return 0
    if mode == "coverage":
        run([sys.executable, "scripts/coverage/run.py", "--write-artifacts"])
        return 0
    raise SystemExit(f"unknown test mode: {mode}")


if __name__ == "__main__":
    raise SystemExit(main())
