"""Repository settings for sc-plan-anchor: ``.sc/plan-anchor/settings.yaml``.

The file follows the Synaptic Canvas convention: one subfolder per package
under ``.sc/`` at the repository root, YAML with snake_case keys, committed to
git, never secrets. Validation is strict: ``schema_version`` must be 1 and
unknown or duplicate keys are rejected, so a typo never silently falls back to
a default.

This is the only plan-anchoring module that needs PyYAML
(``python3 -m pip install pyyaml``). PyYAML is imported lazily, so importing
this module never fails; a missing PyYAML is returned as
``DEPENDENCY.PYYAML``. Every expected failure is returned as
``Err(ToolError)`` (see ``result.py``), never raised.
"""

from __future__ import annotations

from dataclasses import dataclass
import importlib
import os
from pathlib import Path
import subprocess
import tempfile
from types import ModuleType
from typing import Any, Optional

from result import Err, Ok, Result, ToolError, err

SCHEMA_VERSION = 1
SETTINGS_RELPATH = ".sc/plan-anchor/settings.yaml"
TRACKERS = ("bd", "br")
ALLOWED_KEYS = ("schema_version", "anchor_tracker")
PYYAML_INSTALL = "python3 -m pip install pyyaml"

SETTINGS_TEMPLATE = """\
# .sc/plan-anchor/settings.yaml
# Repository settings for the sc-plan-anchor plugin; commit this file, never put secrets in it.
schema_version: {schema_version}
anchor_tracker: {anchor_tracker}
"""


@dataclass(frozen=True)
class Settings:
    schema_version: int
    anchor_tracker: Optional[str]


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
    fix = f"Edit {path}: it must hold 'schema_version: {SCHEMA_VERSION}' and optionally 'anchor_tracker: br' or 'anchor_tracker: bd'."
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
                   f"{path} anchor_tracker is {tracker!r}; expected 'br' or 'bd'", fix)
    return Ok(Settings(schema_version=version, anchor_tracker=tracker))


def read_settings(root: Path) -> Result[Optional[Settings], ToolError]:
    """``Ok(None)`` when the file is absent; the parsed settings, or a typed error."""
    loaded = load_yaml()
    if isinstance(loaded, Err):
        return loaded
    path = settings_path(root)
    if not path.exists() and not path.is_symlink():
        return Ok(None)
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as exc:
        return err("SETTINGS.UNREADABLE", f"{SETTINGS_RELPATH} could not be read: {type(exc).__name__}",
                   f"Make {SETTINGS_RELPATH} a readable UTF-8 file.")
    return parse_settings(text, loaded.value)


def render_settings(settings: Settings) -> str:
    """The committed form: header comment, purpose line, one key per line."""
    if settings.anchor_tracker not in TRACKERS:
        raise ValueError(f"render_settings needs a tracker, got {settings.anchor_tracker!r}")  # caller bug
    return SETTINGS_TEMPLATE.format(schema_version=settings.schema_version,
                                    anchor_tracker=settings.anchor_tracker)


def write_settings(root: Path, settings: Settings) -> Result[Path, ToolError]:
    """Write atomically, then read the file back through the strict parser."""
    loaded = load_yaml()
    if isinstance(loaded, Err):
        return loaded
    path = settings_path(root)
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
