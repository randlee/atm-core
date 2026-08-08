#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import json
import os
import platform
import re
import shutil
import subprocess
import tempfile


@dataclass(frozen=True)
class SmokePaths:
    repo_root: Path
    reports_root: Path
    report_dir: Path
    markdown: Path
    json: Path


@dataclass(frozen=True)
class SmokeFixture:
    root: Path
    workspace_dir: Path
    home_dir: Path
    atm_home: Path
    log_dir: Path
    team_dir: Path
    team_name: str
    operator: str
    recipient: str


@dataclass(frozen=True)
class SharedHostFixturePair:
    root: Path
    workspace_a: SmokeFixture
    workspace_b: SmokeFixture


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def timestamp_slug(now: datetime | None = None) -> str:
    moment = now or datetime.now(timezone.utc)
    return moment.strftime("%Y%m%dT%H%M%S%fZ")


def report_segment(value: str, label: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", value):
        raise ValueError(f"{label} must contain only letters, numbers, '.', '_', or '-'")
    return value


def operating_system_label() -> str:
    return {"darwin": "macos"}.get(platform.system().lower(), platform.system().lower())


def level_slug(level: str) -> str:
    normalized = level.strip().lower()
    if normalized not in {"fast", "normal", "thorough"}:
        raise ValueError(f"unsupported smoke level: {level}")
    return "smoke" if normalized == "normal" else f"smoke-{normalized}"


def smoke_paths(level: str, now: datetime | None = None) -> SmokePaths:
    root = repo_root()
    reports_root = root / "site" / "reports" / "smoke"
    slug = level_slug(level)
    platform_label = report_segment(operating_system_label(), "local platform")
    host_label = report_segment(platform.node(), "local host name")
    requested_run_id = os.environ.get("ATM_SMOKE_RUN_ID", "").strip()
    run_id = report_segment(requested_run_id or timestamp_slug(now), "ATM_SMOKE_RUN_ID")
    report_dir = reports_root / platform_label / host_label / f"{run_id}-pid{os.getpid()}-{slug}"
    return SmokePaths(
        repo_root=root,
        reports_root=reports_root,
        report_dir=report_dir,
        markdown=report_dir / f"{slug}.md",
        json=report_dir / f"{slug}.json",
    )


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def current_binary_sha(root: Path | None = None) -> str:
    working_root = root or repo_root()
    result = subprocess.run(
        ["git", "-C", str(working_root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def write_json(path: Path, payload: object) -> None:
    write_text_atomic(path, json.dumps(payload, indent=2) + "\n")


def write_text_atomic(path: Path, text: str) -> None:
    ensure_parent(path)
    temp_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            "w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as handle:
            handle.write(text)
            temp_path = Path(handle.name)
        os.replace(temp_path, path)
    finally:
        if temp_path is not None:
            temp_path.unlink(missing_ok=True)


def create_clean_room_fixture(
    *,
    prefix: str,
    team_name: str,
    operator: str,
    recipient: str,
) -> SmokeFixture:
    root = Path(tempfile.mkdtemp(prefix=prefix))
    workspace_dir = root / "w"
    home_dir = root / "h"
    atm_home = root / "a"
    log_dir = root / "logs"
    team_dir = atm_home / ".claude" / "teams" / team_name
    (team_dir / "inboxes").mkdir(parents=True, exist_ok=True)
    workspace_dir.mkdir(parents=True, exist_ok=True)
    (workspace_dir / ".atm.toml").write_text(
        f'[atm]\ndefault_team = "{team_name}"\n',
        encoding="utf-8",
    )
    (team_dir / "config.json").write_text('{"members":[]}\n', encoding="utf-8")
    return SmokeFixture(
        root=root,
        workspace_dir=workspace_dir,
        home_dir=home_dir,
        atm_home=atm_home,
        log_dir=log_dir,
        team_dir=team_dir,
        team_name=team_name,
        operator=operator,
        recipient=recipient,
    )


def clone_fixture(source: SmokeFixture, *, prefix: str, clear_logs: bool = True) -> SmokeFixture:
    root = Path(tempfile.mkdtemp(prefix=prefix))
    workspace_dir = root / "w"
    home_dir = root / "h"
    atm_home = root / "a"
    log_dir = root / "logs"

    ignore_runtime_sockets = shutil.ignore_patterns("*.sock")
    shutil.copytree(source.workspace_dir, workspace_dir, dirs_exist_ok=True)
    shutil.copytree(source.home_dir, home_dir, dirs_exist_ok=True, ignore=ignore_runtime_sockets)
    shutil.copytree(source.atm_home, atm_home, dirs_exist_ok=True, ignore=ignore_runtime_sockets)

    if clear_logs:
        shutil.rmtree(log_dir, ignore_errors=True)
    else:
        shutil.copytree(source.log_dir, log_dir, dirs_exist_ok=True)

    team_dir = atm_home / ".claude" / "teams" / source.team_name
    return SmokeFixture(
        root=root,
        workspace_dir=workspace_dir,
        home_dir=home_dir,
        atm_home=atm_home,
        log_dir=log_dir,
        team_dir=team_dir,
        team_name=source.team_name,
        operator=source.operator,
        recipient=source.recipient,
    )


def create_shared_host_fixture_pair(
    *,
    prefix: str,
    team_name_a: str,
    team_name_b: str,
    operator_a: str,
    operator_b: str,
    recipient_a: str,
    recipient_b: str,
) -> SharedHostFixturePair:
    root = Path(tempfile.mkdtemp(prefix=prefix))
    home_dir = root / "h"
    atm_home = root / "a"
    log_dir = root / "logs"

    def create_workspace(
        slug: str,
        team_name: str,
        operator: str,
        recipient: str,
    ) -> SmokeFixture:
        workspace_dir = root / slug / "w"
        team_dir = atm_home / ".claude" / "teams" / team_name
        (team_dir / "inboxes").mkdir(parents=True, exist_ok=True)
        workspace_dir.mkdir(parents=True, exist_ok=True)
        (workspace_dir / ".atm.toml").write_text(
            f'[atm]\ndefault_team = "{team_name}"\n',
            encoding="utf-8",
        )
        (team_dir / "config.json").write_text('{"members":[]}\n', encoding="utf-8")
        return SmokeFixture(
            root=root / slug,
            workspace_dir=workspace_dir,
            home_dir=home_dir,
            atm_home=atm_home,
            log_dir=log_dir,
            team_dir=team_dir,
            team_name=team_name,
            operator=operator,
            recipient=recipient,
        )

    return SharedHostFixturePair(
        root=root,
        workspace_a=create_workspace("atm-a", team_name_a, operator_a, recipient_a),
        workspace_b=create_workspace("atm-b", team_name_b, operator_b, recipient_b),
    )


def smoke_env(fixture: SmokeFixture, *, identity: str, root: Path | None = None) -> dict[str, str]:
    working_root = root or repo_root()
    daemon_name = "atm-daemon.exe" if os.name == "nt" else "atm-daemon"
    temp_root = fixture.root / "tmp"
    temp_root.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.update(
        {
            "HOME": str(fixture.home_dir),
            "ATM_HOME": str(fixture.atm_home),
            "ATM_TEAM": fixture.team_name,
            "ATM_IDENTITY": identity,
            "ATM_LOG": "debug",
            "ATM_LOG_DIR": str(fixture.log_dir),
            "ATM_DAEMON_BIN": str(working_root / "target" / "release" / daemon_name),
            "TMPDIR": str(temp_root),
            "TMP": str(temp_root),
            "TEMP": str(temp_root),
        }
    )
    env["ATM_CONFIG_HOME"] = str(fixture.atm_home)
    env.pop("ATM_TEAMS_DIR", None)
    return env
