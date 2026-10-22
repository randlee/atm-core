"""Classify whether changed repository paths require the runtime CI matrix."""

from __future__ import annotations

import subprocess
import sys
from collections.abc import Iterable


RUNTIME_PATHS = (
    "crates/",
    ".github/workflows/",
    ".github/scripts/",
)
RUNTIME_FILES = {"Cargo.toml", "Cargo.lock", "build.rs", "Justfile"}


def requires_runtime(changed_paths: Iterable[str]) -> bool:
    """Return whether any path can change the Rust build or CI runtime."""
    return any(
        path in RUNTIME_FILES
        or path.startswith("rust-toolchain")
        or path.startswith(RUNTIME_PATHS)
        for path in changed_paths
    )


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("usage: ci_scope.py <range-start> <head>", file=sys.stderr)
        return 2
    changed = subprocess.run(
        ["git", "diff", "--name-only", "--no-renames", "-z", argv[1], argv[2]],
        check=True,
        capture_output=True,
    ).stdout
    paths = changed.decode("utf-8", errors="surrogateescape").split("\0")
    print(f"runtime={str(requires_runtime(paths)).lower()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
