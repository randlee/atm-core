#!/usr/bin/env python3
"""atm-nudge.py <recipient>

Post-send hook for ATM: nudges a named agent's tmux pane when a message is
delivered to them.

Pane resolution: reads BOTH .atm.toml [[rmux.windows.panes]] tmux_pane_id AND
~/.claude/teams/<team>/config.json tmuxPaneId. If they disagree or either is
missing, exits with an error indicating which source has the problem.

CLAUDE_PROJECT_DIR env var is used to locate .atm.toml; falls back to PWD then
os.getcwd() so hooks fired from worktree dirs still find the config.

Usage (from [[atm.post_send_hooks]] in .atm.toml):
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


def read_pane_from_toml(recipient: str) -> tuple[str | None, str | None]:
    """Return (pane_id, error_msg) from .atm.toml [[rmux.windows.panes]]."""
    if tomllib is None:
        return None, "tomllib not available (install tomli for Python < 3.11)"
    for start_dir in candidate_start_dirs():
        toml_path = find_atm_toml(start_dir)
        if toml_path is None:
            continue
        try:
            with toml_path.open("rb") as f:
                config = tomllib.load(f)
        except Exception as exc:
            return None, f"Cannot parse {toml_path}: {exc}"
        for window in config.get("rmux", {}).get("windows", []):
            for pane in window.get("panes", []):
                if pane.get("name") == recipient:
                    pane_id = pane.get("tmux_pane_id", "").strip()
                    if pane_id:
                        return pane_id, None
                    return None, f"'{recipient}' found in .atm.toml but tmux_pane_id is empty"
        return None, f"'{recipient}' not found in .atm.toml [[rmux.windows.panes]]"
    return None, ".atm.toml not found in any parent directory"


def read_pane_from_config(recipient: str, team: str) -> tuple[str | None, str | None]:
    """Return (pane_id, error_msg) from ~/.claude/teams/<team>/config.json."""
    config_path = Path.home() / ".claude" / "teams" / team / "config.json"
    if not config_path.exists():
        return None, f"config.json not found for team '{team}' at {config_path}"
    try:
        config = json.loads(config_path.read_text())
    except Exception as exc:
        return None, f"Cannot parse {config_path}: {exc}"
    member = next(
        (m for m in config.get("members", []) if m.get("name") == recipient), None
    )
    if member is None:
        return None, f"'{recipient}' not in team '{team}' members"
    pane_id = member.get("tmuxPaneId", "").strip()
    if not pane_id:
        return None, f"'{recipient}' in team '{team}' has empty tmuxPaneId"
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

    pane_toml, err_toml = read_pane_from_toml(recipient)
    pane_config, err_config = read_pane_from_config(recipient, team)

    if pane_toml and pane_config:
        if pane_toml != pane_config:
            lines = [
                f"ERROR: Pane mismatch for '{recipient}@{team}':",
                f"  .atm.toml       tmux_pane_id = {pane_toml}",
                f"  config.json     tmuxPaneId   = {pane_config}",
                f"",
                f"Manual nudge (pick the correct pane ID):",
                f"  tmux send-keys -t {pane_toml} -l '{message}' && sleep 0.25 && tmux send-keys -t {pane_toml} Enter",
                f"",
                f"Fix: update tmux_pane_id in .atm.toml [[rmux.windows.panes]] name='{recipient}'",
                f"     and/or tmuxPaneId in ~/.claude/teams/{team}/config.json",
                f"     so both agree on the correct pane.",
            ]
            out = "\n".join(lines)
            log(f"error: pane mismatch for {recipient}@{team} toml={pane_toml} config={pane_config}")
            print(out, file=sys.stderr)
            return 1
        nudge_pane(pane_toml, message, recipient)
        return 0

    if pane_toml and not pane_config:
        lines = [
            f"ERROR: '{recipient}@{team}' pane found in .atm.toml ({pane_toml}) but missing from config.json.",
            f"  config.json error: {err_config}",
            f"",
            f"Manual nudge:",
            f"  tmux send-keys -t {pane_toml} -l '{message}' && sleep 0.25 && tmux send-keys -t {pane_toml} Enter",
            f"",
            f"Fix: add or update tmuxPaneId = \"{pane_toml}\" for '{recipient}'",
            f"     in ~/.claude/teams/{team}/config.json members array.",
        ]
        out = "\n".join(lines)
        log(f"error: {recipient}@{team} toml={pane_toml} config missing: {err_config}")
        print(out, file=sys.stderr)
        return 1

    if pane_config and not pane_toml:
        lines = [
            f"ERROR: '{recipient}@{team}' pane found in config.json ({pane_config}) but missing from .atm.toml.",
            f"  .atm.toml error: {err_toml}",
            f"",
            f"Manual nudge:",
            f"  tmux send-keys -t {pane_config} -l '{message}' && sleep 0.25 && tmux send-keys -t {pane_config} Enter",
            f"",
            f"Fix: add tmux_pane_id = \"{pane_config}\" to [[rmux.windows.panes]] name='{recipient}'",
            f"     in .atm.toml.",
        ]
        out = "\n".join(lines)
        log(f"error: {recipient}@{team} config={pane_config} toml missing: {err_toml}")
        print(out, file=sys.stderr)
        return 1

    lines = [
        f"ERROR: Pane not configured for '{recipient}@{team}' in either source.",
        f"  .atm.toml   error: {err_toml}",
        f"  config.json error: {err_config}",
        f"",
        f"Manual nudge (once you know the correct pane ID):",
        f"  tmux send-keys -t <PANE_ID> -l '{message}' && sleep 0.25 && tmux send-keys -t <PANE_ID> Enter",
        f"",
        f"Fix: add tmux_pane_id = \"<PANE_ID>\" to [[rmux.windows.panes]] name='{recipient}' in .atm.toml",
        f"     and tmuxPaneId = \"<PANE_ID>\" for '{recipient}' in ~/.claude/teams/{team}/config.json.",
    ]
    out = "\n".join(lines)
    log(f"error: pane not found for {recipient}@{team}: toml={err_toml} config={err_config}")
    print(out, file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
