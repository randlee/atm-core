#!/usr/bin/env python3
"""Delete when randlee/sc-lint#783 ships workspace Python pin validation."""
from __future__ import annotations

import re
import tomllib
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(message)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


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
    pieces = [part.strip() for part in specifier.split(",")]
    if not pieces or any(SIMPLE_SPECIFIER_CLAUSE.fullmatch(part) is None for part in pieces):
        fail(f"unsupported version specifier shape: {specifier!r}")
    clauses = [SIMPLE_SPECIFIER_CLAUSE.fullmatch(part).groups() for part in pieces]
    if not clauses:
        fail(f"unsupported version specifier shape: {specifier!r}")
    for operator, bound in clauses:
        bound_tuple = version_tuple(bound)
        # PEP 440 compares missing release components as zero.
        length = max(len(candidate), len(bound_tuple))
        lhs = candidate + (0,) * (length - len(candidate))
        rhs = bound_tuple + (0,) * (length - len(bound_tuple))
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
    """Keep the ATM-specific Python pin gap until randlee/sc-lint#783 ships.

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
            if target_version is None:
                continue
            if not specifier:
                fail(f"{rel_manifest} dependency {entry!r}: workspace dependency must be pinned")
            checked_any = True
            if not specifier_admits(target_version, specifier):
                fail(
                    f"{rel_manifest} dependency {entry!r}: specifier does not admit "
                    f'the current workspace-tracked "{dependency_name}" version '
                    f'"{target_version}" -- update the pin (e.g. to track the workspace minor)'
                )
    return checked_any



if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent
    validate_python_dependency_pins(root, validate_workspace_version(root))
