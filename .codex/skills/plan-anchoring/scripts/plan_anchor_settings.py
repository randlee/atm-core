"""Settings for sc-plan-anchor: ``.sc/plan-anchor/settings.yaml``.

The file follows the Synaptic Canvas convention: one subfolder per package
under ``.sc/``, YAML with snake_case keys, committed to git, never secrets.

Lookup: the nearest settings file at or above the working directory (or
``--repository-root``) wins (``plan_anchor_paths.find_settings``: walk up, stop
before the home directory). The walk starts at that path, not at the git top
level, so an install in a folder inside a repository reads the settings it
wrote there.
A parent folder that holds many repositories can carry one file for all of
them, and a repository overrides it with its own. The nearest file wins as a
whole: keys are never merged across files, so an override that omits
``allow_remote_judge`` means nobody has answered for that repository. Validation is strict: ``schema_version`` must be 1 and
unknown or duplicate keys are rejected, so a typo never silently falls back to
a default.

This is the only plan-anchoring module that needs PyYAML
(``python3 -m pip install pyyaml``). PyYAML is imported lazily, so importing
this module never fails; a missing PyYAML is returned as
``DEPENDENCY.PYYAML``. Every expected failure is returned as
``Err(ToolError)`` (see ``result.py``), never raised.

Keys (all optional except ``schema_version``)
  anchor_tracker      ``bd``; written by ``anchor_tracker.py write``
  allow_remote_judge  ``true`` or ``false``: the user's answer to whether plan
                      text may be sent to TypeSafe (Jev). Absent means nobody
                      has asked yet, and live Jev calls are refused. Only the
                      user decides it; record the answer with ``set``.
  plan_dir            the current phase's plan folder, relative to the folder
                      holding ``.sc/``, by convention ``docs/plans/phase-<id>``
                      (for example ``docs/plans/phase-b``). ``anchors.json``,
                      ``intent.md`` and ``anchor-runs/`` live there. Absent
                      means nobody has named the current phase yet; ask the
                      user and record it with ``set``. Set it again when the
                      phase changes.

Command line (the JSON envelope ``{success, data, error}``; exit 0, or 2 with a
typed error, including ``VALIDATION.INPUT`` for bad arguments)
  show [--repository-root PATH]
      read-only: the parsed settings and where they live
  set allow_remote_judge true|false [--repository-root PATH]
      record the user's answer, keeping every other key; commit the file
  set plan_dir docs/plans/phase-<id> [--repository-root PATH]
      record the current phase folder, keeping every other key; commit the file
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass, replace
import importlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sys
from types import ModuleType
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

from plan_anchor_paths import find_settings  # noqa: E402
from result import Err, Ok, Result, ToolError, err, run_main, utf8_stdio  # noqa: E402

SCHEMA_VERSION = 1
SETTINGS_RELPATH = ".sc/plan-anchor/settings.yaml"
TRACKERS = ("bd",)
ALLOWED_KEYS = ("schema_version", "anchor_tracker", "allow_remote_judge", "plan_dir")
SETTABLE_KEYS = ("allow_remote_judge", "plan_dir")
PLAN_DIR_CONVENTION = "docs/plans/phase-<current>"
PYYAML_INSTALL = "python3 -m pip install pyyaml"
ERROR_EXIT = 2

SETTINGS_HEADER = """\
# .sc/plan-anchor/settings.yaml
# Repository settings for the sc-plan-anchor plugin; commit this file, never put secrets in it.
schema_version: {schema_version}
"""
TRACKER_LINE = "anchor_tracker: {anchor_tracker}\n"
REMOTE_JUDGE_LINES = """\
# allow_remote_judge: may plan text be sent to TypeSafe (Jev) for judging? Only the user decides it.
allow_remote_judge: {value}
"""
PLAN_DIR_LINES = """\
# plan_dir: the current phase's plan folder (anchors.json, intent.md, anchor-runs/). Set it again when the phase changes.
plan_dir: {value}
"""


@dataclass(frozen=True)
class Settings:
    schema_version: int
    anchor_tracker: Optional[str]
    allow_remote_judge: Optional[bool] = None
    plan_dir: Optional[str] = None


def plan_dir_problem(value: Any) -> Optional[str]:
    """Why ``value`` is not a usable ``plan_dir``, or None.

    A relative POSIX path with no empty, ``.`` or ``..`` part and no ``<placeholder>``
    left in, so the current phase id is always written out (``docs/plans/phase-b``).
    """
    if not isinstance(value, str):
        return f"must be a string such as 'docs/plans/phase-b', got {type(value).__name__}"
    if not value or value != value.strip():
        return "must be a non-empty path with no surrounding spaces"
    if "\\" in value or value.startswith("/") or (len(value) > 1 and value[1] == ":"):
        return "must be a relative path with forward slashes"
    if "<" in value or ">" in value:
        return "still holds a placeholder such as <current>; write the phase id, e.g. docs/plans/phase-b"
    if any(part in ("", ".", "..") for part in value.split("/")):
        return "must not hold empty, '.' or '..' parts or end in '/'"
    return None


def load_yaml() -> Result[ModuleType, ToolError]:
    try:
        return Ok(importlib.import_module("yaml"))
    except ImportError:
        return err(
            "DEPENDENCY.PYYAML",
            "PyYAML is required to read .sc/plan-anchor/settings.yaml and is not installed",
            f"Install it: {PYYAML_INSTALL}",
        )


def _git_toplevel(start: Path) -> Optional[Path]:
    try:
        proc = subprocess.run(
            ["git", "-C", str(start), "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, encoding="utf-8", check=False,
            stdin=subprocess.DEVNULL,
        )
    except OSError:
        return None
    out = proc.stdout.strip()
    if proc.returncode != 0 or not out:
        return None
    return Path(out).resolve()


def find_repo_root(start: Path) -> Result[Path, ToolError]:
    """The git top level containing ``start``; walk up for ``.git`` when git cannot answer. Never cwd."""
    try:
        start = start.resolve(strict=True)
    except OSError:
        return err("REPO.NOT_FOUND", f"repository root hint {str(start)!r} does not exist",
                   "Pass --repository-root inside the git repository.")
    if start.is_file():
        start = start.parent
    top = _git_toplevel(start)
    if top is not None:
        return Ok(top)
    for candidate in (start, *start.parents):
        if (candidate / ".git").exists():
            return Ok(candidate)
    return err("REPO.NOT_FOUND", f"{str(start)!r} is not inside a git repository",
               "Run from, or pass --repository-root inside, the repository that holds the plan.")


def settings_path(root: Path) -> Path:
    return root.joinpath(*SETTINGS_RELPATH.split("/"))


def _strict_loader(yaml: ModuleType) -> type:
    class StrictLoader(yaml.SafeLoader):  # type: ignore[name-defined, misc]
        def construct_mapping(self, node, deep=False):  # noqa: D401 - PyYAML hook
            seen = set()
            for key_node, _ in node.value:
                key = self.construct_object(key_node, deep=deep)
                try:
                    duplicate = key in seen
                except TypeError:
                    break  # unhashable key: PyYAML reports it below
                if duplicate:
                    raise yaml.constructor.ConstructorError(
                        None, None, f"duplicate key {key!r}", key_node.start_mark)
                seen.add(key)
            return super().construct_mapping(node, deep=deep)

    return StrictLoader


def parse_settings(text: str, yaml: ModuleType, *, path: str = SETTINGS_RELPATH) -> Result[Settings, ToolError]:
    fix = (f"Edit {path}: it must hold 'schema_version: {SCHEMA_VERSION}' and optionally "
           "'anchor_tracker: bd', 'allow_remote_judge: true' or 'allow_remote_judge: false', "
           f"and 'plan_dir: {PLAN_DIR_CONVENTION}' with the phase id.")
    try:
        data: Any = yaml.load(text, Loader=_strict_loader(yaml))  # a SafeLoader subclass
    except yaml.YAMLError as exc:
        return err("SETTINGS.MALFORMED", f"{path} is not valid YAML: {exc}", fix)
    if not isinstance(data, dict):
        return err("SETTINGS.MALFORMED", f"{path} must be a YAML mapping, got {type(data).__name__}", fix)
    unknown = sorted(str(k) for k in data if k not in ALLOWED_KEYS)
    if unknown:
        return err("SETTINGS.UNKNOWN_KEY", f"{path} has unknown key(s): {', '.join(unknown)}",
                   f"Remove them; supported keys are {', '.join(ALLOWED_KEYS)}.",
                   data={"unknown_keys": unknown})
    version = data.get("schema_version")
    if type(version) is not int or version != SCHEMA_VERSION:
        return err("SETTINGS.SCHEMA_VERSION",
                   f"{path} schema_version is {version!r}; this plugin reads schema_version {SCHEMA_VERSION}",
                   fix, data={"schema_version": version if isinstance(version, (int, str)) else None})
    tracker = data.get("anchor_tracker")
    if "anchor_tracker" in data and tracker not in TRACKERS:
        return err("SETTINGS.INVALID_TRACKER",
                   f"{path} anchor_tracker is {tracker!r}; expected 'bd' (this install is rendered for bd)",
                   f"Set 'anchor_tracker: bd' in {path}, or rerun plan-anchor-installer with the other tracker "
                   "(--tracker) to switch this install.")
    remote = data.get("allow_remote_judge")
    if "allow_remote_judge" in data and type(remote) is not bool:
        return err("SETTINGS.INVALID_REMOTE_JUDGE",
                   f"{path} allow_remote_judge is {remote!r}; expected true or false", fix)
    plan_dir = data.get("plan_dir")
    problem = plan_dir_problem(plan_dir) if "plan_dir" in data else None
    if problem:
        return err("SETTINGS.INVALID_PLAN_DIR", f"{path} plan_dir is {plan_dir!r}: it {problem}", fix)
    return Ok(Settings(schema_version=version, anchor_tracker=tracker, allow_remote_judge=remote, plan_dir=plan_dir))


def read_settings_file(path: Path, shown: Optional[str] = None) -> Result[Optional[Settings], ToolError]:
    """``Ok(None)`` when ``path`` is absent; the parsed settings, or a typed error.

    ``shown`` is how messages name the file (default: the path itself).
    """
    loaded = load_yaml()
    if isinstance(loaded, Err):
        return loaded
    shown = shown or str(path)
    if not path.exists() and not path.is_symlink():
        return Ok(None)
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as exc:
        return err("SETTINGS.UNREADABLE", f"{shown} could not be read: {type(exc).__name__}",
                   f"Make {shown} a readable UTF-8 file.")
    return parse_settings(text, loaded.value, path=shown)


def read_settings(root: Path) -> Result[Optional[Settings], ToolError]:
    """The settings file exactly at ``root`` (no lookup): ``Ok(None)`` when it is absent."""
    return read_settings_file(settings_path(root), SETTINGS_RELPATH)


@dataclass(frozen=True)
class Effective:
    """The settings that apply at a location: the nearest file (``path`` None when there is none)."""
    settings: Optional[Settings]
    path: Optional[Path]


def effective_settings(start: Path) -> Result[Effective, ToolError]:
    """The nearest settings file at or above ``start`` (stopping before the home directory), parsed."""
    loaded = load_yaml()  # required even when no file exists: no silent fallback to defaults
    if isinstance(loaded, Err):
        return loaded
    path = find_settings(start)
    if path is None:
        return Ok(Effective(None, None))
    own = path == settings_path(Path(start).resolve()) or path == settings_path(Path(start))
    loaded = read_settings_file(path, SETTINGS_RELPATH if own else str(path))
    if isinstance(loaded, Err):
        return loaded
    return Ok(Effective(loaded.value, path))


def render_settings(settings: Settings) -> str:
    """The committed form: header comment, purpose line, one key per line (absent keys are omitted)."""
    if settings.anchor_tracker is not None and settings.anchor_tracker not in TRACKERS:
        raise ValueError(f"render_settings got tracker {settings.anchor_tracker!r}")  # caller bug
    if settings.allow_remote_judge is not None and type(settings.allow_remote_judge) is not bool:
        raise ValueError(f"render_settings got allow_remote_judge {settings.allow_remote_judge!r}")  # caller bug
    if settings.plan_dir is not None and plan_dir_problem(settings.plan_dir):
        raise ValueError(f"render_settings got plan_dir {settings.plan_dir!r}")  # caller bug
    text = SETTINGS_HEADER.format(schema_version=settings.schema_version)
    if settings.anchor_tracker is not None:
        text += TRACKER_LINE.format(anchor_tracker=settings.anchor_tracker)
    if settings.allow_remote_judge is not None:
        text += REMOTE_JUDGE_LINES.format(value="true" if settings.allow_remote_judge else "false")
    if settings.plan_dir is not None:
        text += PLAN_DIR_LINES.format(value=json.dumps(settings.plan_dir, ensure_ascii=False))  # quoted: always a string
    return text


def write_settings(root: Path, settings: Settings) -> Result[Path, ToolError]:
    """Write ``root``'s own settings file (see ``write_settings_file``)."""
    return write_settings_file(settings_path(root), settings)


def write_settings_file(path: Path, settings: Settings) -> Result[Path, ToolError]:
    """Write atomically, then read the file back through the strict parser."""
    loaded = load_yaml()
    if isinstance(loaded, Err):
        return loaded
    text = render_settings(settings)
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        fd, tmp = tempfile.mkstemp(prefix=".settings.", suffix=".tmp", dir=str(path.parent))
        try:
            with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as handle:
                handle.write(text)
            os.replace(tmp, path)
        except BaseException:
            if os.path.exists(tmp):
                os.unlink(tmp)
            raise
    except OSError as exc:
        return err("SETTINGS.WRITE_FAILED", f"could not write {SETTINGS_RELPATH}: {type(exc).__name__}: {exc}",
                   f"Check permissions on {path.parent}.")
    reread = parse_settings(text, loaded.value)
    if isinstance(reread, Err) or reread.value != settings:
        raise AssertionError("rendered settings do not round-trip")  # bug, not an expected failure
    return Ok(path)


def git_ignored(root: Path, rel: str) -> Result[bool, str]:
    """Whether git ignores ``rel``; ``Err(reason)`` when git cannot say (reported, not fatal)."""
    try:
        proc = subprocess.run(["git", "-C", str(root), "check-ignore", "-q", "--", rel],
                              capture_output=True, text=True, check=False, stdin=subprocess.DEVNULL)
    except OSError as exc:
        return Err(f"git could not start: {type(exc).__name__}: {exc}")
    if proc.returncode in (0, 1):
        return Ok(proc.returncode == 0)
    stderr = (proc.stderr or "").strip()[-500:]
    return Err(f"git check-ignore exited {proc.returncode}: {stderr or '(no stderr)'}")


# ---------------------------------------------------------------- allow_remote_judge

REMOTE_JUDGE_SET_COMMAND = "plan_anchor_settings.py set allow_remote_judge true|false"
PLAN_DIR_SET_COMMAND = f"plan_anchor_settings.py set plan_dir {PLAN_DIR_CONVENTION}"


def remote_judge_consent(root: Path) -> Result[bool, ToolError]:
    """The recorded answer: ``Ok(True)``, ``Ok(False)``, or ``SETTINGS.REMOTE_JUDGE_UNSET`` when nobody has asked.

    Read from the nearest settings file at or above ``root``. No file needs no
    PyYAML; a present one is parsed strictly (``DEPENDENCY.PYYAML`` when PyYAML
    is missing: never a silent default).
    """
    unset = err("SETTINGS.REMOTE_JUDGE_UNSET", f"{SETTINGS_RELPATH} does not set allow_remote_judge")
    if find_settings(root) is None:
        return unset
    loaded = effective_settings(root)
    if not loaded.ok:
        return loaded
    s = loaded.value.settings
    value = s.allow_remote_judge if s is not None else None
    return unset if value is None else Ok(value)


def set_value(start: Path, key: str, value: Any) -> Result[dict, ToolError]:
    """Record ``key`` in the settings file, keeping every other key; read it back strictly."""
    if key not in SETTABLE_KEYS:
        return err("VALIDATION.INPUT", f"{key!r} cannot be set here; settable keys: {', '.join(SETTABLE_KEYS)}",
                   "Set anchor_tracker with 'anchor_tracker.py write'.")
    if key == "plan_dir" and plan_dir_problem(value):
        return err("VALIDATION.INPUT", f"plan_dir {value!r} {plan_dir_problem(value)}",
                   f"Run '{PLAN_DIR_SET_COMMAND}' with the current phase id, e.g. docs/plans/phase-b.")
    root = find_repo_root(start)
    if not root.ok:
        return root
    effective = effective_settings(start.resolve())
    if not effective.ok:
        return effective  # a malformed file is never overwritten
    # The answer goes into the file that applies here: the nearest one at or above ``start``,
    # else a new one at the repository root.
    path = effective.value.path or settings_path(root.value)
    current = Ok(effective.value.settings)
    before = current.value if current.value is not None else Settings(schema_version=SCHEMA_VERSION, anchor_tracker=None)
    previous = getattr(before, key)
    after = replace(before, **{key: value})
    written = current.value is None or after != current.value
    if written:
        saved = write_settings_file(path, after)
        if not saved.ok:
            return saved
    shown = commit_path(path, root.value)
    checked = git_ignored(path.parent.parent.parent, SETTINGS_RELPATH)  # relative to the folder holding the file
    ignored = checked.value if checked.ok else None
    actions = [f"Commit {shown} so every clone and every agent uses this answer."] if written else []
    if ignored:
        actions.append(f"{shown} is gitignored; un-ignore it so the answer is committed.")
    elif not checked.ok:
        actions.append(f"Could not check whether {shown} is gitignored ({checked.error}); "
                       "confirm with 'git check-ignore -v' before committing.")
    return Ok({
        "key": key,
        "value": value,
        "previous": previous,
        "written": written,
        "repository_root": str(root.value),
        "settings_path": str(path),
        "settings_relpath": SETTINGS_RELPATH,
        "settings_inherited": inherited(path, root.value),
        "settings_ignored_by_git": ignored,
        "settings_ignored_check_error": None if checked.ok else checked.error,
        "commit_paths": [shown] if written else [],
        "next_actions": actions,
    })


def inherited(path: Optional[Path], root: Path) -> bool:
    """Whether the settings file in effect lives above the repository (a parent-folder install).

    The repository's own file and one in a folder inside it (an install there) are not inherited.
    """
    if path is None:
        return False
    holder = Path(path).resolve().parent.parent.parent  # <dir>/.sc/plan-anchor/settings.yaml
    return holder in Path(root).resolve().parents


def commit_path(path: Path, root: Path) -> str:
    """How to name ``path`` for a commit: relative to the repository root when inside it, else absolute."""
    resolved, top = Path(path).resolve(), Path(root).resolve()
    try:
        return resolved.relative_to(top).as_posix()
    except ValueError:
        return str(resolved)


def show(start: Path) -> Result[dict, ToolError]:
    root = find_repo_root(start)
    if not root.ok:
        return root
    current = effective_settings(start.resolve())
    if not current.ok:
        return current
    s, path = current.value.settings, current.value.path
    return Ok({
        "repository_root": str(root.value),
        "settings_path": str(path) if path is not None else str(settings_path(root.value)),
        "settings_relpath": SETTINGS_RELPATH,
        "settings_present": s is not None,
        "settings_inherited": inherited(path, root.value),
        "schema_version": s.schema_version if s is not None else None,
        "anchor_tracker": s.anchor_tracker if s is not None else None,
        "allow_remote_judge": s.allow_remote_judge if s is not None else None,
        "plan_dir": s.plan_dir if s is not None else None,
    })


# ---------------------------------------------------------------- CLI


class _UsageError(Exception):
    pass


class _JsonArgumentParser(argparse.ArgumentParser):
    """Usage errors become a VALIDATION.INPUT envelope instead of argparse's text on stderr."""

    def error(self, message):  # argparse hook; never returns
        raise _UsageError(f"{self.prog}: {message}")


def _value(key: str, text: str) -> Any:
    """The typed value of ``set <key> <text>``; ``_UsageError`` when it is not one."""
    if key == "allow_remote_judge":
        if text not in ("true", "false"):
            raise _UsageError(f"plan_anchor_settings.py set: argument value: expected true or false, got {text!r}")
        return text == "true"
    return text  # plan_dir: checked by set_value


def _parser() -> argparse.ArgumentParser:
    parser = _JsonArgumentParser(prog="plan_anchor_settings.py", description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True, parser_class=_JsonArgumentParser)
    sh = sub.add_parser("show", help="read-only: the parsed settings")
    sh.add_argument("--repository-root", default=".", help="any path inside the repository (default: .)")
    st = sub.add_parser("set", help=f"record a value in {SETTINGS_RELPATH} (commit it)")
    st.add_argument("key", choices=SETTABLE_KEYS)
    st.add_argument("value", metavar="VALUE", help="allow_remote_judge: true|false; plan_dir: docs/plans/phase-<id>")
    st.add_argument("--repository-root", default=".", help="any path inside the repository (default: .)")
    return parser


def _print(envelope: dict) -> None:
    print(json.dumps(envelope, indent=2, sort_keys=True, ensure_ascii=False))


def main(argv: Optional[list] = None) -> int:
    try:
        args = _parser().parse_args(argv)
        if args.command == "set":
            args.value = _value(args.key, args.value)
    except _UsageError as exc:
        _print({"success": False, "data": None, "error": ToolError(
            "VALIDATION.INPUT", str(exc), "Run 'plan_anchor_settings.py --help' for the accepted arguments.").to_json()})
        return ERROR_EXIT
    try:
        start = Path(args.repository_root)
        outcome = show(start) if args.command == "show" else set_value(start, args.key, args.value)
        if outcome.ok:
            envelope, code = {"success": True, "data": outcome.value, "error": None}, 0
        else:
            e = outcome.error
            envelope = {"success": False, "data": dict(e.data) if e.data is not None else None, "error": e.to_json()}
            code = ERROR_EXIT
    except Exception as exc:  # a bug, not an expected failure
        envelope = {"success": False, "data": None, "error": ToolError(
            "SCRIPT.INTERNAL", f"{type(exc).__name__}: {exc}",
            "Report this with the command line that produced it.").to_json()}
        code = ERROR_EXIT
    _print(envelope)
    return code


if __name__ == "__main__":
    sys.exit(run_main(main))
