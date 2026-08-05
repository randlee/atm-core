#!/usr/bin/env python3
"""Enforce the configured runtime-observation source boundary."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import sys

from lint_common import discover_repo_root, is_code_line, load_lint_config, rust_file_test_scope


LINT_NAME = "runtime-observation-boundary"


@dataclass(frozen=True)
class BoundaryViolation:
    path: str
    line_number: int
    line: str
    kind: str

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: [{self.kind}] {self.line}"


def boundary_config(repo_root: Path) -> dict:
    config = load_lint_config(repo_root).get(LINT_NAME, {})
    if not isinstance(config, dict):
        raise SystemExit(f"[{LINT_NAME}] must be a TOML table")
    return config


def _string_list(config: dict, key: str) -> tuple[str, ...]:
    values = config.get(key)
    if not isinstance(values, list) or not all(isinstance(value, str) and value for value in values):
        raise SystemExit(f"[{LINT_NAME}].{key} must be an array of strings")
    return tuple(values)


def load_tokens(repo_root: Path) -> tuple[str, ...]:
    return _string_list(boundary_config(repo_root), "tokens")


def load_allowed_paths(repo_root: Path) -> frozenset[str]:
    return frozenset(_string_list(boundary_config(repo_root), "allowed_paths"))


def load_required_positive(repo_root: Path) -> tuple[tuple[str, str], ...]:
    entries = boundary_config(repo_root).get("required_positive")
    if not isinstance(entries, list):
        raise SystemExit(f"[{LINT_NAME}].required_positive must be an array of tables")
    result: list[tuple[str, str]] = []
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict) or not isinstance(entry.get("path"), str) or not isinstance(entry.get("symbol"), str):
            raise SystemExit(f"[{LINT_NAME}].required_positive[{index}] needs path and symbol strings")
        result.append((entry["path"], entry["symbol"]))
    return tuple(result)


def rust_sources(repo_root: Path) -> list[Path]:
    crates = repo_root / "crates"
    return sorted(crates.rglob("*.rs")) if crates.exists() else []


def collect_runtime_observation_boundary_violations(
    repo_root: Path,
    *,
    tokens: tuple[str, ...],
    allowed_paths: frozenset[str],
    required_positive: tuple[tuple[str, str], ...],
) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    for required_path, symbol in required_positive:
        path = repo_root / required_path
        if not path.exists():
            violations.append(BoundaryViolation(required_path, 0, symbol, "required_positive_missing"))
        elif symbol not in path.read_text(encoding="utf-8"):
            violations.append(BoundaryViolation(required_path, 0, symbol, "required_positive_missing"))

    for source in rust_sources(repo_root):
        relative = source.relative_to(repo_root).as_posix()
        lines = source.read_text(encoding="utf-8").splitlines()
        test_scope = rust_file_test_scope(Path(relative), lines)
        if source.name.endswith("_tests.rs") or source.name == "integration_tests.rs":
            continue
        if relative in allowed_paths:
            continue
        for line_number, (line, in_test_scope) in enumerate(zip(lines, test_scope, strict=True), start=1):
            if in_test_scope or not is_code_line(line):
                continue
            if any(token in line for token in tokens):
                violations.append(BoundaryViolation(relative, line_number, line.strip(), "source_use_not_allowed"))
    return violations


def main(argv: list[str]) -> int:
    repo_root = discover_repo_root(argv[1] if len(argv) > 1 else None)
    violations = collect_runtime_observation_boundary_violations(
        repo_root,
        tokens=load_tokens(repo_root),
        allowed_paths=load_allowed_paths(repo_root),
        required_positive=load_required_positive(repo_root),
    )
    if violations:
        print("\n".join(violation.render() for violation in violations))
        return 1
    print(f"{LINT_NAME} passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
