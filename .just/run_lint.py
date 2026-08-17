#!/usr/bin/env python3
from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import re
import subprocess
import sys
import time

from lint_common import discover_repo_root
from lint_common import format_duration
from lint_common import make_log_path
from lint_common import relative_log_path
from lint_common import workspace_crate_section_lines
from lint_common import write_log


PYTHON_LINT_ORDER = (
    "version",
    "boundaries",
    "unix-gating",
    "same-host-portability",
    "runtime-waits",
    "manifests",
    "daemon-signing-coupling",
    "silent-emit",
    "function-length",
    "legacy-mailbox-paths",
    "capability-degradation",
    "identities",
    "env-var-boundary",
    "runtime-observation-boundary",
    "fixed-sleep",
    "ttl-triage",
    "lines",
    "spell",
    "hermes-adapter",
    "hermes-atm-boundary",
    "atm-graft-python-boundary",
    "daemon-singleton",
    "legacy-transport-removal",
    "pytests",
)
EXTRA_LINTS = ("sc-boundary", "sc-portability")
CARGO_LINT_ORDER = ("fmt", "clippy", "deny", "shear")
FAST_LINT_ORDER = (
    "fmt",
    "version",
    "boundaries",
    "manifests",
    "daemon-signing-coupling",
    "shear",
    "silent-emit",
    "function-length",
    "legacy-mailbox-paths",
    "capability-degradation",
    "spell",
    "hermes-adapter",
    "pytests",
)
HIGH_VOLUME_LINTS = {"identities", "lines"}
CRATE_INVENTORY_LINTS = {"fmt", "clippy", "modules", "boundaries", "sc-boundary", "sc-portability", "manifests"}
COUNT_PATTERNS = (
    ("total violations:", "violations"),
    ("errors:", "errors"),
)
FILE_FINDING_RE = re.compile(r"^[A-Za-z0-9_.-]+/.*:\d+:")
ANSI_ESCAPE_RE = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
ERROR_MARKER_RE = re.compile(r"(?<![A-Za-z0-9_])(error|failed|traceback|exception)(?![A-Za-z0-9_])")


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
    python_command = [sys.executable, "-B"]
    return {
        "fmt": LintTask("fmt", ["just", "_lint-fmt"]),
        "clippy": LintTask("clippy", ["just", "_lint-clippy"]),
        "modules": LintTask("modules", [*python_command, str(repo_root / ".just/lint_cargo_modules.py")]),
        "deny": LintTask("deny", [*python_command, str(repo_root / ".just/lint_cargo_deny.py")]),
        "shear": LintTask("shear", [*python_command, str(repo_root / ".just/lint_cargo_shear.py")]),
        "version": LintTask("version", [*python_command, str(repo_root / ".just/check_version_sync.py")]),
        "identities": LintTask(
            "identities", [*python_command, str(repo_root / ".just/check_test_identity_literals.py")]
        ),
        "env-var-boundary": LintTask(
            "env-var-boundary", [*python_command, str(repo_root / ".just/check_env_var_boundary.py")]
        ),
        "runtime-observation-boundary": LintTask(
            "runtime-observation-boundary",
            [*python_command, str(repo_root / ".just/check_runtime_observation_boundary.py")],
        ),
        "lines": LintTask("lines", [*python_command, str(repo_root / ".just/check_line_counts.py")]),
        "boundaries": LintTask("boundaries", [*python_command, str(repo_root / ".just/lint_boundaries.py")]),
        "unix-gating": LintTask(
            "unix-gating", [*python_command, str(repo_root / ".just/lint_unix_gating.py")]
        ),
        "same-host-portability": LintTask(
            "same-host-portability",
            [*python_command, str(repo_root / ".just/lint_same_host_portability.py")],
        ),
        "runtime-waits": LintTask(
            "runtime-waits", [*python_command, str(repo_root / ".just/lint_runtime_waits.py")]
        ),
        "sc-boundary": LintTask(
            "sc-boundary", [*python_command, str(repo_root / ".just/lint_sc_boundary.py")]
        ),
        "sc-portability": LintTask(
            "sc-portability", [*python_command, str(repo_root / ".just/lint_sc_portability.py")]
        ),
        "manifests": LintTask("manifests", [*python_command, str(repo_root / ".just/lint_manifests.py")]),
        "daemon-signing-coupling": LintTask(
            "daemon-signing-coupling",
            [*python_command, str(repo_root / ".just/lint_daemon_signing_coupling.py")],
        ),
        "silent-emit": LintTask(
            "silent-emit", [*python_command, str(repo_root / "scripts/check-silent-emit.py")]
        ),
        "function-length": LintTask(
            "function-length", [*python_command, str(repo_root / "scripts/check-function-length.py")]
        ),
        "legacy-mailbox-paths": LintTask(
            "legacy-mailbox-paths",
            [*python_command, str(repo_root / "scripts/check-legacy-mailbox-paths.py")],
        ),
        "capability-degradation": LintTask(
            "capability-degradation",
            [*python_command, str(repo_root / "scripts/check-capability-degradation.py")],
        ),
        "spell": LintTask("spell", [*python_command, str(repo_root / ".just/lint_codespell.py")]),
        "hermes-adapter": LintTask(
            "hermes-adapter", [*python_command, str(repo_root / ".just/lint_hermes_adapter.py")]
        ),
        "hermes-atm-boundary": LintTask(
            "hermes-atm-boundary", [*python_command, str(repo_root / ".just/lint_hermes_atm_boundary.py")]
        ),
        "atm-graft-python-boundary": LintTask(
            "atm-graft-python-boundary", [*python_command, str(repo_root / ".just/lint_atm_graft_python_boundary.py")]
        ),
        "fixed-sleep": LintTask(
            "fixed-sleep", [*python_command, str(repo_root / ".just/check_fixed_sleep_hygiene.py")]
        ),
        "ttl-triage": LintTask(
            "ttl-triage", [*python_command, str(repo_root / ".just/lint_ttl_triage_consistency.py")]
        ),
        "daemon-singleton": LintTask(
            "daemon-singleton",
            [*python_command, str(repo_root / "scripts/lint_daemon_singleton.py")],
        ),
        "legacy-transport-removal": LintTask(
            "legacy-transport-removal",
            [
                *python_command,
                str(repo_root / "scripts/phase-am/check_legacy_transport_removal.py"),
                "--category",
                "raw-framing",
                "--category",
                "peer-ingress",
                "--category",
                "resend-replay",
                "--category",
                "direct-sqlite",
                "--category",
                "dead-daemon-dispatch",
            ],
        ),
        "pytests": LintTask("pytests", [*python_command, str(repo_root / ".just/run_pytests.py")]),
    }


def resolve_task_names(target: str) -> list[str]:
    if target == "all":
        return [*CARGO_LINT_ORDER, *PYTHON_LINT_ORDER]
    if target == "fast":
        return list(FAST_LINT_ORDER)
    valid = {"all", "fast", *CARGO_LINT_ORDER, *PYTHON_LINT_ORDER, *EXTRA_LINTS}
    if target not in valid:
        valid_display = ", ".join(sorted(valid))
        raise ValueError(f"unknown lint target: {target}; expected one of: {valid_display}")
    return [target]


def strip_ansi(text: str) -> str:
    return ANSI_ESCAPE_RE.sub("", text)


def interesting_lines(output: str) -> list[str]:
    lines = [strip_ansi(line).strip() for line in output.splitlines() if line.strip()]
    filtered = [line for line in lines if not line.startswith(("python ", "python3 ", "cargo "))]
    return filtered or lines


def prioritize_error_lines(lines: list[str]) -> list[str]:
    error_lines = [
        line
        for line in lines
        if ERROR_MARKER_RE.search(line.lower()) or "could not" in line.lower()
    ]
    return error_lines or lines


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


def preview_lines_for_task(task_name: str, lines: list[str]) -> list[str]:
    if task_name == "sc-boundary":
        filtered = [
            line
            for line in lines
            if line.strip() != "sc-boundary failed" and not line.strip().startswith("full log:")
        ]
        return filtered or lines
    if task_name != "identities":
        return lines

    filtered: list[str] = []
    for line in lines:
        stripped = line.strip()
        if not stripped:
            continue
        if stripped == "crates analyzed:":
            continue
        if "crate_path" in stripped and "manifest" in stripped:
            continue
        if "Cargo.toml" in stripped:
            continue
        if stripped.lower().startswith("rule-"):
            filtered.append(stripped)
            continue
        if FILE_FINDING_RE.match(stripped):
            filtered.append(stripped)
            continue

    return filtered or lines


def failure_preview(task_name: str, lines: list[str]) -> list[str]:
    """Return actionable CI output without hiding a Python test traceback."""
    if task_name == "pytests":
        return lines[-40:]
    return prioritize_error_lines(lines)[:4]


def build_transcript(task: LintTask, result: LintResult, repo_root: Path) -> list[str]:
    transcript = [
        f"lint: {task.name}",
        f"repo_root: {repo_root}",
        f"recorded_at_utc: {datetime.now(timezone.utc).isoformat()}",
        f"duration: {format_duration(result.duration_seconds)}",
        f"command: {' '.join(task.command)}",
        f"exit_code: {result.returncode}",
        "",
    ]
    if task.name in CRATE_INVENTORY_LINTS:
        transcript.extend(workspace_crate_section_lines(repo_root))
    if task.name == "boundaries":
        from lint_boundaries import boundary_doc_section_lines
        from lint_boundaries import parse_boundary_records

        records, _violations = parse_boundary_records(repo_root)
        transcript.extend(boundary_doc_section_lines(repo_root, records))
    transcript.extend(
        [
            "stdout:",
            (result.stdout or "").rstrip(),
            "",
            "stderr:",
            (result.stderr or "").rstrip(),
        ]
    )
    return transcript


def run_task(task: LintTask, repo_root: Path) -> LintResult:
    started_at = datetime.now(timezone.utc)
    start_time = time.perf_counter()
    completed = subprocess.run(
        task.command,
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
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

    lines = preview_lines_for_task(result.task.name, interesting_lines("\n".join((result.stdout or "", result.stderr or ""))))
    log_display = relative_log_path(repo_root, result.log_path)
    print(f"{result.task.name} failed")
    if result.task.name in HIGH_VOLUME_LINTS:
        preview = preview_lines_for_task(result.task.name, lines)[:2]
        for line in preview:
            print(f"  {line}")
        count = extract_count(lines)
        if count is not None:
            print(f"  [{count}] errors in {log_display}")
        else:
            print(f"  full log: {log_display}")
        return

    preview = failure_preview(result.task.name, lines)
    for line in preview:
        print(f"  {line}")
    print(f"  full log: {log_display}")


def run_parallel(tasks: list[LintTask], repo_root: Path) -> list[LintResult]:
    with ThreadPoolExecutor(max_workers=len(tasks)) as executor:
        futures = [executor.submit(run_task, task, repo_root) for task in tasks]
        return [future.result() for future in futures]


def partition_python_tasks(tasks: list[LintTask]) -> tuple[list[LintTask], list[LintTask]]:
    """Keep repository-tool tests out of the concurrent lint batch."""
    return (
        [task for task in tasks if task.name != "pytests"],
        [task for task in tasks if task.name == "pytests"],
    )


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
    python_tasks = [task for task in selected_tasks if task.name in PYTHON_LINT_ORDER or task.name in EXTRA_LINTS]
    results: list[LintResult] = []

    for task in cargo_tasks:
        result = run_task(task, repo_root)
        print_result(result, repo_root)
        results.append(result)

    if python_tasks:
        # The Python suite invokes repository tools and may create their normal
        # output paths.  Running it beside the lint tasks that inspect those
        # paths makes the overall lint gate race-dependent on CI.
        parallel_python_tasks, serial_python_tasks = partition_python_tasks(python_tasks)
        if parallel_python_tasks:
            for result in run_parallel(parallel_python_tasks, repo_root):
                print_result(result, repo_root)
                results.append(result)
        for task in serial_python_tasks:
            result = run_task(task, repo_root)
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
