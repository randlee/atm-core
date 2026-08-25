#!/usr/bin/env python3
"""Run, publish, and push the official dedicated-account benchmark matrix.

The command deliberately owns only the benchmark checkout and public evidence.
It does not select, restart, or otherwise mutate an ambient daemon; the matrix
runner creates its disposable account-scoped daemon and ATM home itself.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Callable, Mapping, Sequence, TextIO

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke.benchmark_account import BenchmarkAccountError, clear_benchmark_database_state

CAPACITY_RUNNER = ROOT / "scripts/smoke/run_admission_capacity.py"
DEFAULT_ACCOUNT = "atmbench"
HOST_LABEL = "m5-atmbench"
DEPLOY_KEY_ENVIRONMENT_VARIABLE = "ATM_BENCHMARK_DEPLOY_KEY"
LOG_DIRECTORY = Path.home() / "benchmark-logs"
RELEASE_BINARIES = (ROOT / "target/release/atm", ROOT / "target/release/atm-daemon")

Run = Callable[..., subprocess.CompletedProcess[str]]
Write = Callable[[str], None]


class OfficialBenchmarkError(RuntimeError):
    """An operational error that makes a campaign unofficial (exit 2)."""


@dataclass(frozen=True)
class OfficialOutcome:
    """Result classification retained independently from post-run operations."""

    measured_failure: bool
    detail: str


class OfficialBenchmark:
    """A command-injectable official-run orchestrator, intentionally small."""

    def __init__(
        self,
        root: Path = ROOT,
        *,
        run: Run = subprocess.run,
        environ: Mapping[str, str] | None = None,
        write: Write = print,
        branch_override: str | None = None,
    ) -> None:
        self.root = root
        self.run = run
        self.environ = dict(os.environ if environ is None else environ)
        self.write = write
        self.branch_override = branch_override
        self.account_verified = False

    def execute(self) -> int:
        """Run the complete contract, preserving a measured FAIL as exit 1."""
        outcome: OfficialOutcome | None = None
        infrastructure_error: str | None = None
        try:
            branch = self.preflight()
            self.build_and_sign()
            outcome = self.measure()
            # The matrix has finished using its disposable daemon and database
            # before evidence is committed or pushed. This keeps the account
            # clear while the network-only publication step runs.
            self.reset_disposable_account()
            post_run_error = self.render_publish_and_push(branch)
            if post_run_error is not None:
                raise OfficialBenchmarkError(post_run_error)
        except OfficialBenchmarkError as error:
            infrastructure_error = str(error)
        finally:
            if self.account_verified:
                try:
                    self.reset_disposable_account()
                except OfficialBenchmarkError as error:
                    if infrastructure_error is None:
                        infrastructure_error = str(error)
                    else:
                        self.emit(f"benchmark-official: cleanup error: {error}")
        return self.finalize(outcome, infrastructure_error)

    def finalize(self, outcome: OfficialOutcome | None, infrastructure_error: str | None) -> int:
        """Apply verdict precedence exactly once after every control-flow path."""
        if outcome is not None and outcome.measured_failure:
            if infrastructure_error is not None:
                self.emit(f"benchmark-official: retained measured FAIL despite infrastructure error: {infrastructure_error}")
            self.notify_team_lead(outcome.detail)
            return 1
        if infrastructure_error is not None:
            self.emit(f"benchmark-official: infrastructure error: {infrastructure_error}")
            return 2
        return 0

    def preflight(self) -> str:
        """Prove account and checkout readiness before isolated measurement."""
        expected_account = self.environ.get("ATM_OFFICIAL_ACCOUNT", DEFAULT_ACCOUNT)
        account = self.command(["whoami"]).stdout.strip()
        if account != expected_account:
            raise OfficialBenchmarkError(
                f"official runs require account {expected_account!r}; current account is {account!r}"
            )
        self.account_verified = True
        self.reset_disposable_account()
        self.require_clean_tree("before sync")

        branch = self.branch_override or self.current_branch()
        if self.branch_override is None:
            self.require_integrate_branch(branch)
        else:
            self.require_valid_override(branch)
        self.command(["git", "fetch", "origin"])
        self.push_stranded_commit_if_needed(branch)
        self.command(["git", "reset", "--hard", f"origin/{branch}"])
        self.require_clean_tree("after sync")
        return branch

    def reset_disposable_account(self) -> None:
        """Kill only this account's daemon, then remove all disposable DB state."""
        self.stop_account_daemon()
        try:
            clear_benchmark_database_state()
        except BenchmarkAccountError as error:
            raise OfficialBenchmarkError(f"benchmark-account database cleanup failed: {error}") from error

    def stop_account_daemon(self) -> None:
        """Ensure the disposable account begins and ends with no atm-daemon."""
        pids = self.account_daemon_pids()
        for pid in pids:
            terminated = self.command(["kill", "-TERM", pid], allow_failure=True)
            if terminated.returncode:
                raise OfficialBenchmarkError(self.detail("benchmark-account daemon termination", terminated))
        remaining = self.account_daemon_pids()
        for pid in remaining:
            forced = self.command(["kill", "-KILL", pid], allow_failure=True)
            if forced.returncode:
                raise OfficialBenchmarkError(self.detail("benchmark-account daemon forced termination", forced))
        remaining = self.account_daemon_pids()
        if remaining:
            raise OfficialBenchmarkError(
                "benchmark-account daemon remained running after termination: " + ", ".join(remaining)
            )

    def account_daemon_pids(self) -> tuple[str, ...]:
        """Return only matching daemons owned by this OS account, never peers'."""
        observed = self.command(["pgrep", "-x", "atm-daemon"], allow_failure=True)
        if observed.returncode == 1:
            return ()
        if observed.returncode != 0:
            raise OfficialBenchmarkError(self.detail("benchmark-account daemon inspection", observed))
        owned: list[str] = []
        for pid in observed.stdout.splitlines():
            if not pid.isdecimal():
                raise OfficialBenchmarkError(f"benchmark-account daemon inspection returned invalid PID: {pid!r}")
            owner = self.command(["ps", "-o", "uid=", "-p", pid], allow_failure=True)
            if owner.returncode:
                raise OfficialBenchmarkError(self.detail(f"benchmark-account daemon owner {pid}", owner))
            if owner.stdout.strip().isdecimal() and int(owner.stdout.strip()) == os.geteuid():
                owned.append(pid)
        return tuple(owned)

    def build_and_sign(self) -> None:
        """Measure only fresh, signed release binaries from the synced commit."""
        self.command(["cargo", "build", "--release", "-p", "agent-team-mail", "-p", "atm-daemon"])
        self.command([sys.executable, str(self.root / ".just/sign_daemon_dev.py")])
        missing = [str(path) for path in self.release_binaries() if not path.is_file()]
        if missing:
            raise OfficialBenchmarkError(
                "fresh build did not produce required signed release binaries: " + ", ".join(missing)
            )

    def measure(self) -> OfficialOutcome:
        """Run the full matrix directly so the runner's verdict cannot be masked."""
        environment = dict(self.environ)
        environment["ATM_CAPACITY_HOST_LABEL"] = HOST_LABEL
        result = self.command([sys.executable, str(self.root / CAPACITY_RUNNER.relative_to(ROOT))], env=environment, allow_failure=True)
        if result.returncode == 0:
            return OfficialOutcome(False, "all target floors passed")
        if result.returncode != 1:
            raise OfficialBenchmarkError(self.detail("benchmark runner", result))
        summary = self.failure_summary()
        self.emit(f"benchmark-official: measured FAIL: {summary}")
        return OfficialOutcome(True, summary)

    def render_publish_and_push(self, branch: str) -> str | None:
        """Return a post-measurement error without losing a recorded FAIL verdict."""
        for label, command in (
            ("report rebuild", [sys.executable, str(self.root / "scripts/smoke/benchmark_report.py"), "--rebuild"]),
            ("benchmark publication", ["just", "benchmark-publish"]),
        ):
            result = self.command(command, allow_failure=True)
            if result.returncode:
                return self.detail(label, result)
        staged = self.command(["git", "diff", "--cached", "--quiet"], allow_failure=True)
        if staged.returncode == 0:
            return "benchmark publication staged no report artifacts; refusing an evidence-free official result"
        if staged.returncode != 1:
            return self.detail("staged-report inspection", staged)
        revision = self.command(["git", "rev-parse", "--short", "HEAD"]).stdout.strip()
        committed = self.command(
            ["git", "commit", "-m", f"evidence(benchmark): official {HOST_LABEL} {revision}"],
            allow_failure=True,
        )
        if committed.returncode:
            return self.detail("benchmark evidence commit", committed)
        pushed = self.command(["git", "push", "origin", branch], env=self.push_environment(), allow_failure=True)
        if pushed.returncode:
            return self.detail("benchmark evidence push", pushed)
        self.require_clean_tree("after report push")
        return None

    def push_stranded_commit_if_needed(self, branch: str) -> None:
        """Push local evidence before sync; never reset a stranded commit away."""
        ahead = self.command(["git", "rev-list", f"origin/{branch}..HEAD"]).stdout.splitlines()
        if not ahead:
            return
        stranded = ahead[0]
        remote = self.command(["git", "rev-parse", f"origin/{branch}"]).stdout.strip()
        pushed = self.command(["git", "push", "origin", branch], env=self.push_environment(), allow_failure=True)
        if pushed.returncode:
            raise OfficialBenchmarkError(
                f"stranded local commit {stranded} could not be pushed to origin/{branch} ({remote}); "
                f"refusing to reset it away: {self.detail('stranded push', pushed)}"
            )

    def current_branch(self) -> str:
        result = self.command(["git", "branch", "--show-current"])
        branch = result.stdout.strip()
        if not branch:
            raise OfficialBenchmarkError("official run requires a named integrate/phase-* branch, not detached HEAD")
        return branch

    @staticmethod
    def require_integrate_branch(branch: str) -> None:
        if not branch.startswith("integrate/phase-"):
            raise OfficialBenchmarkError(
                f"official run requires an integrate/phase-* branch; current branch is {branch!r}"
            )

    @staticmethod
    def require_valid_override(branch: str) -> None:
        if not branch or branch.startswith("-") or any(character.isspace() for character in branch):
            raise OfficialBenchmarkError(f"invalid official branch override: {branch!r}")

    def require_clean_tree(self, stage: str) -> None:
        dirty = self.command(["git", "status", "--porcelain"]).stdout.strip()
        if dirty:
            raise OfficialBenchmarkError(f"working tree is dirty {stage}; commit or remove local changes first")

    def push_environment(self) -> dict[str, str]:
        deploy_key = self.environ.get(DEPLOY_KEY_ENVIRONMENT_VARIABLE, "").strip()
        if not deploy_key:
            raise OfficialBenchmarkError(
                f"{DEPLOY_KEY_ENVIRONMENT_VARIABLE} is required for unattended Git push authentication"
            )
        return {
            **self.environ,
            "GIT_SSH_COMMAND": f"ssh -i {deploy_key} -o IdentitiesOnly=yes -o BatchMode=yes",
        }

    def notify_team_lead(self, detail: str) -> None:
        """Best-effort only: a notification never changes the measured verdict."""
        message = f"official benchmark FAIL on {HOST_LABEL}: {detail}"
        try:
            sent = self.command(["atm", "send", "team-lead@atm-dev", message], allow_failure=True)
        except OfficialBenchmarkError as error:
            self.emit(f"benchmark-official: team-lead notification unavailable: {error}")
            return
        if sent.returncode:
            self.emit(f"benchmark-official: team-lead notification unavailable: {self.detail('atm send', sent)}")

    def release_binaries(self) -> tuple[Path, ...]:
        return tuple(self.root / path.relative_to(ROOT) for path in RELEASE_BINARIES)

    def failure_summary(self) -> str:
        """Read the freshly retained campaign so exit-1 output names p50 and floor."""
        campaigns = sorted(
            (self.root / "site/reports/send-message-benchmark").glob("*.campaign.json"),
            key=lambda path: path.stat().st_mtime_ns,
        )
        if not campaigns:
            return "runner returned FAIL without a retained campaign artifact"
        try:
            payload = json.loads(campaigns[-1].read_text(encoding="utf-8"))
            failed = [result for result in payload["results"] if result["status"] == "FAIL"]
            return "; ".join(
                f"{result['target']} p50={result['metrics']['admissions_per_second']['p50']:.2f} "
                f"floor={result['baseline']['p50_floor']:.2f}"
                for result in failed
            ) or "campaign marked FAIL"
        except (KeyError, OSError, TypeError, ValueError) as error:
            return f"runner returned FAIL; campaign summary unavailable ({error})"

    def command(
        self,
        command: list[str],
        *,
        env: Mapping[str, str] | None = None,
        allow_failure: bool = False,
    ) -> subprocess.CompletedProcess[str]:
        self.emit("+ " + " ".join(command))
        try:
            result = self.run(
                command,
                cwd=self.root,
                env=env,
                check=False,
                capture_output=True,
                text=True,
            )
        except OSError as error:
            raise OfficialBenchmarkError(f"could not execute {' '.join(command)}: {error}") from error
        for stream in (result.stdout, result.stderr):
            if stream:
                self.emit(stream.rstrip())
        if result.returncode and not allow_failure:
            raise OfficialBenchmarkError(self.detail("command", result))
        return result

    @staticmethod
    def detail(label: str, result: subprocess.CompletedProcess[str]) -> str:
        detail = (result.stderr or result.stdout or "no output").strip()
        return f"{label} failed (exit {result.returncode}): {detail}"

    def emit(self, message: str) -> None:
        self.write(message)


def _log_writer(stream: TextIO, log: TextIO) -> Write:
    def write(message: str) -> None:
        print(message, file=stream)
        print(message, file=log, flush=True)

    return write


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--branch",
        help=(
            "reviewed remote branch to synchronize before a pre-merge validation; "
            "ordinary official runs omit this and require the current integrate/phase-* branch"
        ),
    )
    args = parser.parse_args(argv)
    LOG_DIRECTORY.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    log_path = LOG_DIRECTORY / f"benchmark-official-{timestamp}.log"
    with log_path.open("x", encoding="utf-8") as log:
        runner = OfficialBenchmark(write=_log_writer(sys.stdout, log), branch_override=args.branch)
        runner.emit(f"benchmark-official log: {log_path}")
        return runner.execute()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
