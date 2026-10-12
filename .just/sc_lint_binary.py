"""Resolve and invoke the sc-lint binary managed by the adoption kit."""
from __future__ import annotations

import os
from pathlib import Path
from typing import NoReturn


# Delete when randlee/sc-lint#388 adds a bootstrap pass-through for built-in analyzers.
def resolve_sc_lint_binary() -> Path:
    configured_binary = os.environ.get("SC_LINT_BIN")
    if configured_binary:
        binary = Path(configured_binary)
    else:
        data_home = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
        install_dir = Path(os.environ.get("SC_LINT_INSTALL_DIR", data_home / "sc-lint" / "bin"))
        binary = install_dir / "sc-lint"

    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise FileNotFoundError(f"sc-lint binary is unavailable at {binary}; run just setup")
    return binary


def lint_argv(repo_root: Path, analyzer: str, args: list[str]) -> tuple[Path, list[str]]:
    binary = resolve_sc_lint_binary()
    return binary, [str(binary), "lint", analyzer, "--root", str(repo_root), *args]


def exec_lint(repo_root: Path, analyzer: str, args: list[str]) -> NoReturn:
    binary, argv = lint_argv(repo_root, analyzer, args)
    os.execv(str(binary), argv)
