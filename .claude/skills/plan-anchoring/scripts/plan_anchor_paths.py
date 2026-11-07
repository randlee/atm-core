#!/usr/bin/env python3
"""Where a plan-anchor install lives, and which locations are global.

One rule, shared by the installer, the gates and the settings lookup: walk up
from the working directory, nearest first, and stop before the home directory.

  - ``ancestors(start)``: ``start`` and its parents, nearest first. When
    ``start`` is under the home directory the walk stops before it (the home
    directory itself is never visited); otherwise it ends at the filesystem
    root. ``start`` equal to the home directory visits nothing. When the home
    directory cannot be determined (Windows with no ``USERPROFILE`` or
    ``HOMEDRIVE``/``HOMEPATH``), the walk ends at the filesystem root.
  - ``find_installs(start)``: the nearest directory holding
    ``<dir>/<harness dir>/skills/plan-anchoring/scripts/jev_judge.py`` and every
    copy in it (one per harness directory the installer writes: .claude,
    .codex, .cursor; never .agents, which the installer does not write).
  - ``trusted_script(script, start)``: the script must be one of those copies
    and the install root must hold the installer's
    ``.sc/plan-anchor/install-lock.json`` naming it; a hand-copied or lock-less
    copy is refused.
  - ``find_settings(start)`` / ``find_lock(start)``: the nearest
    ``.sc/plan-anchor/settings.yaml`` / ``install-lock.json``.
  - ``global_location(path)``: why ``path`` is a global (user-wide or
    machine-wide) harness location, or None. The home directory itself, and
    anything inside ``~/.claude``, ``~/.codex``, ``~/.cursor``, ``~/.agents``,
    ``/etc/codex`` (Codex admin skills) and the Claude Code managed-settings
    directories, is global; that covers every plugin cache
    under them. A plan-anchor install is per folder, never global. With no
    determinable home directory only the machine-wide directories are known
    to be global, and they are still refused.

No lookup raises because the home directory is unknown.

Standard library only; importing it has no side effects.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Iterator, List, Optional, Tuple

SKILL_NAME = "plan-anchoring"
HARNESS_DIRS = (".claude", ".codex", ".cursor")  # the directories install.py writes (--target)
# User-wide harness directories: global, so never an install location. .agents (Codex's documented
# user skill directory) is global here but is not a harness directory the installer writes.
GLOBAL_HARNESS_DIRS = HARNESS_DIRS + (".agents",)
SETTINGS_RELPATH = ".sc/plan-anchor/settings.yaml"
LOCK_RELPATH = ".sc/plan-anchor/install-lock.json"
SCRIPT_RELPATH = f"skills/{SKILL_NAME}/scripts/jev_judge.py"
# Machine-wide skill/config directories (Codex admin skills; Claude Code managed settings).
SYSTEM_GLOBALS = (
    "/etc/codex",
    "/etc/claude-code",
    "/Library/Application Support/ClaudeCode",
    "C:/ProgramData/ClaudeCode",
)


def _expand(path: Path) -> Path:
    """``path`` with a leading ``~`` expanded; unchanged when the home directory cannot be determined."""
    try:
        return Path(path).expanduser()
    except (KeyError, RuntimeError):
        return Path(path)


def _resolve(path: Path) -> Path:
    expanded = _expand(path)
    try:
        return expanded.resolve()
    except (OSError, RuntimeError):
        return Path(os.path.abspath(str(expanded)))


def home_dir() -> Optional[Path]:
    """The user's home directory (``$HOME`` / ``%USERPROFILE%``), resolved, or None when it cannot be determined.

    ``os.path.expanduser`` returns ``~`` unchanged when no home is known (on
    Windows: no ``USERPROFILE`` and no ``HOMEDRIVE``/``HOMEPATH``; on POSIX: no
    ``HOME`` and no password-database entry), and ``Path.home()`` raises.
    """
    try:
        raw = os.path.expanduser("~")
    except (KeyError, RuntimeError, OSError):
        return None
    if not raw or raw.startswith("~"):
        return None
    return _resolve(Path(raw))


def _home(home: Optional[Path]) -> Optional[Path]:
    return _resolve(home) if home is not None else home_dir()


def _within(path: Path, root: Path) -> bool:
    return path == root or root in path.parents


def global_roots(home: Optional[Path] = None) -> List[Path]:
    """The global directories: the harness directories under home (when it is known) and the machine-wide ones."""
    home = _home(home)
    roots = [home / name for name in GLOBAL_HARNESS_DIRS] if home is not None else []
    roots += [_resolve(Path(p)) for p in SYSTEM_GLOBALS]
    return roots


def global_location(path: Path, home: Optional[Path] = None) -> Optional[str]:
    """Why ``path`` is a global location (a short reason), or None when it is a per-folder location."""
    home = _home(home)
    target = _resolve(path)
    if home is not None and target == home:
        return f"{home} is the home directory"
    for root in global_roots(home):
        if _within(target, root):
            return f"{target} is inside the global location {root}"
    return None


def ancestors(start: Path, home: Optional[Path] = None) -> Iterator[Path]:
    """``start`` and its parents, nearest first, stopping before the home directory (if it is known)."""
    home = _home(home)
    current = _resolve(start)
    if current.is_file():
        current = current.parent
    if home is not None and current == home:
        return
    under_home = home is not None and home in current.parents
    while True:
        yield current
        parent = current.parent
        if parent == current or (under_home and parent == home):
            return
        current = parent


def find_installs(start: Path, home: Optional[Path] = None) -> Optional[Tuple[Path, List[Path]]]:
    """(install root, [jev_judge.py per harness dir]) for the nearest install above ``start``, or None."""
    for directory in ancestors(start, home):
        found = [directory / harness / SCRIPT_RELPATH for harness in HARNESS_DIRS]
        found = [p for p in found if p.is_file()]
        if found:
            return directory, found
    return None


def _nearest(start: Path, rel: str, home: Optional[Path]) -> Optional[Path]:
    for directory in ancestors(start, home):
        candidate = directory.joinpath(*rel.split("/"))
        if candidate.exists() or candidate.is_symlink():
            return candidate
    return None


def find_settings(start: Path, home: Optional[Path] = None) -> Optional[Path]:
    """The nearest ``.sc/plan-anchor/settings.yaml`` at or above ``start`` (stopping before home)."""
    return _nearest(start, SETTINGS_RELPATH, home)


def find_lock(start: Path, home: Optional[Path] = None) -> Optional[Path]:
    """The nearest ``.sc/plan-anchor/install-lock.json`` at or above ``start`` (stopping before home)."""
    return _nearest(start, LOCK_RELPATH, home)


def trusted_script(script: Path, start: Path, home: Optional[Path] = None) -> Tuple[bool, str]:
    """Whether ``script`` is the jev_judge.py the trust rule selects from ``start``; (ok, reason)."""
    resolved = _resolve(script)
    why = global_location(resolved, home)
    if why is not None:
        return False, f"the script is in a global location ({why}); plan-anchor is installed per folder"
    nearest = find_installs(start, home)
    if nearest is None:
        return False, (f"no {', '.join(HARNESS_DIRS)} install of {SKILL_NAME} at or above {_resolve(start)} "
                       "(the walk stops before the home directory)")
    root, copies = nearest
    if resolved not in {_resolve(p) for p in copies}:
        return False, f"the nearest install is {root}; {resolved} is not one of its copies"
    named, why = lock_names(root, resolved)
    if not named:
        return False, why
    return True, f"nearest install {root}"


def lock_names(root: Path, script: Path) -> Tuple[bool, str]:
    """Whether ``root``'s install lock (written by install.py) names ``script``; (ok, reason).

    Only an install the installer wrote is trusted: a copy without the lock, or one the lock does not list,
    was placed by hand.
    """
    lock = root.joinpath(*LOCK_RELPATH.split("/"))
    try:
        data = json.loads(lock.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return False, (f"{root} has no {LOCK_RELPATH}: the copy was not written by the plan-anchor installer "
                       "(install.py --dest <folder>)")
    except (OSError, UnicodeError, ValueError) as exc:
        return False, f"{lock} could not be read as JSON ({type(exc).__name__})"
    files = data.get("files") if isinstance(data, dict) else None
    if not isinstance(files, dict):
        return False, f"{lock} is not a plan-anchor install record"
    try:
        key = _resolve(script).relative_to(_resolve(root)).as_posix()
    except ValueError:
        return False, f"{script} is not inside the install root {root}"
    if key not in files:
        return False, f"{lock} does not list {key}: the copy was not written by the plan-anchor installer"
    return True, f"{lock} lists {key}"
