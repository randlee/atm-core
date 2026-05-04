#!/usr/bin/env python3
from __future__ import annotations

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
from build_view_site import build_site


VIEW_ORDER = ("boundaries", "modules", "deps", "unsafe")


@dataclass(frozen=True)
class ViewTask:
    name: str
    command: list[str]


@dataclass(frozen=True)
class ViewResult:
    task: ViewTask
    returncode: int
    stdout: str
    stderr: str
    duration_seconds: float
    log_path: Path


def build_tasks(repo_root: Path) -> dict[str, ViewTask]:
    python_executable = sys.executable
    return {
        "boundaries": ViewTask("boundaries", [python_executable, str(repo_root / ".just/view_boundaries.py")]),
        "modules": ViewTask("modules", [python_executable, str(repo_root / ".just/view_modules.py")]),
        "deps": ViewTask("deps", [python_executable, str(repo_root / ".just/view_deps.py")]),
        "unsafe": ViewTask("unsafe", [python_executable, str(repo_root / ".just/view_unsafe.py")]),
    }


def resolve_task_names(target: str) -> list[str]:
    if target == "all":
        return list(VIEW_ORDER)
    valid = {"all", *VIEW_ORDER}
    if target not in valid:
        valid_display = ", ".join(sorted(valid))
        raise ValueError(f"unknown view target: {target}; expected one of: {valid_display}")
    return [target]


def build_transcript(task: ViewTask, result: ViewResult, repo_root: Path) -> list[str]:
    return [
        f"view: {task.name}",
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


def run_task(task: ViewTask, repo_root: Path) -> ViewResult:
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
    log_path = make_log_path(repo_root, f"view-{task.name}", started_at)
    result = ViewResult(
        task=task,
        returncode=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
        duration_seconds=duration_seconds,
        log_path=log_path,
    )
    write_log(log_path, build_transcript(task, result, repo_root))
    return result


def print_result(result: ViewResult, repo_root: Path) -> None:
    lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    if result.returncode == 0:
        if lines:
            print(lines[0])
        else:
            print(f"{result.task.name} view generated [{format_duration(result.duration_seconds)}]")
        return

    log_display = relative_log_path(repo_root, result.log_path)
    print(f"{result.task.name} view failed")
    preview = lines[:3] or [line.strip() for line in result.stderr.splitlines() if line.strip()][:3]
    for line in preview:
        print(f"  {line}")
    print(f"  full log: {log_display}")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run architecture visualization targets.")
    parser.add_argument("target", nargs="?", default="all")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    try:
        task_names = resolve_task_names(args.target)
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 2

    tasks = build_tasks(repo_root)
    results: list[ViewResult] = []
    for task_name in task_names:
        result = run_task(tasks[task_name], repo_root)
        print_result(result, repo_root)
        results.append(result)

    try:
        site_path = build_site(repo_root)
        print(f"view index generated: {site_path.relative_to(repo_root).as_posix()}")
    except SystemExit as error:
        print(f"view index generation failed: {error}")
        return 1

    failures = [result for result in results if result.returncode != 0]
    if failures:
        print(f"view failed: {len(failures)} target(s) failed")
        return 1

    print(f"view completed: {len(results)} target(s) generated")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
