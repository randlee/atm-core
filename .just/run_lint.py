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


if sys.platform == "win32":
    # Cross-host diagnostics may contain Unicode that the legacy Windows
    # console code page cannot encode.
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


PYTHON_LINT_ORDER = (
    "version",
    "boundaries",
    "adr-index",
    "unix-gating",
    "same-host-portability",
    "runtime-waits",
    "manifests",
    "daemon-signing-coupling",
    "silent-emit",
    "function-length",
    "legacy-mailbox-paths",
    "nudge-taxonomy",
    "capability-degradation",
    "identities",
    "env-var-boundary",
    "runtime-observation-boundary",
    "read-concurrency-gates",
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
CARGO_LINT_ORDER = ("fmt", "clippy", "deny", "shear", "arch-gates")
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
    "nudge-taxonomy",
    "capability-degradation",
    "spell",
    "hermes-adapter",
    "pytests",
)
HIGH_VOLUME_LINTS = {"identities", "lines"}
CRATE_INVENTORY_LINTS = {"fmt", "clippy", "modules", "boundaries", "sc-boundary", "sc-portability", "manifests", "arch-gates"}
COUNT_PATTERNS = (
    ("total violations:", "violations"),
    ("errors:", "errors"),
)
FILE_FINDING_RE = re.compile(r"^[A-Za-z0-9_.-]+/.*:\d+:")
ANSI_ESCAPE_RE = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
ERROR_MARKER_RE = re.compile(r"(?<![A-Za-z0-9_])(error|failed|traceback|exception)(?![A-Za-z0-9_])")
PYTEST_BLOCK_HEADER_RE = re.compile(r"^(ERROR|FAIL): ")
PYTEST_BLOCK_SEPARATOR_RE = re.compile(r"^=+$")
PYTEST_BLOCK_RULE_RE = re.compile(r"^-+$")
PYTEST_PREVIEW_BLOCK_LINES = 15
PYTEST_PREVIEW_MAX_LINES = 400


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
        "arch-gates": LintTask("arch-gates", ["cargo", "test", "-p", "atm-architecture", "--quiet"]),
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
        "read-concurrency-gates": LintTask(
            "read-concurrency-gates",
            [*python_command, str(repo_root / ".just/check_read_concurrency_gates.py")],
        ),
        "lines": LintTask("lines", [*python_command, str(repo_root / ".just/check_line_counts.py")]),
        "boundaries": LintTask("boundaries", [*python_command, str(repo_root / ".just/lint_boundaries.py")]),
        "adr-index": LintTask("adr-index", [*python_command, str(repo_root / ".just/check_adr_index.py")]),
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
        "nudge-taxonomy": LintTask(
            "nudge-taxonomy",
            [*python_command, str(repo_root / "scripts/check-nudge-taxonomy.py")],
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
        return [*CARGO_LINT_ORDER, *PYTHON_LINT_ORDER, *EXTRA_LINTS]
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


def extract_error_fail_preview(
    lines: list[str],
    *,
    block_lines: int = PYTEST_PREVIEW_BLOCK_LINES,
    max_total_lines: int = PYTEST_PREVIEW_MAX_LINES,
) -> list[str]:
    """Return every ``unittest`` ``ERROR:``/``FAIL:`` test id with a bounded traceback head.

    ``unittest.TextTestRunner`` prints one ``ERROR:``/``FAIL:`` header line
    per failing test id, each followed by its traceback, with every
    ``ERROR`` block printed before any ``FAIL`` block. A tail-only preview
    (``lines[-40:]``) only ever shows whichever block happens to land at the
    end of the run -- on a suite with more errors than fit in that window,
    every ``ERROR:`` id (and some ``FAIL:`` ids) silently disappear from the
    CI console. This walks every block instead, keeping each test id line
    plus up to ``block_lines`` lines of its traceback, and caps the combined
    preview at ``max_total_lines`` so a large failure count still prints a
    bounded summary rather than a full traceback dump.
    """
    blocks: list[list[str]] = []
    index = 0
    total = len(lines)
    while index < total:
        line = lines[index]
        if PYTEST_BLOCK_HEADER_RE.match(line):
            block = [line]
            cursor = index + 1
            # The dash rule immediately under the id line belongs to this
            # block (it's the header's own underline). A *second* dash rule
            # is unittest's run-summary underline ("Ran N tests..."), which
            # must end the block rather than being swept into it.
            seen_header_rule = False
            while cursor < total and len(block) < block_lines:
                next_line = lines[cursor]
                if PYTEST_BLOCK_HEADER_RE.match(next_line) or PYTEST_BLOCK_SEPARATOR_RE.match(next_line):
                    break
                if PYTEST_BLOCK_RULE_RE.match(next_line):
                    if seen_header_rule:
                        break
                    seen_header_rule = True
                block.append(next_line)
                cursor += 1
            blocks.append(block)
            index = cursor
        else:
            index += 1

    if not blocks:
        # No parseable ERROR:/FAIL: blocks (e.g. the suite crashed before any
        # test ran) -- fall back to the plain tail so the console still shows
        # the most recent output rather than nothing at all.
        return lines[-40:]

    preview: list[str] = []
    for block in blocks:
        if len(preview) + len(block) > max_total_lines:
            remaining = max_total_lines - len(preview)
            if remaining > 0:
                preview.extend(block[:remaining])
            preview.append(f"... preview truncated at {max_total_lines} lines; see full log")
            break
        preview.extend(block)
    return preview


def failure_preview(task_name: str, lines: list[str]) -> list[str]:
    """Return actionable CI output without hiding a Python test traceback."""
    if task_name == "pytests":
        return extract_error_fail_preview(lines)
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


def console_safe_text(text: str, encoding: str | None) -> str:
    """Return text that can be written to a console with *encoding*.

    Subprocess output is deliberately decoded as UTF-8 with replacement so
    diagnostic logs are portable.  Windows CI's default console encoding can
    still be a legacy code page, though, and cannot render every replacement
    character.  Preserve the diagnostic by escaping only characters that the
    console cannot encode instead of letting the lint reporter crash.
    """
    selected_encoding = encoding or "utf-8"
    return text.encode(selected_encoding, errors="backslashreplace").decode(selected_encoding)


def print_console_line(text: str, output: object | None = None) -> None:
    """Print a diagnostic line without assuming a UTF-8 terminal."""
    stream = sys.stdout if output is None else output
    encoding = getattr(stream, "encoding", None)
    print(console_safe_text(text, encoding), file=stream)


def print_result(result: LintResult, repo_root: Path) -> None:
    if result.returncode == 0:
        print_console_line(f"{result.task.name} passed [{format_duration(result.duration_seconds)}]")
        return

    lines = preview_lines_for_task(result.task.name, interesting_lines("\n".join((result.stdout or "", result.stderr or ""))))
    log_display = relative_log_path(repo_root, result.log_path)
    print_console_line(f"{result.task.name} failed")
    if result.task.name in HIGH_VOLUME_LINTS:
        preview = preview_lines_for_task(result.task.name, lines)[:2]
        for line in preview:
            print_console_line(f"  {line}")
        count = extract_count(lines)
        if count is not None:
            print_console_line(f"  [{count}] errors in {log_display}")
        else:
            print_console_line(f"  full log: {log_display}")
        return

    preview = failure_preview(result.task.name, lines)
    for line in preview:
        print_console_line(f"  {line}")
    print_console_line(f"  full log: {log_display}")


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
        print_console_line(str(error), sys.stderr)
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
        print_console_line(f"lint failed: {len(failures)} check(s) failed")
        return 1

    print_console_line(f"lint passed: {len(results)} check(s) succeeded")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
