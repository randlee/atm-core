#!/usr/bin/env python3
"""Independent guard for boundary-policy relaxations and daemon->sqlite drift."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
from pathlib import Path

FORBIDDEN_EDGE = "atm-daemon -> atm-rusqlite"
RUNTIME_COMPOSITION = Path("boundaries/atm-runtime/runtime-composition.toml")
SQLITE_BOUNDARY_FILES = [
    Path("boundaries/atm-rusqlite/sqlite-boundary-assembly.toml"),
    Path("boundaries/atm-rusqlite/mail-store-sqlite.toml"),
    Path("boundaries/atm-rusqlite/mail-store-doctor-sqlite.toml"),
    Path("boundaries/atm-rusqlite/roster-store-sqlite.toml"),
    Path("boundaries/atm-rusqlite/roster-store-doctor-sqlite.toml"),
    Path("boundaries/atm-rusqlite/task-store-sqlite.toml"),
    Path("boundaries/atm-rusqlite/task-store-doctor-sqlite.toml"),
    Path("boundaries/atm-rusqlite/shared-db.toml"),
]
ALL_GUARDED_BOUNDARY_FILES = [RUNTIME_COMPOSITION, *SQLITE_BOUNDARY_FILES]
VISIBILITY_ORDER = {
    "private": 0,
    "pub(crate)": 1,
    "crate": 1,
    "public": 2,
}


def _read_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text())


def _find_line(path: Path, needle: str) -> str:
    try:
        for number, line in enumerate(path.read_text().splitlines(), start=1):
            if needle in line:
                return f"{path.as_posix()}:{number}"
    except FileNotFoundError:
        pass
    return path.as_posix()


def _as_list(doc: dict, *keys: str) -> list[str]:
    current = doc
    for key in keys:
        if not isinstance(current, dict):
            return []
        current = current.get(key, [])
    return list(current) if isinstance(current, list) else []


def _as_string(doc: dict, *keys: str) -> str | None:
    current = doc
    for key in keys:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current if isinstance(current, str) else None


def _removed(base_values: list[str], current_values: list[str]) -> list[str]:
    return sorted(set(base_values) - set(current_values))


def _added(base_values: list[str], current_values: list[str]) -> list[str]:
    return sorted(set(current_values) - set(base_values))


def compare_boundary_policy(path: Path, base_doc: dict, current_doc: dict) -> list[dict]:
    relaxations: list[dict] = []
    if path not in ALL_GUARDED_BOUNDARY_FILES:
        return relaxations

    allowed_additions = _added(
        _as_list(base_doc, "dependencies", "allowed_dependents"),
        _as_list(current_doc, "dependencies", "allowed_dependents"),
    )
    if allowed_additions:
        relaxations.append(
            {
                "file": path.as_posix(),
                "field": "allowed_dependents",
                "change": f"added {', '.join(allowed_additions)}",
                "requires_approval": True,
            }
        )

    forbidden_removals = _removed(
        _as_list(base_doc, "dependencies", "forbidden_edges"),
        _as_list(current_doc, "dependencies", "forbidden_edges"),
    )
    if forbidden_removals:
        relaxations.append(
            {
                "file": path.as_posix(),
                "field": "forbidden_edges",
                "change": f"removed {', '.join(forbidden_removals)}",
                "requires_approval": True,
            }
        )

    if path in SQLITE_BOUNDARY_FILES:
        for field in ("visibility", "constructor"):
            base_value = _as_string(base_doc, "implementation", field)
            current_value = _as_string(current_doc, "implementation", field)
            if (
                base_value in VISIBILITY_ORDER
                and current_value in VISIBILITY_ORDER
                and VISIBILITY_ORDER[current_value] > VISIBILITY_ORDER[base_value]
            ):
                relaxations.append(
                    {
                        "file": path.as_posix(),
                        "field": field,
                        "change": f"{base_value} -> {current_value}",
                        "requires_approval": True,
                    }
                )

        for field, key_path in (
            ("forbidden_test_bypasses", ("testing", "forbidden_test_bypasses")),
            ("forbidden", ("references", "forbidden")),
        ):
            removals = _removed(_as_list(base_doc, *key_path), _as_list(current_doc, *key_path))
            if removals:
                relaxations.append(
                    {
                        "file": path.as_posix(),
                        "field": field,
                        "change": f"removed {', '.join(removals)}",
                        "requires_approval": True,
                    }
                )

    return relaxations


def _git_stdout(repo_root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=repo_root,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


def changed_boundary_files(repo_root: Path, base_ref: str) -> list[Path]:
    output = _git_stdout(repo_root, "diff", "--name-only", "--diff-filter=ACMR", base_ref, "--", "boundaries")
    files = [Path(line.strip()) for line in output.splitlines() if line.strip().endswith(".toml")]
    return sorted(files)


def load_base_toml(repo_root: Path, base_ref: str, relative_path: Path) -> dict | None:
    result = subprocess.run(
        ["git", "show", f"{base_ref}:{relative_path.as_posix()}"],
        cwd=repo_root,
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        return None
    return tomllib.loads(result.stdout.decode())


def detect_policy_relaxations(repo_root: Path, base_ref: str) -> list[dict]:
    relaxations: list[dict] = []
    for relative_path in changed_boundary_files(repo_root, base_ref):
        current_path = repo_root / relative_path
        if not current_path.exists():
            continue
        base_doc = load_base_toml(repo_root, base_ref, relative_path)
        if base_doc is None:
            continue
        current_doc = _read_toml(current_path)
        relaxations.extend(compare_boundary_policy(relative_path, base_doc, current_doc))
    return relaxations


def check_required_boundary_policy(repo_root: Path) -> list[dict]:
    violations: list[dict] = []

    runtime_doc = _read_toml(repo_root / RUNTIME_COMPOSITION)
    runtime_edges = set(_as_list(runtime_doc, "dependencies", "forbidden_edges"))
    if FORBIDDEN_EDGE not in runtime_edges:
        violations.append(
            {
                "category": "FORBIDDEN-EDGE",
                "detail": "runtime-composition.toml must forbid atm-daemon -> atm-rusqlite",
                "ref": _find_line(repo_root / RUNTIME_COMPOSITION, "forbidden_edges"),
            }
        )

    for relative_path in SQLITE_BOUNDARY_FILES:
        doc = _read_toml(repo_root / relative_path)
        allowed_dependents = set(_as_list(doc, "dependencies", "allowed_dependents"))
        forbidden_edges = set(_as_list(doc, "dependencies", "forbidden_edges"))
        if "atm-daemon" in allowed_dependents:
            violations.append(
                {
                    "category": "POLICY-RELAXATION",
                    "detail": "sqlite boundary still lists atm-daemon as an allowed dependent",
                    "ref": _find_line(repo_root / relative_path, "allowed_dependents"),
                }
            )
        if FORBIDDEN_EDGE not in forbidden_edges:
            violations.append(
                {
                    "category": "FORBIDDEN-EDGE",
                    "detail": "sqlite boundary must forbid atm-daemon -> atm-rusqlite",
                    "ref": _find_line(repo_root / relative_path, "forbidden_edges"),
                }
            )

    return violations


def _dependency_section_violations(doc: dict, cargo_path: Path, section_name: str, table: dict | None) -> list[dict]:
    if not isinstance(table, dict) or "atm-rusqlite" not in table:
        return []
    return [
        {
            "category": "FORBIDDEN-EDGE",
            "detail": f"crates/atm-daemon/Cargo.toml must not declare atm-rusqlite in {section_name}",
            "ref": _find_line(cargo_path, "atm-rusqlite"),
        }
    ]


def check_forbidden_code_edge(repo_root: Path) -> list[dict]:
    violations: list[dict] = []
    cargo_path = repo_root / "crates/atm-daemon/Cargo.toml"
    cargo_doc = _read_toml(cargo_path)

    for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
        violations.extend(
            _dependency_section_violations(cargo_doc, cargo_path, section_name, cargo_doc.get(section_name))
        )

    for target_name, target_table in cargo_doc.get("target", {}).items():
        if not isinstance(target_table, dict):
            continue
        for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
            full_section = f"target.{target_name}.{section_name}"
            violations.extend(
                _dependency_section_violations(
                    cargo_doc,
                    cargo_path,
                    full_section,
                    target_table.get(section_name),
                )
            )

    daemon_src = repo_root / "crates/atm-daemon/src"
    for path in sorted(daemon_src.rglob("*.rs")):
        for number, line in enumerate(path.read_text().splitlines(), start=1):
            if "atm_rusqlite" in line:
                violations.append(
                    {
                        "category": "FORBIDDEN-EDGE",
                        "detail": "daemon source must not reference atm_rusqlite directly",
                        "ref": f"{path.as_posix()}:{number}",
                    }
                )

    return violations


def build_report(repo_root: Path, base_ref: str) -> dict:
    policy_relaxations = detect_policy_relaxations(repo_root, base_ref)
    violations = [
        *check_required_boundary_policy(repo_root),
        *check_forbidden_code_edge(repo_root),
    ]
    return {
        "status": "FAIL" if violations else "PASS",
        "forbidden_edges": [FORBIDDEN_EDGE],
        "policy_relaxations": policy_relaxations,
        "violations": violations,
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--base-ref", default="HEAD~1")
    parser.add_argument("--pretty", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    report = build_report(args.repo_root.resolve(), args.base_ref)
    json.dump(report, sys.stdout, indent=2 if args.pretty else None)
    sys.stdout.write("\n")
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
