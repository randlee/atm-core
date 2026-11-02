"""The console's PhaseMonitor: `cli.py` calls `route` once, then only these methods.

TodayMonitor runs today's console code; PolicyMonitor runs collect -> decide -> execute for a policy profile.
Lands as `atm_bd_console/monitor.py`; the console types below are today's, unchanged.
"""
from __future__ import annotations

import enum
from dataclasses import dataclass
from typing import Literal, Protocol

from atm_bd_console.closed_cache import ClosedBeads
from atm_bd_console.state import StateStore
from atm_bd_console.step_result import StepResult
from atm_bd_console.types import Config, State


class Exit(enum.IntEnum):
    """Process exit codes the console already uses."""

    OK = 0
    UNAVAILABLE = 2  # no snapshot / console not running / tick stopped
    CONFIG = 3  # phase file, policy or cache unusable; nothing was read or sent


@dataclass(frozen=True)
class MonitorArgs:
    """The parsed subcommand options, the same for every profile."""

    command: Literal["queues", "snapshot", "cron", "console", "show"]
    phase: str | None = None
    root: str | None = None
    all: bool = False
    stack: bool = False
    json: bool = False
    min_age: int = 10
    state_file: str | None = None
    md_table: bool = False
    github: Literal["graphql", "rest", "off"] | None = None
    interval: int | None = None
    github_poll_interval: int | None = None
    github_quota_threshold: int | None = None
    dry_run: bool = False
    no_fetch: bool = False


class PhaseMonitor(Protocol):
    def queues(self, args: MonitorArgs) -> Exit:
        """Decide without executing; print the queue rows (`--json`: non-quiet rows as one object)."""
        ...

    def snapshot(self, args: MonitorArgs) -> Exit:
        """Decide without executing, under the console lock; write snapshot.json."""
        ...

    def step(self, config: Config, state: State, force_github: bool, store: StateStore,
             cache: ClosedBeads | None = None) -> StepResult:
        """One TUI tick with `runner.step`'s signature; never executes actions."""
        ...

    def cron(self, args: MonitorArgs) -> Exit:
        """Decide and execute (`--dry-run`: record only), under the console lock; one line per outcome."""
        ...
