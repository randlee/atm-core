#!/usr/bin/env python3
from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Callable

from lint_common import load_lint_config


def fail(message: str) -> None:
    raise SystemExit(message)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def version_sync_config(repo_root: Path) -> dict:
    config = load_lint_config(repo_root).get("version_sync", {})
    if not isinstance(config, dict):
        raise SystemExit("[version_sync] must be a TOML table")
    return config


def winget_settings(config: dict) -> tuple[str, str, str, str] | None:
    winget = config.get("winget", {})
    if not isinstance(winget, dict) or not winget.get("enabled", False):
        return None
    manifest_glob = winget.get("manifest_glob")
    fields = (
        winget.get("package_version_field", "PackageVersion"),
        winget.get("manifest_version_field", "ManifestVersion"),
        winget.get("installer_url_field", "InstallerUrl"),
    )
    if not isinstance(manifest_glob, str) or not manifest_glob.strip():
        raise SystemExit("[version_sync.winget].manifest_glob must be a non-empty string when enabled")
    if not all(isinstance(field, str) and field for field in fields):
        raise SystemExit("[version_sync.winget] field names must be non-empty strings")
    return (manifest_glob, *fields)


def winget_manifest_paths(repo_root: Path, manifest_glob: str) -> list[Path]:
    paths = sorted(repo_root.glob(manifest_glob))
    if not paths:
        fail(f"no Winget manifests found for glob {manifest_glob!r}")
    return paths


def winget_field_pattern(field_name: str) -> re.Pattern[str]:
    return re.compile(rf"^([ \t]*{re.escape(field_name)}:[ \t]*)(\S+)([ \t]*)$", re.MULTILINE)


def replace_winget_field_values(
    text: str, field_name: str, transform: Callable[[str], str], *, expected_count: int | None = None
) -> tuple[str, int]:
    pattern = winget_field_pattern(field_name)
    matches = list(pattern.finditer(text))
    if not matches:
        return text, 0
    if expected_count is not None and len(matches) != expected_count:
        return text, len(matches)
    updated = pattern.sub(
        lambda match: f"{match.group(1)}{transform(match.group(2))}{match.group(3)}",
        text,
    )
    return updated, len(matches)


def replace_version_occurrences(value: str, old_version: str, new_version: str) -> str:
    pattern = re.compile(rf"(?<![0-9.]){re.escape(old_version)}(?![0-9.])")
    return pattern.sub(new_version, value)


def validate_workspace_version(repo_root: Path) -> str:
    cargo_toml = tomllib.loads(read_text(repo_root / "Cargo.toml"))
    workspace_version = cargo_toml.get("workspace", {}).get("package", {}).get("version")
    if not workspace_version:
        fail("workspace version missing from Cargo.toml")
    return workspace_version


SIMPLE_SPECIFIER_CLAUSE = re.compile(r"(>=|<=|==|!=|>|<)\s*(\d+(?:\.\d+)*)")


def version_tuple(value: str) -> tuple[int, ...]:
    return tuple(int(part) for part in value.split("."))


def specifier_admits(version: str, specifier: str) -> bool:
    """Evaluate a simple comma-separated PEP 440-style specifier (>=, <=, ==, !=, >, <).

    Deliberately minimal (no pre-releases, no ~=, no packaging dependency) --
    this repo's own pyproject.toml dependency pins only ever use this shape.
    """

    candidate = version_tuple(version)
    clauses = SIMPLE_SPECIFIER_CLAUSE.findall(specifier)
    if not clauses:
        fail(f"unsupported version specifier shape: {specifier!r}")
    for operator, bound in clauses:
        bound_tuple = version_tuple(bound)
        # Compare on the shorter length so "1.5" bounds admit "1.5.3" candidates.
        length = min(len(candidate), len(bound_tuple))
        lhs, rhs = candidate[:length], bound_tuple[:length]
        if operator == ">=" and not lhs >= rhs:
            return False
        if operator == "<=" and not lhs <= rhs:
            return False
        if operator == "==" and not lhs == rhs:
            return False
        if operator == "!=" and not lhs != rhs:
            return False
        if operator == ">" and not lhs > rhs:
            return False
        if operator == "<" and not lhs < rhs:
            return False
    return True


PYTHON_DEPENDENCY_ENTRY = re.compile(r"^([A-Za-z0-9_.-]+)\s*(.*)$")


def workspace_python_distribution_names(repo_root: Path) -> set[str]:
    """Names of the Python packages built from the Cargo workspace, as declared under
    ``[[python_distributions]]`` in ``release/publish-artifacts.toml``. Their static
    ``[project].version`` is rewritten by the publish kit on every bump, so they
    track the workspace version exactly.
    """

    manifest_path = repo_root / "release" / "publish-artifacts.toml"
    if not manifest_path.is_file():
        return set()
    distributions = tomllib.loads(read_text(manifest_path)).get("python_distributions", [])
    if not isinstance(distributions, list):
        return set()
    return {
        entry["name"]
        for entry in distributions
        if isinstance(entry, dict) and isinstance(entry.get("name"), str)
    }


def workspace_tracked_python_packages(repo_root: Path, workspace_version: str) -> dict[str, str]:
    """Map Python package name -> version for every workspace pyproject.toml whose
    version tracks the Cargo workspace version: the workspace-built distributions
    (e.g. the atm-graft wheel, static ``[project].version`` kept in lockstep by the
    publish kit) and any package still declaring ``dynamic = ["version"]``. These are
    the packages other in-workspace pyproject.toml dependency pins must track when
    the workspace version bumps. A tracked static version that has drifted from the
    workspace version fails outright.
    """

    tracked_names = workspace_python_distribution_names(repo_root)
    packages: dict[str, str] = {}
    for pyproject in sorted((repo_root / "crates").glob("*/pyproject.toml")):
        manifest = tomllib.loads(read_text(pyproject))
        project = manifest.get("project", {})
        if not isinstance(project, dict):
            continue
        name = project.get("name")
        if not isinstance(name, str):
            continue
        dynamic = project.get("dynamic", [])
        is_dynamic = isinstance(dynamic, list) and "version" in dynamic
        if not is_dynamic and name not in tracked_names:
            continue
        if not is_dynamic and project.get("version") != workspace_version:
            fail(
                f"{pyproject.relative_to(repo_root).as_posix()} [project].version "
                f"{project.get('version')!r} does not match the workspace version "
                f'"{workspace_version}" -- run the publish kit sync-python-version step'
            )
        packages[name] = workspace_version
    return packages


def validate_python_dependency_pins(repo_root: Path, workspace_version: str) -> bool:
    """Keep the ATM-specific Python pin gap until randlee/sc-lint#137 ships.

    Fail if any in-workspace pyproject.toml pins a workspace-tracked Python
    package (e.g. hermes-atm's ``atm-graft`` dependency) with a specifier the
    current workspace version no longer satisfies. Prevents the class of bug
    where a crate's own ``version`` field is bumped but a sibling's dependency
    range on it is left stale (e.g. ``atm-graft>=1.4,<1.5`` after atm-graft
    itself moved to 1.5.x).
    """

    tracked_packages = workspace_tracked_python_packages(repo_root, workspace_version)
    if not tracked_packages:
        return False

    checked_any = False
    for pyproject in sorted((repo_root / "crates").glob("*/pyproject.toml")):
        manifest = tomllib.loads(read_text(pyproject))
        project = manifest.get("project", {})
        if not isinstance(project, dict):
            continue
        dependencies = project.get("dependencies", [])
        if not isinstance(dependencies, list):
            continue
        rel_manifest = pyproject.relative_to(repo_root).as_posix()
        for entry in dependencies:
            if not isinstance(entry, str):
                continue
            match = PYTHON_DEPENDENCY_ENTRY.match(entry.strip())
            if match is None:
                continue
            dependency_name, specifier = match.group(1), match.group(2).strip()
            target_version = tracked_packages.get(dependency_name)
            if target_version is None or not specifier:
                continue
            checked_any = True
            if not specifier_admits(target_version, specifier):
                fail(
                    f"{rel_manifest} dependency {entry!r}: specifier does not admit "
                    f'the current workspace-tracked "{dependency_name}" version '
                    f'"{target_version}" -- update the pin (e.g. to track the workspace minor)'
                )
    return checked_any


def sync_winget_manifests(
    repo_root: Path, old_version: str, workspace_version: str, config: dict
) -> list[Path]:
    """Update the configured Winget version fields for a release."""

    settings = winget_settings(config)
    if settings is None:
        return []
    manifest_glob, package_version_field, manifest_version_field, installer_url_field = settings
    manifest_paths = winget_manifest_paths(repo_root, manifest_glob)

    changed: list[Path] = []
    for manifest_path in manifest_paths:
        text = read_text(manifest_path)
        original = text
        for field_name in (package_version_field, manifest_version_field):
            text, count = replace_winget_field_values(
                text,
                field_name,
                lambda _old_value: workspace_version,
                expected_count=1,
            )
            if count != 1:
                fail(f"{manifest_path.relative_to(repo_root)} must contain exactly one {field_name} field")

        text, count = replace_winget_field_values(
            text,
            installer_url_field,
            lambda value: replace_version_occurrences(value, old_version, workspace_version),
        )
        if count == 0:
            fail(f"{manifest_path.relative_to(repo_root)} is missing {installer_url_field} version")

        if text != original:
            manifest_path.write_text(text, encoding="utf-8")
            changed.append(manifest_path)
    return changed


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    venv_python = repo_root / ".sc-lint/venv" / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    if not venv_python.is_file():
        bootstrap = repo_root / ".sc-lint" / "bootstrap.py"
        setup_command = [sys.executable, str(bootstrap), "setup", "--config", "sc-lint.toml"]
        setup = subprocess.run(setup_command, cwd=repo_root, check=False)
        if setup.returncode != 0:
            return setup.returncode
    published = subprocess.run(
        [str(venv_python), "-m", "sc_lint.check_version_sync"],
        cwd=repo_root,
        check=False,
    )
    if published.returncode != 0:
        return published.returncode

    # delete when randlee/sc-lint#137 ships: the published version check does
    # not yet verify workspace-tracked Python dependency specifiers.
    workspace_version = validate_workspace_version(repo_root)
    validate_python_dependency_pins(repo_root, workspace_version)
    return 0


if __name__ == "__main__":
    sys.exit(main())
