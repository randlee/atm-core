#!/usr/bin/env python3
"""Select the anchor tracker (bd) for this repository's planning gate.

Selection rules
  1. An explicit ``anchor_tracker`` in the nearest .sc/plan-anchor/settings.yaml
     (the repository's own, else one in a parent folder) wins, but
     is checked against disk: ``bd`` needs .beads/metadata.json with
     ``"backend": "dolt"``. A mismatch is an error, never a silent switch.
  2. With no setting: this install selects ``bd``, checked against disk the
     same way. This install is rendered for bd: a .beads/ that is not
     Dolt-backed (or no .beads/) is TRACKER.DOLT_REQUIRED, fixed by rerunning
     plan-anchor-installer with the other tracker (--tracker), never by a
     silent switch.
  3. The selected CLI must be on PATH.

This tool never runs or recommends bd's init command. bd is supported only
where a Dolt-backed .beads/ already exists; provisioning Dolt (DoltHub or a
Dolt server) is outside this skill.

Subcommands
  resolve   read-only: the tracker, its source (setting|detected), the
            settings path, and the evidence it rests on
  write     persist the selection to .sc/plan-anchor/settings.yaml (commit it);
            --tracker pins a choice

Output is the JSON envelope {success, data, error}.

Exit codes (the plugin convention shared with jev_judge.py)
  0  success: stdout holds {success: true, data}
  2  any error, including invalid arguments (VALIDATION.INPUT) and an internal
     error (SCRIPT.INTERNAL): stdout holds {success: false, error}; the error
     code says what to fix. There is no exit 1 anywhere in this plugin: a
     command that ran reports a negative answer in data with exit 0, and
     selection has none.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from typing import Any, Callable, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

from plan_anchor_settings import (  # noqa: E402
    SCHEMA_VERSION,
    SETTINGS_RELPATH,
    TRACKERS,
    Settings,
    commit_path,
    effective_settings,
    find_repo_root,
    git_ignored,
    inherited,
    settings_path,
    write_settings_file,
)
from result import Err, Ok, Result, ToolError, err, run_main, utf8_stdio  # noqa: E402

Which = Callable[[str], Optional[str]]
Runner = Callable[..., "subprocess.CompletedProcess[str]"]

METADATA_RELPATH = ".beads/metadata.json"
INIT_TIMEOUT_SECONDS = 120
ERROR_EXIT = 2
DOLT_OUT_OF_SCOPE = (
    "bd is supported only where a Dolt-backed .beads/ already exists; "
    "provisioning Dolt (DoltHub or a Dolt server) is outside this skill."
)
OTHER_TRACKER = "rerun plan-anchor-installer with the other tracker (--tracker) to switch this install"
INSTALL_HINT = {
    "bd": "Install bd and put it on PATH; see references/installation-and-troubleshooting.md.",
}


@dataclass(frozen=True)
class BeadsState:
    exists: bool
    metadata_present: bool
    backend: Optional[str]

    @property
    def dolt(self) -> bool:
        return self.backend == "dolt"

    def to_json(self) -> dict:
        return {
            "beads_dir": self.exists,
            "metadata_path": METADATA_RELPATH if self.metadata_present else None,
            "backend": self.backend,
            "dolt_backed": self.dolt,
        }


@dataclass(frozen=True)
class Selection:
    tracker: str
    source: str  # "setting" | "detected"
    root: Path
    settings: Optional[Settings]
    beads: BeadsState
    cli_path: str
    settings_file: Optional[Path] = None  # the nearest settings file that applies (None: no file)

    def to_json(self) -> dict:
        own = settings_path(self.root)
        return {
            "tracker": self.tracker,
            "source": self.source,
            "repository_root": str(self.root),
            "settings_path": str(self.settings_file or own),
            "settings_relpath": SETTINGS_RELPATH,
            "settings_present": self.settings is not None,
            "settings_inherited": inherited(self.settings_file, self.root),
            "cli_path": self.cli_path,
            "beads_initialized": self.beads.exists,
            "evidence": self.beads.to_json(),
        }


def inspect_beads(root: Path) -> Result[BeadsState, ToolError]:
    beads = root / ".beads"
    if not beads.exists():
        return Ok(BeadsState(exists=False, metadata_present=False, backend=None))
    if not beads.is_dir():
        return err("BEADS.NOT_DIRECTORY", ".beads exists but is not a directory",
                   "Move the .beads file aside; it must be the tracker workspace directory.")
    metadata = beads / "metadata.json"
    if not metadata.exists():
        return Ok(BeadsState(exists=True, metadata_present=False, backend=None))
    fix = f"Repair {METADATA_RELPATH} (a JSON object; bd's Dolt workspaces carry \"backend\": \"dolt\")."
    try:
        data: Any = json.loads(metadata.read_text(encoding="utf-8"))
    except (OSError, UnicodeError) as exc:
        return err("BEADS.METADATA_INVALID", f"{METADATA_RELPATH} could not be read: {type(exc).__name__}", fix)
    except json.JSONDecodeError as exc:
        return err("BEADS.METADATA_INVALID", f"{METADATA_RELPATH} is not valid JSON: {exc.msg}", fix)
    if not isinstance(data, dict):
        return err("BEADS.METADATA_INVALID", f"{METADATA_RELPATH} must be a JSON object", fix)
    backend = data.get("backend")
    if backend is not None and not isinstance(backend, str):
        return err("BEADS.METADATA_INVALID", f"{METADATA_RELPATH} backend must be a string, got {backend!r}", fix)
    return Ok(BeadsState(exists=True, metadata_present=True, backend=backend))


def detect(beads: BeadsState) -> str:
    return "bd"  # the tracker this install is rendered for; check_against_disk confirms .beads/ fits it


def check_against_disk(tracker: str, beads: BeadsState, origin: str) -> Result[None, ToolError]:
    if tracker == "bd" and not beads.dolt:
        found = "no .beads/" if not beads.exists else f".beads/ without a Dolt backend (backend {beads.backend!r})"
        return err("TRACKER.DOLT_REQUIRED",
                   f"{origin} selects bd, but the repository has {found}. {DOLT_OUT_OF_SCOPE}",
                   "This install is rendered for bd: point the repository at an existing Dolt-backed .beads/ "
                   f"provisioned outside this skill, or {OTHER_TRACKER}.",
                   data={"evidence": beads.to_json()})
    return Ok(None)


def locate_cli(tracker: str, which: Which) -> Result[str, ToolError]:
    path = which(tracker)
    if not path:
        return err("TRACKER.CLI_MISSING", f"the selected anchor tracker {tracker!r} is not on PATH",
                   INSTALL_HINT[tracker], data={"tracker": tracker})
    # which() can answer relative to the working directory (a relative PATH
    # entry, or Windows searching the current directory first). Pin it to an
    # absolute path so a later run with another cwd starts the same file.
    return Ok(os.path.abspath(path))


def _choose(settings: Optional[Settings], pinned: Optional[str], beads: BeadsState,
            shown: str = SETTINGS_RELPATH) -> Result[tuple, ToolError]:
    """(tracker, source) from the setting, an explicit pin, or detection; ``shown`` names the settings file in effect."""
    configured = settings.anchor_tracker if settings is not None else None
    if configured and pinned and configured != pinned:
        return err("SETTINGS.CONFLICT",
                   f"{shown} already sets anchor_tracker: {configured}; refusing to change it to {pinned}",
                   f"Edit {shown} by hand if the tracker really changed.")
    if configured:
        checked = check_against_disk(configured, beads, f"{shown} anchor_tracker")
        return checked if isinstance(checked, Err) else Ok((configured, "setting"))
    if pinned:
        if pinned not in TRACKERS:
            return err("VALIDATION.INPUT", f"--tracker {pinned!r}: this install is rendered for bd only",
                       f"Pin bd, or {OTHER_TRACKER}.")
        checked = check_against_disk(pinned, beads, "--tracker")
        return checked if isinstance(checked, Err) else Ok((pinned, "setting"))
    checked = check_against_disk(detect(beads), beads, "this install")
    return checked if isinstance(checked, Err) else Ok((detect(beads), "detected"))


def resolve(start: Path, *, which: Which = shutil.which, pinned: Optional[str] = None) -> Result[Selection, ToolError]:
    root = find_repo_root(start)
    if isinstance(root, Err):
        return root
    # The nearest settings file at or above ``start`` wins (REQ-070), so the walk starts there, not at the
    # git top level: an install in a folder inside the repository writes its settings in that folder.
    found = effective_settings(Path(start).resolve())
    if isinstance(found, Err):
        return found
    settings = Ok(found.value.settings)
    beads = inspect_beads(root.value)
    if isinstance(beads, Err):
        return beads
    path = found.value.path
    shown = SETTINGS_RELPATH if path is None or path == settings_path(root.value) else commit_path(path, root.value)
    chosen = _choose(settings.value, pinned, beads.value, shown)
    if isinstance(chosen, Err):
        return chosen
    tracker, source = chosen.value
    cli = locate_cli(tracker, which)
    if isinstance(cli, Err):
        return cli
    return Ok(Selection(tracker=tracker, source=source, root=root.value, settings=settings.value,
                        beads=beads.value, cli_path=cli.value, settings_file=found.value.path))


_git_ignored = git_ignored  # shared with plan_anchor_settings.py set


def write(start: Path, *, pinned: Optional[str] = None, init: bool = False,
          which: Which = shutil.which, run: Runner = subprocess.run) -> Result[dict, ToolError]:
    selected = resolve(start, which=which, pinned=pinned)
    if isinstance(selected, Err):
        return selected
    sel = selected.value
    initialized = False
    beads = sel.beads
    written = False
    # The file in effect when it is the repository's own or a subfolder install's; else a new one at the root.
    own = sel.settings_file is not None and not inherited(sel.settings_file, sel.root)
    path = sel.settings_file if own else settings_path(sel.root)
    shown = commit_path(path, sel.root)
    if sel.settings is None or sel.settings.anchor_tracker is None:
        kept = sel.settings if sel.settings is not None else Settings(schema_version=SCHEMA_VERSION, anchor_tracker=None)
        saved = write_settings_file(path, Settings(schema_version=SCHEMA_VERSION, anchor_tracker=sel.tracker,
                                                   allow_remote_judge=kept.allow_remote_judge, plan_dir=kept.plan_dir))
        if isinstance(saved, Err):
            return saved
        written = True
    commit = [shown] if written else []
    if initialized:
        commit.append(".beads/")
    checked = _git_ignored(sel.root, shown)
    ignored = checked.value if isinstance(checked, Ok) else None
    actions = []
    if written:
        actions.append(f"Commit {shown} so every clone selects {sel.tracker}.")
    if ignored:
        actions.append(f"{shown} is gitignored; un-ignore it so the selection is committed.")
    elif isinstance(checked, Err):
        actions.append(f"Could not check whether {shown} is gitignored ({checked.error}); "
                       "confirm with 'git check-ignore -v' before committing.")
    data = sel.to_json()
    data.update({
        "written": written,
        "settings_present": True,
        "beads_initialized": beads.exists,
        "initialized_now": initialized,
        "evidence": beads.to_json(),
        "settings_ignored_by_git": ignored,
        "settings_ignored_check_error": checked.error if isinstance(checked, Err) else None,
        "commit_paths": commit,
        "next_actions": actions,
    })
    return Ok(data)


def _envelope(result: Result[Any, ToolError]) -> tuple:
    if isinstance(result, Ok):
        value = result.value
        data = value.to_json() if isinstance(value, Selection) else value
        return {"success": True, "data": data, "error": None}, 0
    error: ToolError = result.error
    data = dict(error.data) if error.data is not None else None
    return {"success": False, "data": data, "error": error.to_json()}, ERROR_EXIT


class _UsageError(Exception):
    pass


class _JsonArgumentParser(argparse.ArgumentParser):
    """Usage errors become a VALIDATION.INPUT envelope instead of argparse's text on stderr."""

    def error(self, message):  # argparse hook; never returns
        raise _UsageError(f"{self.prog}: {message}")


def _parser() -> argparse.ArgumentParser:
    parser = _JsonArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True, parser_class=_JsonArgumentParser)
    res = sub.add_parser("resolve", help="read-only: report the selected anchor tracker")
    res.add_argument("--repository-root", default=".", help="any path inside the repository (default: .)")
    wr = sub.add_parser("write", help=f"persist the selection to {SETTINGS_RELPATH}")
    wr.add_argument("--repository-root", default=".", help="any path inside the repository (default: .)")
    wr.add_argument("--tracker", choices=TRACKERS, help="pin this tracker instead of detecting (checked against disk)")
    return parser


def main(argv: Optional[list] = None, *, which: Which = shutil.which, run: Runner = subprocess.run) -> int:
    try:
        args = _parser().parse_args(argv)
    except _UsageError as exc:
        envelope = {"success": False, "data": None, "error": ToolError(
            "VALIDATION.INPUT", str(exc), "Run 'anchor_tracker.py --help' for the accepted arguments.").to_json()}
        print(json.dumps(envelope, indent=2, sort_keys=True, ensure_ascii=False))
        return ERROR_EXIT
    try:
        start = Path(args.repository_root)
        if args.command == "resolve":
            outcome: Result[Any, ToolError] = resolve(start, which=which)
        else:
            outcome = write(start, pinned=args.tracker, which=which, run=run)
        envelope, code = _envelope(outcome)
    except Exception as exc:  # a bug, not an expected failure
        envelope = {"success": False, "data": None, "error": ToolError(
            "SCRIPT.INTERNAL", f"{type(exc).__name__}: {exc}",
            "Report this with the command line that produced it.").to_json()}
        code = ERROR_EXIT
    print(json.dumps(envelope, indent=2, sort_keys=True, ensure_ascii=False))
    return code


if __name__ == "__main__":
    sys.exit(run_main(main))
