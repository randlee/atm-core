#!/usr/bin/env python3
"""atm-nudge.py [--pane <id>] <recipient> [<message>]

Post-send hook for ATM: nudges a named agent's tmux pane when a message is
delivered to them.

Normal mode (post-send hook, registered in [[atm.post_send_hooks]]):
  atm-nudge.py <recipient>
  Reads BOTH .atm.toml [[rmux.windows.panes]] tmux_pane_id AND
  ~/.claude/teams/<team>/config.json tmuxPaneId. If they agree, nudges.
  If either is missing or they disagree, prints a JSON error to stderr with
  a ready-to-run nudge_command and call_to_action, then exits 1.

Override mode (manual nudge or error recovery):
  atm-nudge.py --pane <id> <recipient> <message>
  Bypasses all config lookup. Validates inputs then nudges directly.

CLAUDE_PROJECT_DIR env var is used to locate .atm.toml; falls back to PWD
then os.getcwd() so hooks fired from worktree dirs still find the config.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import NamedTuple

try:
    import tomllib
except ModuleNotFoundError:
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ModuleNotFoundError:
        tomllib = None  # type: ignore[assignment]


CODEX_DEFAULT_PANE = "%1"
LOG_FILE = "/tmp/atm-nudge.log"


class PaneLookup(NamedTuple):
    pane_id: str | None
    error_code: str | None   # None on success; one of the ERR_* constants below
    error_msg: str | None    # human-readable detail


ERR_FILE_MISSING = "file_missing"
ERR_NOT_FOUND = "not_found"
ERR_EMPTY_PANE = "empty_pane"
ERR_PARSE_ERROR = "parse_error"
ERR_NO_TOMLLIB = "no_tomllib"


def log(message: str) -> None:
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    with open(LOG_FILE, "a") as f:
        f.write(f"{timestamp} {message}\n")


def candidate_start_dirs() -> list[Path]:
    """Return candidate directories for .atm.toml walk-up search.

    CLAUDE_PROJECT_DIR is checked first so hooks fired from worktree
    subdirectories still find the repo-root config.
    """
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


def read_pane_from_toml(recipient: str) -> PaneLookup:
    """Look up tmux_pane_id for recipient in .atm.toml [[rmux.windows.panes]]."""
    if tomllib is None:
        return PaneLookup(None, ERR_NO_TOMLLIB, "tomllib not available (install tomli for Python < 3.11)")
    for start_dir in candidate_start_dirs():
        toml_path = find_atm_toml(start_dir)
        if toml_path is None:
            continue
        try:
            with toml_path.open("rb") as f:
                config = tomllib.load(f)
        except Exception as exc:
            return PaneLookup(None, ERR_PARSE_ERROR, f"Cannot parse {toml_path}: {exc}")
        for window in config.get("rmux", {}).get("windows", []):
            for pane in window.get("panes", []):
                if pane.get("name") == recipient:
                    pane_id = pane.get("tmux_pane_id", "").strip()
                    if pane_id:
                        return PaneLookup(pane_id, None, None)
                    return PaneLookup(None, ERR_EMPTY_PANE,
                                      f"'{recipient}' found in .atm.toml but tmux_pane_id is empty")
        return PaneLookup(None, ERR_NOT_FOUND,
                          f"'{recipient}' not found in .atm.toml [[rmux.windows.panes]]")
    return PaneLookup(None, ERR_FILE_MISSING, ".atm.toml not found in any parent directory")


def read_pane_from_config(recipient: str, team: str) -> PaneLookup:
    """Look up tmuxPaneId for recipient in ~/.claude/teams/<team>/config.json."""
    config_path = Path.home() / ".claude" / "teams" / team / "config.json"
    if not config_path.exists():
        return PaneLookup(None, ERR_FILE_MISSING,
                          f"config.json not found for team '{team}' at {config_path}")
    try:
        config = json.loads(config_path.read_text())
    except Exception as exc:
        return PaneLookup(None, ERR_PARSE_ERROR, f"Cannot parse {config_path}: {exc}")
    member = next(
        (m for m in config.get("members", []) if m.get("name") == recipient), None
    )
    if member is None:
        return PaneLookup(None, ERR_NOT_FOUND,
                          f"'{recipient}' not in team '{team}' members")
    pane_id = member.get("tmuxPaneId", "").strip()
    if not pane_id:
        return PaneLookup(None, ERR_EMPTY_PANE,
                          f"'{recipient}' in team '{team}' has empty tmuxPaneId")
    return PaneLookup(pane_id, None, None)


def nudge_pane(pane_id: str, recipient: str, message: str) -> None:
    """Send message to a tmux pane. Validates all inputs are non-empty strings."""
    if not isinstance(pane_id, str) or not pane_id.strip():
        raise ValueError(f"pane_id must be a non-empty string, got: {pane_id!r}")
    if not isinstance(recipient, str) or not recipient.strip():
        raise ValueError(f"recipient must be a non-empty string, got: {recipient!r}")
    if not isinstance(message, str) or not message.strip():
        raise ValueError(f"message must be a non-empty string, got: {message!r}")
    subprocess.run(["tmux", "send-keys", "-t", pane_id, "-l", message], check=True)
    time.sleep(0.25)
    subprocess.run(["tmux", "send-keys", "-t", pane_id, "Enter"], check=True)
    log(f"nudged recipient={recipient} pane={pane_id}")


def build_message(team: str) -> str:
    return (
        f"<atm><action>read atm --team {team}</action>"
        f"<action>ack the message</action>"
        f"<action>execute the assigned task</action>"
        f'<when idle="immediate" busy="after-current-task"/>'
        f'<console announce="concise" pause="false"/></atm>'
    )


def build_nudge_command(pane: str, recipient: str, message: str) -> str:
    return f"python3 scripts/atm-nudge.py --pane {pane} {recipient} '{message}'"


def emit_error(data: dict) -> None:
    print(json.dumps(data, indent=2), file=sys.stderr)


def main(argv: list[str]) -> int:
    args = argv[1:]
    pane_override: str | None = None

    if len(args) >= 2 and args[0] == "--pane":
        pane_override = args[1].strip()
        args = args[2:]

    if not args or not args[0].strip():
        print("usage: atm-nudge.py [--pane <id>] <recipient> [<message>]", file=sys.stderr)
        return 1

    recipient = args[0].strip()
    message_arg = args[1].strip() if len(args) >= 2 else None

    team = resolve_team()
    message = message_arg if message_arg else build_message(team)

    # Override mode: bypass all config lookup and nudge directly.
    if pane_override:
        nudge_pane(pane_override, recipient, message)
        return 0

    toml = read_pane_from_toml(recipient)
    cfg = read_pane_from_config(recipient, team)

    nudge_cmd = build_nudge_command

    # ── Happy path ─────────────────────────────────────────────────────────────
    if toml.pane_id and cfg.pane_id and toml.pane_id == cfg.pane_id:
        nudge_pane(toml.pane_id, recipient, message)
        return 0

    # ── Error cases ────────────────────────────────────────────────────────────

    if toml.pane_id and cfg.pane_id and toml.pane_id != cfg.pane_id:
        emit_error({
            "status": "error",
            "error_code": "pane_mismatch",
            "recipient": recipient,
            "team": team,
            "detail": f"Pane mismatch for '{recipient}@{team}': .atm.toml={toml.pane_id}, config.json={cfg.pane_id}",
            "call_to_action": [
                "STOP: the ATM message was NOT delivered. Run nudge_command NOW before doing anything else.",
                "THEN fix both configs per fix[] so future sends work automatically.",
            ],
            "nudge_command": nudge_cmd(toml.pane_id, recipient, message),
            "fix": [
                f"Set tmux_pane_id in .atm.toml [[rmux.windows.panes]] name='{recipient}' to the correct pane",
                f"Set tmuxPaneId in ~/.claude/teams/{team}/config.json for '{recipient}' to the correct pane",
            ],
        })
        log(f"error: pane mismatch for {recipient}@{team} toml={toml.pane_id} config={cfg.pane_id}")
        return 1

    if toml.pane_id and not cfg.pane_id:
        if cfg.error_code == ERR_FILE_MISSING:
            error_code = "config_file_missing"
            detail = f"config.json not found for team '{team}'. Known pane from .atm.toml: {toml.pane_id}"
            cta0 = "STOP: the ATM message was NOT delivered and config.json is missing. Run nudge_command NOW before doing anything else."
            fix = [
                f"Create ~/.claude/teams/{team}/config.json and add tmuxPaneId = \"{toml.pane_id}\" for '{recipient}'",
            ]
        else:
            error_code = "recipient_not_in_config"
            detail = f"'{recipient}' not configured in config.json for team '{team}'. Known pane from .atm.toml: {toml.pane_id}"
            cta0 = "STOP: the ATM message was NOT delivered. Run nudge_command NOW before doing anything else."
            fix = [
                f"Add tmuxPaneId = \"{toml.pane_id}\" for '{recipient}' in ~/.claude/teams/{team}/config.json",
            ]
        emit_error({
            "status": "error",
            "error_code": error_code,
            "recipient": recipient,
            "team": team,
            "detail": detail,
            "call_to_action": [
                cta0,
                "THEN fix per fix[] so future sends work automatically.",
            ],
            "nudge_command": nudge_cmd(toml.pane_id, recipient, message),
            "fix": fix,
        })
        log(f"error: {recipient}@{team} toml={toml.pane_id} config error={cfg.error_code}: {cfg.error_msg}")
        return 1

    if cfg.pane_id and not toml.pane_id:
        if toml.error_code == ERR_FILE_MISSING:
            error_code = "toml_file_missing"
            detail = f".atm.toml not found in any parent directory. Known pane from config.json: {cfg.pane_id}"
            cta0 = "STOP: the ATM message was NOT delivered and .atm.toml is missing. Run nudge_command NOW before doing anything else."
            fix = [
                f"Create .atm.toml with [[rmux.windows.panes]] name='{recipient}' tmux_pane_id=\"{cfg.pane_id}\"",
            ]
        else:
            error_code = "recipient_not_in_toml"
            detail = f"'{recipient}' not found in .atm.toml [[rmux.windows.panes]]. Known pane from config.json: {cfg.pane_id}"
            cta0 = "STOP: the ATM message was NOT delivered. Run nudge_command NOW before doing anything else."
            fix = [
                f"Add tmux_pane_id = \"{cfg.pane_id}\" to [[rmux.windows.panes]] name='{recipient}' in .atm.toml",
            ]
        emit_error({
            "status": "error",
            "error_code": error_code,
            "recipient": recipient,
            "team": team,
            "detail": detail,
            "call_to_action": [
                cta0,
                "THEN fix per fix[] so future sends work automatically.",
            ],
            "nudge_command": nudge_cmd(cfg.pane_id, recipient, message),
            "fix": fix,
        })
        log(f"error: {recipient}@{team} config={cfg.pane_id} toml error={toml.error_code}: {toml.error_msg}")
        return 1

    # Neither source has a pane.
    emit_error({
        "status": "error",
        "error_code": "pane_not_configured",
        "recipient": recipient,
        "team": team,
        "detail": f"No pane configured for '{recipient}@{team}' in either source",
        "call_to_action": [
            f"STOP: the ATM message was NOT delivered. Run nudge_command NOW (using default Codex pane {CODEX_DEFAULT_PANE}).",
            "THEN fix both configs per fix[] so future sends work automatically.",
        ],
        "nudge_command": nudge_cmd(CODEX_DEFAULT_PANE, recipient, message),
        "fix": [
            f"Add tmux_pane_id to [[rmux.windows.panes]] name='{recipient}' in .atm.toml",
            f"Add tmuxPaneId for '{recipient}' in ~/.claude/teams/{team}/config.json",
        ],
    })
    log(f"error: pane not found for {recipient}@{team}: toml={toml.error_code} config={cfg.error_code}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
