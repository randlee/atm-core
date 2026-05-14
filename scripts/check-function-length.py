#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import argparse
import os
import re
import subprocess
import sys


DEFAULT_WARN_THRESHOLD = 70
DEFAULT_FAIL_THRESHOLD = 80
DEFAULT_BASE_REF = "origin/develop"
FUNCTION_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b"
)
HUNK_RE = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


@dataclass(frozen=True)
class FunctionSpan:
    path: Path
    name: str
    start_line: int
    end_line: int

    @property
    def line_count(self) -> int:
        return self.end_line - self.start_line + 1


def discover_repo_root(explicit_root: str | None = None) -> Path:
    if explicit_root is not None:
        return Path(explicit_root).resolve()
    return Path(__file__).resolve().parent.parent


def workspace_manifest_paths(repo_root: Path) -> list[Path]:
    import tomllib

    root_manifest = tomllib.loads((repo_root / "Cargo.toml").read_text(encoding="utf-8"))
    workspace = root_manifest.get("workspace", {})
    if not isinstance(workspace, dict):
        return []

    members = workspace.get("members", [])
    if not isinstance(members, list) or not all(isinstance(item, str) for item in members):
        return []

    manifests: dict[Path, Path] = {}
    for pattern in members:
        for match in repo_root.glob(pattern):
            if match.is_dir():
                manifest_path = match / "Cargo.toml"
            elif match.is_file() and match.name == "Cargo.toml":
                manifest_path = match
            else:
                continue
            if manifest_path.exists():
                manifests[manifest_path.resolve()] = manifest_path
    return sorted(manifests.values())


def iter_workspace_rust_files(repo_root: Path) -> list[Path]:
    paths: list[Path] = []
    for manifest_path in workspace_manifest_paths(repo_root):
        crate_root = manifest_path.parent
        for directory_name in ("src", "tests"):
            directory = crate_root / directory_name
            if directory.exists():
                paths.extend(sorted(directory.rglob("*.rs")))
    return sorted(set(paths))


def is_test_only_path(path: Path) -> bool:
    if "tests" in path.parts:
        return True
    name = path.name
    return (
        name == "tests.rs"
        or name.startswith("test_")
        or name.endswith("_test.rs")
        or name.endswith("_tests.rs")
        or "test_support" in name
    )


def has_test_attribute(lines: list[str], start_index: int) -> bool:
    index = start_index - 1
    while index >= 0:
        stripped = lines[index].strip()
        if not stripped:
            break
        if stripped.startswith("#["):
            lowered = stripped.lower()
            if "test" in lowered:
                return True
            index -= 1
            continue
        break
    return False


def find_function_spans(path: Path) -> list[FunctionSpan]:
    lines = path.read_text(encoding="utf-8").splitlines()
    spans: list[FunctionSpan] = []
    index = 0
    while index < len(lines):
        match = FUNCTION_RE.match(lines[index])
        if match is None:
            index += 1
            continue
        if has_test_attribute(lines, index):
            index += 1
            continue

        name = match.group(1)
        signature_end = index
        while signature_end < len(lines) and "{" not in lines[signature_end]:
            signature_end += 1
        if signature_end >= len(lines):
            index += 1
            continue

        depth = 0
        started = False
        end_index: int | None = None
        for candidate in range(signature_end, len(lines)):
            for character in lines[candidate]:
                if character == "{":
                    depth += 1
                    started = True
                elif character == "}":
                    depth -= 1
            if started and depth == 0:
                end_index = candidate
                break
        if end_index is None:
            index += 1
            continue

        spans.append(FunctionSpan(path=path, name=name, start_line=index + 1, end_line=end_index + 1))
        index = end_index + 1
    return spans


def resolve_base_ref(explicit_base_ref: str | None) -> str:
    if explicit_base_ref:
        return explicit_base_ref
    env_base = os.environ.get("ATM_LINT_BASE_REF")
    if env_base:
        return env_base
    github_base = os.environ.get("GITHUB_BASE_REF")
    if github_base:
        return f"origin/{github_base}"
    return DEFAULT_BASE_REF


def changed_lines_by_file(repo_root: Path, base_ref: str) -> dict[str, set[int]]:
    completed = subprocess.run(
        ["git", "diff", "--unified=0", "--no-color", f"{base_ref}...HEAD", "--", "*.rs"],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise SystemExit(f"unable to diff against {base_ref}: {completed.stderr.strip() or completed.stdout.strip()}")

    changed: dict[str, set[int]] = {}
    current_path: str | None = None
    for line in completed.stdout.splitlines():
        if line.startswith("+++ b/"):
            current_path = line[6:]
            changed.setdefault(current_path, set())
            continue
        match = HUNK_RE.match(line)
        if match is None or current_path is None:
            continue
        start = int(match.group(1))
        count = int(match.group(2) or "1")
        if count == 0:
            continue
        changed[current_path].update(range(start, start + count))
    return changed


def overlaps_changed_lines(function: FunctionSpan, changed_lines: set[int]) -> bool:
    return any(line in changed_lines for line in range(function.start_line, function.end_line + 1))


def render(repo_root: Path, function: FunctionSpan) -> str:
    return (
        f"{function.path.relative_to(repo_root)}:{function.start_line}-{function.end_line}: "
        f"{function.name} ({function.line_count} lines)"
    )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Enforce RULE-002 function-length thresholds on non-test Rust code.")
    parser.add_argument("--root", help="Explicit repo root.")
    parser.add_argument("--base-ref", help="Git base ref used for grandfathered-diff comparison.")
    parser.add_argument("--warn-threshold", type=int, default=DEFAULT_WARN_THRESHOLD)
    parser.add_argument("--fail-threshold", type=int, default=DEFAULT_FAIL_THRESHOLD)
    args = parser.parse_args(argv[1:])

    repo_root = discover_repo_root(args.root)
    base_ref = resolve_base_ref(args.base_ref)
    changed_by_file = changed_lines_by_file(repo_root, base_ref)

    new_failures: list[FunctionSpan] = []
    grandfathered_failures: list[FunctionSpan] = []
    advisories: list[FunctionSpan] = []

    for rust_path in iter_workspace_rust_files(repo_root):
        relative_path = rust_path.relative_to(repo_root)
        if is_test_only_path(relative_path):
            continue
        changed_lines = changed_by_file.get(relative_path.as_posix(), set())
        for function in find_function_spans(rust_path):
            if function.line_count >= args.fail_threshold:
                if overlaps_changed_lines(function, changed_lines):
                    new_failures.append(function)
                else:
                    grandfathered_failures.append(function)
            elif function.line_count >= args.warn_threshold:
                advisories.append(function)

    print(f"function-length base ref: {base_ref}")

    if advisories:
        print("RULE-002 advisory (70-79 lines):")
        for function in advisories:
            print(render(repo_root, function))

    if grandfathered_failures:
        print("RULE-002 grandfathered hard violations (80+ lines, unchanged in this diff):")
        for function in grandfathered_failures:
            print(render(repo_root, function))

    if new_failures:
        print("RULE-002 failed: new hard violations (80+ lines) overlap the current diff:")
        for function in new_failures:
            print(render(repo_root, function))
        return 1

    print("function-length passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
