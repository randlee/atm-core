#!/usr/bin/env python3
from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import subprocess
import sys
import time

from lint_common import discover_repo_root
from lint_common import format_duration
from lint_common import make_log_path
from lint_common import relative_log_path
from lint_common import write_log


PYTHON_LINT_ORDER = (
    "version",
    "boundaries",
    "manifests",
    "identities",
    "lines",
    "spell",
    "pytests",
)
CARGO_LINT_ORDER = ("fmt", "clippy", "deny", "shear")
HIGH_VOLUME_LINTS = {"identities", "lines"}
COUNT_PATTERNS = (
    ("total violations:", "violations"),
    ("errors:", "errors"),
)


@dataclass(frozen=True)
class LintTask:
    name: str
    command: list[str]


@dataclass(frozen=True)
class LintResult:
    task: LintTask
    returncode: int
    stdout: str
    stderr: str
    duration_seconds: float
    log_path: Path


def build_tasks(repo_root: Path) -> dict[str, LintTask]:
    python_executable = sys.executable
    return {
        "fmt": LintTask("fmt", ["just", "_lint-fmt"]),
        "clippy": LintTask("clippy", ["just", "_lint-clippy"]),
        "deny": LintTask("deny", [python_executable, str(repo_root / ".just/lint_cargo_deny.py")]),
        "shear": LintTask("shear", [python_executable, str(repo_root / ".just/lint_cargo_shear.py")]),
        "version": LintTask("version", [python_executable, str(repo_root / ".just/check_version_sync.py")]),
        "identities": LintTask(
            "identities", [python_executable, str(repo_root / ".just/check_test_identity_literals.py")]
        ),
        "lines": LintTask("lines", [python_executable, str(repo_root / ".just/check_line_counts.py")]),
        "boundaries": LintTask("boundaries", [python_executable, str(repo_root / ".just/lint_boundaries.py")]),
        "manifests": LintTask("manifests", [python_executable, str(repo_root / ".just/lint_manifests.py")]),
        "spell": LintTask("spell", [python_executable, str(repo_root / ".just/lint_codespell.py")]),
        "pytests": LintTask(
            "pytests",
            [python_executable, "-m", "unittest", "discover", "-s", str(repo_root / ".just/tests"), "-p", "test_*.py"],
        ),
    }


def resolve_task_names(target: str) -> list[str]:
    if target == "all":
        return [*CARGO_LINT_ORDER, *PYTHON_LINT_ORDER]
    valid = {"all", *CARGO_LINT_ORDER, *PYTHON_LINT_ORDER}
    if target not in valid:
        valid_display = ", ".join(sorted(valid))
        raise ValueError(f"unknown lint target: {target}; expected one of: {valid_display}")
    return [target]


def interesting_lines(output: str) -> list[str]:
    lines = [line.strip() for line in output.splitlines() if line.strip()]
    filtered = [line for line in lines if not line.startswith(("python ", "python3 ", "cargo "))]
    return filtered or lines


def extract_count(lines: list[str]) -> int | None:
    for line in reversed(lines):
        lowered = line.lower()
        for prefix, _label in COUNT_PATTERNS:
            if prefix in lowered:
                trailing = lowered.split(prefix, 1)[1].strip()
                if trailing.isdigit():
                    return int(trailing)
        if lowered.startswith("[") and "] errors in " in lowered:
            count = lowered.split("]", 1)[0].lstrip("[")
            if count.isdigit():
                return int(count)
    return None


def build_transcript(task: LintTask, result: LintResult, repo_root: Path) -> list[str]:
    return [
        f"lint: {task.name}",
        f"repo_root: {repo_root}",
        f"recorded_at_utc: {datetime.now(timezone.utc).isoformat()}",
        f"duration: {format_duration(result.duration_seconds)}",
        f"command: {' '.join(task.command)}",
        f"exit_code: {result.returncode}",
        "",
        "stdout:",
        result.stdout.rstrip(),
        "",
        "stderr:",
        result.stderr.rstrip(),
    ]


def run_task(task: LintTask, repo_root: Path) -> LintResult:
    started_at = datetime.now(timezone.utc)
    start_time = time.perf_counter()
    completed = subprocess.run(
        task.command,
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    duration_seconds = time.perf_counter() - start_time
    log_path = make_log_path(repo_root, task.name, started_at)
    result = LintResult(
        task=task,
        returncode=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
        duration_seconds=duration_seconds,
        log_path=log_path,
    )
    write_log(log_path, build_transcript(task, result, repo_root))
    return result


def print_result(result: LintResult, repo_root: Path) -> None:
    if result.returncode == 0:
        print(f"{result.task.name} passed [{format_duration(result.duration_seconds)}]")
        return

    lines = interesting_lines("\n".join((result.stdout, result.stderr)))
    log_display = relative_log_path(repo_root, result.log_path)
    print(f"{result.task.name} failed")
    if result.task.name in HIGH_VOLUME_LINTS:
        preview = lines[:2]
        for line in preview:
            print(f"  {line}")
        count = extract_count(lines)
        if count is not None:
            print(f"  [{count}] errors in {log_display}")
        else:
            print(f"  full log: {log_display}")
        return

    preview = lines[:4]
    for line in preview:
        print(f"  {line}")
    print(f"  full log: {log_display}")


def run_parallel(tasks: list[LintTask], repo_root: Path) -> list[LintResult]:
    with ThreadPoolExecutor(max_workers=len(tasks)) as executor:
        futures = [executor.submit(run_task, task, repo_root) for task in tasks]
        return [future.result() for future in futures]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run repo lint targets.")
    parser.add_argument("target", nargs="?", default="all")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    target = args.target
    try:
        task_names = resolve_task_names(target)
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 2

    tasks = build_tasks(repo_root)
    selected_tasks = [tasks[name] for name in task_names]

    cargo_tasks = [task for task in selected_tasks if task.name in CARGO_LINT_ORDER]
    python_tasks = [task for task in selected_tasks if task.name in PYTHON_LINT_ORDER]
    results: list[LintResult] = []

    for task in cargo_tasks:
        result = run_task(task, repo_root)
        print_result(result, repo_root)
        results.append(result)

    if python_tasks:
        for result in run_parallel(python_tasks, repo_root):
            print_result(result, repo_root)
            results.append(result)

    failures = [result for result in results if result.returncode != 0]
    if failures:
        print(f"lint failed: {len(failures)} check(s) failed")
        return 1

    print(f"lint passed: {len(results)} check(s) succeeded")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
