"""POSIX-shell discovery used only by cross-platform script tests."""
from __future__ import annotations

import ntpath
import os
from pathlib import Path
import shutil
import subprocess


def resolve_posix_shell(
    *,
    git_bash: str | None = None,
    windows: bool | None = None,
    windir: str | None = None,
) -> str | None:
    """Find a usable shell, rejecting Windows' System32 WSL launcher."""
    if not (os.name == "nt" if windows is None else windows):
        return shutil.which("bash") or shutil.which("sh")
    candidates = _windows_shell_candidates(git_bash)
    try:
        git_result = subprocess.run(
            ["git", "--exec-path"], text=True, capture_output=True, check=False, timeout=5
        )
    except OSError:
        git_exec_path = ""
    else:
        git_exec_path = git_result.stdout.strip()
    candidates.extend(_git_bash_candidates(git_exec_path))
    candidates.extend(value for value in (shutil.which("bash"), shutil.which("sh")) if value)
    system32 = ntpath.join(windir or os.environ.get("WINDIR", r"C:\Windows"), "System32")
    for candidate in candidates:
        if not _is_system32_wsl_stub(candidate, system32) and Path(candidate).is_file():
            return candidate
    return None


def _windows_shell_candidates(git_bash: str | None) -> list[str]:
    if not git_bash:
        return []
    return [ntpath.join(git_bash, "bash.exe") if ntpath.isdir(git_bash) else git_bash]


def _git_bash_candidates(git_exec_path: str) -> list[str]:
    candidates: list[str] = []
    current = git_exec_path
    while current:
        candidates.extend((ntpath.join(current, "bin", "bash.exe"), ntpath.join(current, "usr", "bin", "bash.exe")))
        parent = ntpath.dirname(current)
        if parent == current:
            break
        current = parent
    return candidates


def _is_system32_wsl_stub(candidate: str, system32: str) -> bool:
    resolved_candidate = str(Path(candidate).resolve(strict=False)).replace("\\", "/").casefold().rstrip("/")
    resolved_system32 = str(Path(system32).resolve(strict=False)).replace("\\", "/").casefold().rstrip("/")
    return resolved_candidate == resolved_system32 or resolved_candidate.startswith(f"{resolved_system32}/")
