#!/usr/bin/env python3
"""atm-nudge.py <recipient>

Post-send hook for ATM: nudges a named agent's tmux pane when a message is
delivered to them.

Pane resolution order:
  1. .atm.toml [[rmux.windows.panes]] tmux_pane_id field (preferred)
  2. ~/.claude/teams/<team>/config.json tmuxPaneId field

CLAUDE_PROJECT_DIR env var is used to locate .atm.toml; falls back to PWD then
os.getcwd() so hooks fired from worktree dirs still find the config.

Usage (from [[atm.post_send_hooks]] in .atm.toml):
  command = ["scripts/atm-nudge.py", "team-lead"]
  command = ["scripts/atm-nudge.py", "arch-ctm"]
"""
from __future__ import annotations

import os
import json
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ModuleNotFoundError:
        tomllib = None  # type: ignore[assignment]


LOG_FILE = "/tmp/atm-nudge.log"


def log(message: str) -> None:
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    with open(LOG_FILE, "a") as f:
        f.write(f"{timestamp} {message}\n")


def candidate_start_dirs() -> list[Path]:
    candidates: list[Path] = []
    seen: set[Path] = set()
    for raw in (
        os.environ.get("CLAUDE_PROJECT_DIR", "").strip(),
        os.environ.get("PWD", "").strip(),
        os.getcwd(),
    ):
        if not raw:
            continue
        try:
            path = Path(raw).expanduser().resolve()
        except Exception:
            continue
        if path not in seen:
            seen.add(path)
            candidates.append(path)
    return candidates


def find_atm_toml(start_dir: Path) -> Path | None:
    current = start_dir.resolve()
    while True:
        candidate = current / ".atm.toml"
        if candidate.is_file():
            return candidate
        parent = current.parent
        if parent == current:
            return None
        current = parent


def read_post_send_payload() -> dict[str, object]:
    raw = os.environ.get("ATM_POST_SEND", "").strip()
    if not raw:
        return {}
    try:
        payload = json.loads(raw)
    except Exception:
        return {}
    return payload if isinstance(payload, dict) else {}


def resolve_team() -> str:
    payload = read_post_send_payload()
    payload_team = payload.get("team")
    if isinstance(payload_team, str) and payload_team.strip():
        return payload_team.strip()
    if tomllib is not None:
        for start_dir in candidate_start_dirs():
            toml_path = find_atm_toml(start_dir)
            if toml_path is None:
                continue
            try:
                with toml_path.open("rb") as f:
                    config = tomllib.load(f)
                for section in ("atm", "core"):
                    team = config.get(section, {}).get("default_team")
                    if team:
                        return str(team)
            except Exception:
                continue
    return os.environ.get("ATM_TEAM", "atm-dev")


def find_pane_via_toml(recipient: str) -> str | None:
    """Read tmux_pane_id from .atm.toml [[rmux.windows.panes]] entries."""
    if tomllib is None:
        return None
    for start_dir in candidate_start_dirs():
        toml_path = find_atm_toml(start_dir)
        if toml_path is None:
            continue
        try:
            with toml_path.open("rb") as f:
                config = tomllib.load(f)
        except Exception:
            continue
        for window in config.get("rmux", {}).get("windows", []):
            for pane in window.get("panes", []):
                if pane.get("name") == recipient:
                    pane_id = pane.get("tmux_pane_id", "").strip()
                    if pane_id:
                        return pane_id
    return None


def find_pane_via_config(recipient: str, team: str) -> tuple[str | None, str | None]:
    """Return (pane_id, error_msg). Looks up tmuxPaneId in team config.json."""
    config_path = Path.home() / ".claude" / "teams" / team / "config.json"
    if not config_path.exists():
        return None, f"No config.json for team '{team}'"
    try:
        config = json.loads(config_path.read_text())
    except Exception as exc:
        return None, f"Cannot parse {config_path}: {exc}"
    member = next(
        (m for m in config.get("members", []) if m.get("name") == recipient), None
    )
    if member is None:
        return None, f"Agent '{recipient}' not in team '{team}'"
    pane_id = member.get("tmuxPaneId", "").strip()
    if not pane_id:
        return None, f"No tmuxPaneId for '{recipient}' in team '{team}'"
    return pane_id, None


def nudge_pane(pane_id: str, message: str, recipient: str) -> None:
    subprocess.run(["tmux", "send-keys", "-t", pane_id, "-l", message], check=True)
    time.sleep(0.25)
    subprocess.run(["tmux", "send-keys", "-t", pane_id, "Enter"], check=True)
    log(f"nudged recipient={recipient} pane={pane_id}")


def main(argv: list[str]) -> int:
    if len(argv) < 2 or not argv[1].strip():
        print("usage: atm-nudge.py <recipient>", file=sys.stderr)
        return 1

    recipient = argv[1].strip()
    team = resolve_team()
    message = (
        f"<atm><action>read atm --team {team}</action>"
        f"<action>ack the message</action>"
        f"<action>execute the assigned task</action>"
        f'<when idle="immediate" busy="after-current-task"/>'
        f'<console announce="concise" pause="false"/></atm>'
    )

    # 1. .atm.toml rmux panes (preferred — zero extra subprocess calls)
    pane_id = find_pane_via_toml(recipient)

    # 2. config.json fallback
    if pane_id is None:
        warn = f"warn: {recipient} not in .atm.toml rmux panes, trying config.json"
        log(warn)
        print(warn, file=sys.stderr)
        pane_id, config_error = find_pane_via_config(recipient, team)
        if pane_id is None:
            error_msg = f"Cannot nudge {recipient}@{team}: {config_error}"
            log(f"error: {error_msg}")
            print(error_msg, file=sys.stderr)
            return 1

    nudge_pane(pane_id, message, recipient)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
