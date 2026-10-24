#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys

from lint_common import discover_repo_root
from lint_common import workspace_crate_section_lines


def build_command(repo_root: Path) -> list[str]:
    return [
        sys.executable,
        "-c",
        "import sys; from codespell_lib import _script_main; sys.exit(_script_main())",
    ]


def emit_console_text(text: str, *, stream = None) -> None:
    if not text:
        return
    if stream is None:
        stream = sys.stdout
    encoding = getattr(stream, "encoding", None) or "utf-8"
    if hasattr(stream, "buffer"):
        stream.buffer.write(text.encode(encoding, errors="replace"))
        stream.flush()
        return
    stream.write(text.encode(encoding, errors="replace").decode(encoding))
    stream.flush()


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run codespell with the repo policy.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)

    for line in workspace_crate_section_lines(repo_root):
        print(line)

    try:
        completed = subprocess.run(
            build_command(repo_root),
            cwd=repo_root,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
    except FileNotFoundError as exc:
        print(
            f"error: spelling check executable is unavailable ({exc}); run `just bootstrap` and retry",
            file=sys.stderr,
        )
        return 127
    except OSError as exc:
        print(f"error: spelling check could not execute: {exc}", file=sys.stderr)
        return 2
    if completed.stdout:
        emit_console_text(completed.stdout)
    if completed.stderr:
        emit_console_text(completed.stderr, stream=sys.stderr)
    if completed.returncode and "No module named 'codespell_lib'" in completed.stderr:
        print(
            "error: codespell is unavailable in the Python environment; run `just bootstrap` and retry",
            file=sys.stderr,
        )
        return 2
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
