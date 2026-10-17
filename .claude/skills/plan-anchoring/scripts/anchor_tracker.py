#!/usr/bin/env python3
"""Select the anchor tracker (bd or br) for this repository's planning gate.

Selection rules
  1. An explicit ``anchor_tracker`` in .sc/plan-anchor/settings.yaml wins, but
     is checked against disk: ``bd`` needs .beads/metadata.json with
     ``"backend": "dolt"``; ``br`` needs a .beads/ that is not Dolt-backed (or
     no .beads/ yet). A mismatch is an error, never a silent switch.
  2. With no setting: a Dolt-backed .beads/ selects ``bd``; any other .beads/
     selects ``br``; no .beads/ selects ``br``.
  3. The selected CLI must be on PATH.

This tool never runs or recommends bd's init command. bd is supported only
where a Dolt-backed .beads/ already exists; provisioning Dolt (DoltHub or a
Dolt server) is outside this skill. ``br init`` is the only bootstrap, and only
``write --init`` runs it.

Subcommands
  resolve   read-only: the tracker, its source (setting|detected), the
            settings path, and the evidence it rests on
  write     persist the selection to .sc/plan-anchor/settings.yaml (commit it);
            --tracker pins a choice, --init runs ``br init`` when there is no
            .beads/

Output is the JSON envelope {success, data, error}.

Exit codes (the plugin convention shared with jev_judge.py and plan_source.py)
  0  success: stdout holds {success: true, data}
  2  any error, including invalid arguments (VALIDATION.INPUT) and an internal
     error (SCRIPT.INTERNAL): stdout holds {success: false, error}; the error
     code says what to fix. Exit 1 is not used: in this plugin it means a
     completed run with a negative result, and selection has none.
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
    find_repo_root,
    read_settings,
    settings_path,
    write_settings,
)
from result import Err, Ok, Result, ToolError, err  # noqa: E402

Which = Callable[[str], Optional[str]]
Runner = Callable[..., "subprocess.CompletedProcess[str]"]

METADATA_RELPATH = ".beads/metadata.json"
INIT_TIMEOUT_SECONDS = 120
ERROR_EXIT = 2
DOLT_OUT_OF_SCOPE = (
    "bd is supported only where a Dolt-backed .beads/ already exists; "
    "provisioning Dolt (DoltHub or a Dolt server) is outside this skill."
)
INSTALL_HINT = {
    "br": "Install br (beads_rust) and put it on PATH; see references/installation-and-troubleshooting.md.",
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

    def to_json(self) -> dict:
        return {
            "tracker": self.tracker,
            "source": self.source,
            "repository_root": str(self.root),
            "settings_path": str(settings_path(self.root)),
            "settings_relpath": SETTINGS_RELPATH,
            "settings_present": self.settings is not None,
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
    return "bd" if beads.dolt else "br"


def check_against_disk(tracker: str, beads: BeadsState, origin: str) -> Result[None, ToolError]:
    if tracker == "bd" and not beads.dolt:
        found = "no .beads/" if not beads.exists else f".beads/ without a Dolt backend (backend {beads.backend!r})"
        return err("TRACKER.DOLT_REQUIRED",
                   f"{origin} selects bd, but the repository has {found}. {DOLT_OUT_OF_SCOPE}",
                   f"Use br (set 'anchor_tracker: br' in {SETTINGS_RELPATH}), or point the repository at an "
                   "existing Dolt-backed .beads/ provisioned outside this skill.",
                   data={"evidence": beads.to_json()})
    if tracker == "br" and beads.dolt:
        return err("TRACKER.MISMATCH",
                   f"{origin} selects br, but .beads/ is Dolt-backed (bd's workspace)",
                   f"Set 'anchor_tracker: bd' in {SETTINGS_RELPATH}, or remove the setting to detect bd.",
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


def _choose(settings: Optional[Settings], pinned: Optional[str], beads: BeadsState) -> Result[tuple, ToolError]:
    """(tracker, source) from the setting, an explicit pin, or detection."""
    configured = settings.anchor_tracker if settings is not None else None
    if configured and pinned and configured != pinned:
        return err("SETTINGS.CONFLICT",
                   f"{SETTINGS_RELPATH} already sets anchor_tracker: {configured}; refusing to change it to {pinned}",
                   f"Edit {SETTINGS_RELPATH} by hand if the tracker really changed.")
    if configured:
        checked = check_against_disk(configured, beads, f"{SETTINGS_RELPATH} anchor_tracker")
        return checked if isinstance(checked, Err) else Ok((configured, "setting"))
    if pinned:
        checked = check_against_disk(pinned, beads, "--tracker")
        return checked if isinstance(checked, Err) else Ok((pinned, "setting"))
    return Ok((detect(beads), "detected"))


def resolve(start: Path, *, which: Which = shutil.which, pinned: Optional[str] = None) -> Result[Selection, ToolError]:
    root = find_repo_root(start)
    if isinstance(root, Err):
        return root
    settings = read_settings(root.value)
    if isinstance(settings, Err):
        return settings
    beads = inspect_beads(root.value)
    if isinstance(beads, Err):
        return beads
    chosen = _choose(settings.value, pinned, beads.value)
    if isinstance(chosen, Err):
        return chosen
    tracker, source = chosen.value
    cli = locate_cli(tracker, which)
    if isinstance(cli, Err):
        return cli
    return Ok(Selection(tracker=tracker, source=source, root=root.value, settings=settings.value,
                        beads=beads.value, cli_path=cli.value))


def init_br(tracker: str, cli_path: str, root: Path, run: Runner) -> Result[BeadsState, ToolError]:
    """Run ``br init`` with a fixed argv (no shell) and confirm a non-Dolt .beads/ appeared."""
    if tracker != "br":
        return err("TRACKER.INIT_REFUSED", f"only br is ever initialized by this skill, not {tracker!r}. {DOLT_OUT_OF_SCOPE}")
    argv = [cli_path, "init"]
    try:
        proc = run(argv, cwd=str(root), capture_output=True, text=True, check=False,
                   stdin=subprocess.DEVNULL, timeout=INIT_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        return err("TRACKER.INIT_FAILED", f"br init did not finish within {INIT_TIMEOUT_SECONDS} s",
                   "Run 'br init' in the repository root by hand and inspect its output.")
    except OSError as exc:
        return err("TRACKER.INIT_FAILED", f"br init could not start: {type(exc).__name__}: {exc}",
                   INSTALL_HINT["br"])
    if proc.returncode != 0:
        stderr = (proc.stderr or "").strip()[-2000:]
        return err("TRACKER.INIT_FAILED", f"br init exited {proc.returncode}: {stderr or '(no stderr)'}",
                   "Fix the reported problem, then rerun 'anchor_tracker.py write --init'.",
                   data={"returncode": proc.returncode, "stderr": stderr})
    after = inspect_beads(root)
    if isinstance(after, Err):
        return after
    if not after.value.exists or after.value.dolt:
        return err("TRACKER.INIT_FAILED", "br init exited 0 but did not create a br .beads/ workspace",
                   "Run 'br init' in the repository root by hand and inspect its output.",
                   data={"evidence": after.value.to_json()})
    return after


def _git_ignored(root: Path, rel: str) -> Result[bool, str]:
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


def write(start: Path, *, pinned: Optional[str] = None, init: bool = False,
          which: Which = shutil.which, run: Runner = subprocess.run) -> Result[dict, ToolError]:
    selected = resolve(start, which=which, pinned=pinned)
    if isinstance(selected, Err):
        return selected
    sel = selected.value
    initialized = False
    beads = sel.beads
    if init and not beads.exists:
        after = init_br(sel.tracker, sel.cli_path, sel.root, run)
        if isinstance(after, Err):
            return after
        beads, initialized = after.value, True
    written = False
    path = settings_path(sel.root)
    if sel.settings is None or sel.settings.anchor_tracker is None:
        saved = write_settings(sel.root, Settings(schema_version=SCHEMA_VERSION, anchor_tracker=sel.tracker))
        if isinstance(saved, Err):
            return saved
        written = True
    commit = [SETTINGS_RELPATH] if written else []
    if initialized:
        commit.append(".beads/")
    checked = _git_ignored(sel.root, SETTINGS_RELPATH)
    ignored = checked.value if isinstance(checked, Ok) else None
    actions = []
    if written:
        actions.append(f"Commit {SETTINGS_RELPATH} so every clone selects {sel.tracker}.")
    if initialized:
        actions.append("Run 'br sync --flush-only', then commit .beads/ (beads.db stays gitignored by design).")
    elif not beads.exists:
        actions.append("No .beads/ yet: rerun with --init to run 'br init' (the only bootstrap this skill performs).")
    if ignored:
        actions.append(f"{SETTINGS_RELPATH} is gitignored; un-ignore it so the selection is committed.")
    elif isinstance(checked, Err):
        actions.append(f"Could not check whether {SETTINGS_RELPATH} is gitignored ({checked.error}); "
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
    wr.add_argument("--init", action="store_true", help="run 'br init' when br is selected and there is no .beads/")
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
            outcome = write(start, pinned=args.tracker, init=args.init, which=which, run=run)
        envelope, code = _envelope(outcome)
    except Exception as exc:  # a bug, not an expected failure
        envelope = {"success": False, "data": None, "error": ToolError(
            "SCRIPT.INTERNAL", f"{type(exc).__name__}: {exc}",
            "Report this with the command line that produced it.").to_json()}
        code = ERROR_EXIT
    print(json.dumps(envelope, indent=2, sort_keys=True, ensure_ascii=False))
    return code


if __name__ == "__main__":
    sys.exit(main())
