#!/usr/bin/env python3
"""Fail closed on daemon-singleton bypasses before they reach CI runners."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import re
import sys


JUST_DIR = Path(__file__).resolve().parents[1] / ".just"
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_common import build_report, discover_repo_root, monotonic_now, print_report


LINT_NAME = "daemon-singleton"
CONFIG_PATH = Path("scripts/lint_daemon_singleton.toml")
KNOWN_TEST_ENV_NAMES = frozenset({
    "ATM_TEST_FAIL_RESTORE_INBOX_STAGE",
    "ATM_TEST_FAIL_RESTORE_MARKER_REMOVE",
    "ATM_TEST_FAIL_TEAM_CONFIG_WRITE",
    "ATM_TEST_FAKE_TRANSPORT_INJECTION_FAILED",
    "ATM_TEST_FORCE_LOCK_NON_CONTENTION_ERROR",
    "ATM_TEST_FORCE_SOURCE_DISCOVERY_FAULT",
    "ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS",
    "ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD",
    "ATM_TEST_SQLITE_RUNTIME_PATH",
    "ATM_TEST_TMUX_BIN",
    "ATM_TEST_TMUX_LOG",
})
TEST_ENV_RE = re.compile(r"\bATM_(?:TEST_[A-Z0-9_]*|[A-Z0-9_]*_TEST_[A-Z0-9_]*)\b")
RUNTIME_OVERRIDE_RE = re.compile(r"ATM_TEST_RUNTIME_HOME|test_runtime_home")
DAEMON_LAUNCH_RE = re.compile(r"(?:Popen|Command::new|\.spawn\(|os\.system|shell=True)")
ENDPOINT_OVERRIDE_RE = re.compile(r"--direct-peer-port")


@dataclass(frozen=True)
class Violation:
    path: str
    line: int
    category: str
    detail: str

    def render(self) -> str:
        return f"{self.path}:{self.line}: [{self.category}] {self.detail}"


def line_number(source: str, offset: int) -> int:
    return source.count("\n", 0, offset) + 1


def sources(root: Path) -> list[Path]:
    return sorted(
        path for base in (root / "crates", root / "scripts", root / ".just")
        if base.exists()
        for path in base.rglob("*")
        if path.is_file() and path.suffix in {".py", ".rs", ".sh", ".ps1"}
    )


def is_gated_launcher(source: str) -> bool:
    """Recognize only an executable clean-host or normal-client gate."""
    return (
        "DaemonSupervisor" in source
        or "require_clean_host" in source
        or "ambient_daemon_pids" in source
        or "require_clean_host_daemon_state" in source
    )


def is_daemon_launch(source: str) -> bool:
    for match in DAEMON_LAUNCH_RE.finditer(source):
        window = source[match.start() : match.end() + 240]
        if "atm-daemon" in window:
            return True
    return "def start_daemon" in source or "class OwnedDaemon" in source


def collect_violations(root: Path) -> list[Violation]:
    violations: list[Violation] = []
    config = root / CONFIG_PATH
    if config.exists() and "[[daemon_singleton.allow]]" in config.read_text(encoding="utf-8"):
        violations.append(Violation(CONFIG_PATH.as_posix(), 1, "allowlist", "allow-list entries are forbidden"))

    for path in sources(root):
        source = path.read_text(encoding="utf-8")
        relative = path.relative_to(root).as_posix()
        if relative not in {"scripts/lint_daemon_singleton.py", ".just/tests/test_lint_daemon_singleton.py"}:
            for match in ENDPOINT_OVERRIDE_RE.finditer(source):
                violations.append(Violation(relative, line_number(source, match.start()), "endpoint-override", "flag or environment changes the fixed direct-peer endpoint"))
        if relative.startswith("crates/"):
            for match in TEST_ENV_RE.finditer(source):
                name = match.group(0)
                if name not in KNOWN_TEST_ENV_NAMES:
                    violations.append(Violation(relative, line_number(source, match.start()), "new-test-env", f"unapproved test environment variable {name}"))
            for match in RUNTIME_OVERRIDE_RE.finditer(source):
                violations.append(Violation(relative, line_number(source, match.start()), "runtime-scope-override", "test-only runtime scope override"))
        if not is_daemon_launch(source):
            continue
        if is_gated_launcher(source):
            continue
        match = DAEMON_LAUNCH_RE.search(source)
        assert match is not None
        violations.append(Violation(relative, line_number(source, match.start()), "ungated-daemon-launch", "daemon launcher does not prove the production clean-host gate"))
    return sorted(violations, key=lambda item: (item.path, item.line, item.category))


def run(root: Path) -> int:
    started_at = datetime.now(timezone.utc)
    started = monotonic_now()
    violations = collect_violations(root)
    findings = [violation.render() for violation in violations]
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=root,
        passed=not violations,
        summary=("daemon singleton policy satisfied (0 allow-list entries)" if not violations else f"daemon singleton policy violated ({len(violations)} finding(s))"),
        findings=findings,
        transcript_lines=["allow entries: 0 (required)", "", "violations:", *(findings or ["no daemon-singleton violations found"])],
        started_at=started_at,
        duration_seconds=monotonic_now() - started,
    )
    print_report(report, repo_root=root, preview_limit=4, direct_threshold=4)
    return 0 if report.passed else 1


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Reject daemon singleton bypasses.")
    parser.add_argument("--root", help="repository root to inspect")
    args = parser.parse_args(argv[1:])
    return run(discover_repo_root(args.root))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
