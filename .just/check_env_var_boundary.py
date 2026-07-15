#!/usr/bin/env python3
"""Enforce the ATM_TEAM/ATM_IDENTITY client-boundary rule.

ATM_TEAM and ATM_IDENTITY are CLI-identity environment variables that must
only be read at client entry points (the `atm` CLI and the `atm-graft`
client). Library crates (`atm-core`, `atm-daemon`) must receive already
resolved values as parameters instead of reading these variables themselves.

This lint flags:
  * direct `env::var("ATM_TEAM"/"ATM_IDENTITY")` /
    `env::var_os("ATM_TEAM"/"ATM_IDENTITY")` calls,
  * calls that route one of the forbidden literals through a same-file helper
    function whose body forwards its string parameter straight into
    `env::var`/`env::var_os` (e.g. `read_env_raw("ATM_IDENTITY")` where
    `read_env_raw(key: &str)` calls `env::var_os(key)`), and
  * calls to the explicitly configured `boundary_reader_functions` and any
    same-file wrapper functions that call them transitively (the known
    CLI-only resolver functions that read these variables internally), from
    any other production source file in the restricted crate roots.

Findings for pre-existing call sites may be allowlisted in
`.just/allowlists/env_var_boundary_allowlist.toml` while the follow-up
refactor to push these reads out to the client boundary is scheduled.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import re
import sys
import tomllib

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import is_code_line
from lint_common import load_lint_config
from lint_common import monotonic_now
from lint_common import print_report
from lint_common import rust_file_test_scope
from lint_common import workspace_crate_section_lines


LINT_NAME = "env-var-boundary"
ALLOWLIST_PATH = Path(".just/allowlists/env_var_boundary_allowlist.toml")

ENV_CALL_RE = re.compile(
    r"\b(?:std::)?env::var(?:_os)?\s*\(\s*(?:\"(?P<literal>[^\"]*)\"|(?P<ident>[A-Za-z_][A-Za-z0-9_]*))\s*\)"
)
FN_SINGLE_STR_PARAM_RE = re.compile(
    r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*:\s*&str\s*\)"
)
CALL_LITERAL_RE = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(\s*\"(?P<literal>[^\"]*)\"")
FN_DEF_RE = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")


@dataclass(frozen=True)
class EnvVarViolation:
    path: str
    line_number: int
    line: str
    symbol: str
    kind: str

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: [{self.kind}] {self.line}"


@dataclass(frozen=True)
class AllowlistEntry:
    rule: str
    path: Path
    symbol: str
    line: str
    why: str
    sunset_sprint: str


def env_boundary_config(repo_root: Path) -> dict:
    config = load_lint_config(repo_root).get("env_var_boundary", {})
    if not isinstance(config, dict):
        raise SystemExit("[env_var_boundary] must be a TOML table")
    return config


def load_forbidden_env_vars(repo_root: Path) -> tuple[str, ...]:
    config = env_boundary_config(repo_root)
    values = config.get("forbidden_env_vars")
    if not isinstance(values, list) or not all(isinstance(item, str) for item in values):
        raise SystemExit("[env_var_boundary].forbidden_env_vars must be an array of strings")
    return tuple(values)


def load_restricted_crate_roots(repo_root: Path) -> tuple[Path, ...]:
    config = env_boundary_config(repo_root)
    values = config.get("restricted_crate_roots")
    if not isinstance(values, list) or not all(isinstance(item, str) for item in values):
        raise SystemExit("[env_var_boundary].restricted_crate_roots must be an array of strings")
    return tuple(Path(item) for item in values)


def load_boundary_reader_functions(repo_root: Path) -> tuple[str, ...]:
    config = env_boundary_config(repo_root)
    values = config.get("boundary_reader_functions", [])
    if not isinstance(values, list) or not all(isinstance(item, str) for item in values):
        raise SystemExit("[env_var_boundary].boundary_reader_functions must be an array of strings")
    return tuple(values)


def load_allowlist(repo_root: Path) -> list[AllowlistEntry]:
    allowlist_path = repo_root / ALLOWLIST_PATH
    if not allowlist_path.exists():
        return []
    data = tomllib.loads(allowlist_path.read_text(encoding="utf-8"))
    raw_entries = data.get("allow", [])
    if not isinstance(raw_entries, list):
        raise SystemExit("[allow] must be an array of tables")

    entries: list[AllowlistEntry] = []
    for index, raw_entry in enumerate(raw_entries):
        if not isinstance(raw_entry, dict):
            raise SystemExit(f"[allow][{index}] must be a TOML table")
        required = ("rule", "path", "symbol", "line", "why", "sunset_sprint")
        for field in required:
            value = raw_entry.get(field)
            if not isinstance(value, str) or not value.strip():
                raise SystemExit(f"[allow][{index}].{field} must be a non-empty string")
        entries.append(
            AllowlistEntry(
                rule=raw_entry["rule"],
                path=Path(raw_entry["path"]),
                symbol=raw_entry["symbol"],
                line=raw_entry["line"],
                why=raw_entry["why"],
                sunset_sprint=raw_entry["sunset_sprint"],
            )
        )
    return entries


def is_allowlisted(
    entries: list[AllowlistEntry],
    *,
    rel_path: Path,
    symbol: str,
    line: str,
) -> bool:
    """Match an exact previously-reviewed call site, not the whole function.

    Allowlist entries are keyed on (path, symbol, exact violated line text) so
    that a genuinely new env-boundary violation added inside an already
    allowlisted function -- even one that shares the same enclosing symbol --
    still requires its own allowlist entry instead of being silently
    swallowed by the enclosing function's existing entry.
    """
    return any(
        entry.path == rel_path and entry.symbol == symbol and entry.line == line
        for entry in entries
    )


def restricted_rust_files(repo_root: Path, restricted_crate_roots: tuple[Path, ...]) -> list[Path]:
    paths: list[Path] = []
    for root in restricted_crate_roots:
        root_dir = repo_root / root
        if root_dir.exists():
            paths.extend(sorted(root_dir.rglob("*.rs")))
    return sorted(set(paths))


def enclosing_function_name(lines: list[str], line_number: int) -> str:
    for index in range(line_number - 1, -1, -1):
        match = FN_DEF_RE.match(lines[index].strip())
        if match is not None:
            return match.group(1)
    return "<module scope>"


def function_body_bounds(lines: list[str], signature_line_index: int) -> tuple[int, int]:
    """Return the (start, end) 0-indexed line range spanned by a function body."""
    depth = 0
    started = False
    end_index = signature_line_index
    for index in range(signature_line_index, len(lines)):
        depth += lines[index].count("{") - lines[index].count("}")
        if lines[index].count("{") > 0:
            started = True
        end_index = index
        if started and depth <= 0:
            break
    return signature_line_index, end_index


def collect_same_file_forwarding_functions(lines: list[str]) -> set[str]:
    """Find functions whose body forwards their single &str parameter into env::var(_os)."""
    forwarding: set[str] = set()
    for index, line in enumerate(lines):
        match = FN_SINGLE_STR_PARAM_RE.search(line)
        if match is None:
            continue
        function_name, param_name = match.group(1), match.group(2)
        start, end = function_body_bounds(lines, index)
        for body_line in lines[start : end + 1]:
            for call_match in ENV_CALL_RE.finditer(body_line):
                if call_match.group("ident") == param_name:
                    forwarding.add(function_name)
                    break
    return forwarding


def find_boundary_reader_definition_files(
    file_lines: dict[Path, list[str]],
    boundary_reader_functions: tuple[str, ...],
) -> dict[str, Path]:
    """Map each configured boundary-reader function name to the file that defines it.

    Calls to a boundary-reader function from within its own defining file are
    internal implementation details of that resolver (already covered, and
    allowlisted where needed, via the forwarding-function detection), not a
    new client of the env read, so they are excluded from Rule C.
    """
    definition_files: dict[str, Path] = {}
    for rel_path, lines in file_lines.items():
        for line in lines:
            match = FN_DEF_RE.match(line.strip())
            if match is not None and match.group(1) in boundary_reader_functions:
                definition_files.setdefault(match.group(1), rel_path)
    return definition_files


def find_function_definition_line(lines: list[str], function_name: str) -> int | None:
    for index, line in enumerate(lines):
        match = FN_DEF_RE.match(line.strip())
        if match is not None and match.group(1) == function_name:
            return index
    return None


def expand_boundary_reader_functions(
    file_lines: dict[Path, list[str]],
    boundary_reader_functions: tuple[str, ...],
) -> tuple[str, ...]:
    """Treat same-file wrappers around boundary readers as boundary readers too.

    The configured names are the seed readers at the actual client boundary.
    Any function defined in the same file that calls one of those readers is
    itself also a boundary reader, because callers outside that file still
    trigger an ATM_TEAM/ATM_IDENTITY read indirectly through the wrapper.
    """
    expanded = set(boundary_reader_functions)
    definition_files = find_boundary_reader_definition_files(file_lines, boundary_reader_functions)
    boundary_files = {path for path in definition_files.values()}

    changed = True
    while changed:
        changed = False
        for rel_path in boundary_files:
            lines = file_lines[rel_path]
            for index, line in enumerate(lines):
                match = FN_DEF_RE.match(line.strip())
                if match is None:
                    continue
                function_name = match.group(1)
                if function_name in expanded:
                    continue
                start = index
                _, end = function_body_bounds(lines, start)
                body_lines = lines[start : end + 1]
                if any(
                    re.search(
                        rf"\b(?:[A-Za-z_][A-Za-z0-9_]*::)*{re.escape(boundary_reader)}\s*\(",
                        body_line,
                    )
                    for boundary_reader in expanded
                    for body_line in body_lines
                ):
                    expanded.add(function_name)
                    changed = True

    return tuple(sorted(expanded))


def collect_file_violations(
    *,
    rel_path: Path,
    lines: list[str],
    scope: list[bool],
    forbidden_env_vars: tuple[str, ...],
    boundary_reader_functions: tuple[str, ...],
    boundary_reader_definition_files: dict[str, Path],
) -> list[EnvVarViolation]:
    violations: list[EnvVarViolation] = []
    forwarding_functions = collect_same_file_forwarding_functions(lines)

    for line_number, (line, in_test_scope) in enumerate(zip(lines, scope, strict=True), start=1):
        if in_test_scope or not is_code_line(line):
            continue

        for call_match in ENV_CALL_RE.finditer(line):
            literal = call_match.group("literal")
            if literal is not None and literal in forbidden_env_vars:
                violations.append(
                    EnvVarViolation(
                        path=rel_path.as_posix(),
                        line_number=line_number,
                        line=line.strip(),
                        symbol=enclosing_function_name(lines, line_number),
                        kind="direct_literal_env_read",
                    )
                )

        for call_match in CALL_LITERAL_RE.finditer(line):
            func_name = call_match.group(1)
            literal = call_match.group("literal")
            if literal not in forbidden_env_vars:
                continue
            if FN_DEF_RE.match(line.strip()):
                continue
            if func_name in forwarding_functions:
                violations.append(
                    EnvVarViolation(
                        path=rel_path.as_posix(),
                        line_number=line_number,
                        line=line.strip(),
                        symbol=enclosing_function_name(lines, line_number),
                        kind="env_var_via_forwarding_function",
                    )
                )

        stripped = line.strip()
        if FN_DEF_RE.match(stripped):
            continue
        for function_name in boundary_reader_functions:
            if boundary_reader_definition_files.get(function_name) == rel_path:
                continue
            if re.search(rf"\b(?:[A-Za-z_][A-Za-z0-9_]*::)*{re.escape(function_name)}\s*\(", line):
                violations.append(
                    EnvVarViolation(
                        path=rel_path.as_posix(),
                        line_number=line_number,
                        line=line.strip(),
                        symbol=enclosing_function_name(lines, line_number),
                        kind="boundary_reader_function_call",
                    )
                )

    return violations


def collect_env_var_boundary_violations(
    repo_root: Path,
    *,
    forbidden_env_vars: tuple[str, ...],
    restricted_crate_roots: tuple[Path, ...],
    boundary_reader_functions: tuple[str, ...],
    allowlist: list[AllowlistEntry],
) -> list[EnvVarViolation]:
    violations: list[EnvVarViolation] = []
    file_lines: dict[Path, list[str]] = {}
    for abs_path in restricted_rust_files(repo_root, restricted_crate_roots):
        rel_path = abs_path.relative_to(repo_root)
        file_lines[rel_path] = abs_path.read_text(encoding="utf-8").splitlines()

    boundary_reader_functions = expand_boundary_reader_functions(file_lines, boundary_reader_functions)
    boundary_reader_definition_files = find_boundary_reader_definition_files(
        file_lines, boundary_reader_functions
    )

    for rel_path, lines in file_lines.items():
        scope = rust_file_test_scope(rel_path, lines)

        file_violations = collect_file_violations(
            rel_path=rel_path,
            lines=lines,
            scope=scope,
            forbidden_env_vars=forbidden_env_vars,
            boundary_reader_functions=boundary_reader_functions,
            boundary_reader_definition_files=boundary_reader_definition_files,
        )
        for violation in file_violations:
            if is_allowlisted(
                allowlist,
                rel_path=rel_path,
                symbol=violation.symbol,
                line=violation.line,
            ):
                continue
            violations.append(violation)

    return violations


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Reject ATM_TEAM/ATM_IDENTITY environment reads inside atm-core/atm-daemon library code."
    )
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    started_at = datetime.now(timezone.utc)
    start_time = monotonic_now()

    forbidden_env_vars = load_forbidden_env_vars(repo_root)
    restricted_crate_roots = load_restricted_crate_roots(repo_root)
    boundary_reader_functions = load_boundary_reader_functions(repo_root)
    allowlist = load_allowlist(repo_root)

    violations = collect_env_var_boundary_violations(
        repo_root,
        forbidden_env_vars=forbidden_env_vars,
        restricted_crate_roots=restricted_crate_roots,
        boundary_reader_functions=boundary_reader_functions,
        allowlist=allowlist,
    )
    duration_seconds = monotonic_now() - start_time

    findings = [violation.render() for violation in violations]
    transcript_lines = [
        *workspace_crate_section_lines(repo_root),
        "findings:",
        *(findings or ["none"]),
    ]
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not violations,
        summary=(
            "ATM_TEAM/ATM_IDENTITY environment reads found inside restricted library crates"
            if violations
            else "no disallowed ATM_TEAM/ATM_IDENTITY environment reads found inside restricted library crates"
        ),
        findings=findings,
        transcript_lines=transcript_lines,
        started_at=started_at,
        duration_seconds=duration_seconds,
    )

    for line in workspace_crate_section_lines(repo_root):
        print(line)

    if not report.passed:
        print(
            "ATM-ENV-BOUNDARY violation: ATM_TEAM/ATM_IDENTITY must only be read at "
            "client entry points (the atm CLI and atm-graft), not inside atm-core/atm-daemon."
        )
        for finding in report.findings:
            print(finding)
        print(f"total violations: {len(report.findings)}")
        print_report(report, repo_root=repo_root, preview_limit=0, direct_threshold=0)
        return 1

    print_report(report, repo_root=repo_root, preview_limit=0, direct_threshold=0)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
