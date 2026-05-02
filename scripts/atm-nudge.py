#!/usr/bin/env python3
"""atm-nudge.py [--pane <id>] <recipient> [<message>]

Post-send hook for ATM: nudge a named agent's tmux pane after successful send.

Normal mode:
  atm-nudge.py <recipient>
  Resolves the target pane from the committed repo-local `.atm.toml` by
  matching both recipient name and ATM team. `config.json` is read only for
  advisory diagnostics and recovery suggestions; it is not the authoritative
  pane source for delivery.

Override mode:
  atm-nudge.py --pane <id> <recipient> [<message>]
  Bypasses file lookup and nudges directly.
"""
from __future__ import annotations

import json
import os
import shlex
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

ERR_FILE_MISSING = "file_missing"
ERR_NOT_FOUND = "not_found"
ERR_EMPTY_PANE = "empty_pane"
ERR_PARSE_ERROR = "parse_error"
ERR_NO_TOMLLIB = "no_tomllib"
ERR_AMBIGUOUS = "ambiguous_match"
ERR_INVALID_STRUCTURE = "invalid_structure"


class PaneLookup(NamedTuple):
    pane_id: str | None
    error_code: str | None
    error_msg: str | None
    source_path: str | None = None


def log(message: str) -> None:
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    with open(LOG_FILE, "a", encoding="utf-8") as handle:
        handle.write(f"{timestamp} {message}\n")


def candidate_start_dirs() -> list[Path]:
    """Return candidate directories for .atm.toml walk-up search."""
    candidates: list[Path] = []
    seen: set[Path] = set()
    raw_candidates = [
        os.environ.get("CLAUDE_PROJECT_DIR", "").strip(),
        os.environ.get("PWD", "").strip(),
    ]
    try:
        raw_candidates.append(os.getcwd())
    except Exception:
        pass
    for raw in raw_candidates:
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


def discover_atm_toml() -> Path | None:
    for start_dir in candidate_start_dirs():
        toml_path = find_atm_toml(start_dir)
        if toml_path is not None:
            return toml_path
    return None


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

    toml_path = discover_atm_toml()
    if tomllib is not None and toml_path is not None:
        try:
            with toml_path.open("rb") as handle:
                config = tomllib.load(handle)
            for section in ("atm", "core"):
                team = config.get(section, {}).get("default_team")
                if isinstance(team, str) and team.strip():
                    return team.strip()
        except Exception:
            pass

    env_team = os.environ.get("ATM_TEAM", "").strip()
    return env_team or "atm-dev"


def _normalize_team(candidate: object) -> str | None:
    if not isinstance(candidate, str):
        return None
    value = candidate.strip()
    return value or None


def _pane_team(pane: dict[str, object]) -> str | None:
    env = pane.get("env")
    if not isinstance(env, dict):
        return None
    return _normalize_team(env.get("ATM_TEAM"))


def read_pane_from_toml(recipient: str, team: str) -> PaneLookup:
    """Read the authoritative pane from the repo-local .atm.toml."""
    if tomllib is None:
        return PaneLookup(
            None,
            ERR_NO_TOMLLIB,
            "tomllib not available (install tomli for Python < 3.11)",
        )

    toml_path = discover_atm_toml()
    if toml_path is None:
        return PaneLookup(
            None,
            ERR_FILE_MISSING,
            ".atm.toml not found in any parent directory",
        )

    try:
        with toml_path.open("rb") as handle:
            config = tomllib.load(handle)
    except Exception as exc:
        return PaneLookup(
            None,
            ERR_PARSE_ERROR,
            f"Cannot parse {toml_path}: {exc}",
            str(toml_path),
        )

    windows = config.get("rmux", {}).get("windows", [])
    if not isinstance(windows, list):
        return PaneLookup(
            None,
            ERR_INVALID_STRUCTURE,
            f"{toml_path} has invalid rmux.windows structure",
            str(toml_path),
        )

    matches: list[dict[str, object]] = []
    team_matches: list[dict[str, object]] = []

    for window in windows:
        if not isinstance(window, dict):
            continue
        panes = window.get("panes", [])
        if not isinstance(panes, list):
            continue
        for pane in panes:
            if not isinstance(pane, dict):
                continue
            if pane.get("name") != recipient:
                continue
            matches.append(pane)
            if _pane_team(pane) == team:
                team_matches.append(pane)

    if not matches:
        return PaneLookup(
            None,
            ERR_NOT_FOUND,
            f"'{recipient}' not found in {toml_path} [[rmux.windows.panes]]",
            str(toml_path),
        )

    if not team_matches and len(matches) == 1:
        team_matches = matches

    if not team_matches:
        return PaneLookup(
            None,
            ERR_NOT_FOUND,
            f"'{recipient}' found in {toml_path}, but no pane is tagged with ATM_TEAM='{team}'",
            str(toml_path),
        )

    if len(team_matches) > 1:
        panes = ", ".join(str(pane.get("tmux_pane_id", "")).strip() or "<empty>" for pane in team_matches)
        return PaneLookup(
            None,
            ERR_AMBIGUOUS,
            f"Multiple panes match '{recipient}@{team}' in {toml_path}: {panes}",
            str(toml_path),
        )

    pane_id = str(team_matches[0].get("tmux_pane_id", "")).strip()
    if not pane_id:
        return PaneLookup(
            None,
            ERR_EMPTY_PANE,
            f"'{recipient}@{team}' found in {toml_path} but tmux_pane_id is empty",
            str(toml_path),
        )

    return PaneLookup(pane_id, None, None, str(toml_path))


def read_pane_from_config(recipient: str, team: str) -> PaneLookup:
    """Read advisory pane info from Claude team config.json."""
    config_path = Path.home() / ".claude" / "teams" / team / "config.json"
    if not config_path.exists():
        return PaneLookup(
            None,
            ERR_FILE_MISSING,
            f"config.json not found for team '{team}' at {config_path}",
            str(config_path),
        )
    try:
        config = json.loads(config_path.read_text(encoding="utf-8"))
    except Exception as exc:
        return PaneLookup(
            None,
            ERR_PARSE_ERROR,
            f"Cannot parse {config_path}: {exc}",
            str(config_path),
        )
    members = config.get("members", [])
    if not isinstance(members, list):
        return PaneLookup(
            None,
            ERR_INVALID_STRUCTURE,
            f"{config_path} has invalid members structure",
            str(config_path),
        )

    member = next(
        (entry for entry in members if isinstance(entry, dict) and entry.get("name") == recipient),
        None,
    )
    if member is None:
        return PaneLookup(
            None,
            ERR_NOT_FOUND,
            f"'{recipient}' not in team '{team}' members",
            str(config_path),
        )

    pane_id = str(member.get("tmuxPaneId", "")).strip()
    if not pane_id:
        return PaneLookup(
            None,
            ERR_EMPTY_PANE,
            f"'{recipient}' in team '{team}' has empty tmuxPaneId",
            str(config_path),
        )

    return PaneLookup(pane_id, None, None, str(config_path))


def nudge_pane(pane_id: str, recipient: str, message: str) -> None:
    """Send a message to a tmux pane after validating all inputs."""
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
    argv = [
        sys.executable or "python3",
        str(Path(__file__).resolve()),
        "--pane",
        pane,
        recipient,
        message,
    ]
    return shlex.join(argv)


def emit_json_stderr(data: dict[str, object]) -> None:
    print(json.dumps(data, indent=2), file=sys.stderr)


def emit_hook_result(level: str, message: str, fields: dict[str, object]) -> None:
    print(json.dumps({"level": level, "message": message, "fields": fields}))


def build_error_payload(
    *,
    recipient: str,
    team: str,
    message: str,
    toml: PaneLookup,
    cfg: PaneLookup,
) -> dict[str, object]:
    recommended_pane = cfg.pane_id or CODEX_DEFAULT_PANE
    recommended_source = "config.json" if cfg.pane_id else "default"
    discovered_toml = discover_atm_toml()
    toml_path = toml.source_path or (str(discovered_toml) if discovered_toml else None)
    config_path = cfg.source_path or str(Path.home() / ".claude" / "teams" / team / "config.json")
    nudge_command = build_nudge_command(recommended_pane, recipient, message)
    try:
        cwd = os.getcwd()
    except Exception:
        cwd = None

    call_to_action = [
        "STOP: the ATM message was NOT delivered automatically.",
        f"Run nudge_command NOW to deliver the message manually using suggested pane {recommended_pane} from {recommended_source}.",
        "VERIFY the pane id before running it; the suggested pane may be stale or incorrect.",
        "THEN fix the configuration in fix[] so future sends work automatically.",
    ]

    fix: list[str] = []
    if toml.error_code in {ERR_FILE_MISSING, ERR_PARSE_ERROR, ERR_INVALID_STRUCTURE}:
        fix.append("Fix or restore the repo-local .atm.toml so the hook can resolve a committed pane mapping.")
    elif toml.error_code == ERR_NOT_FOUND:
        fix.append(f"Add [[rmux.windows.panes]] name='{recipient}' with env.ATM_TEAM='{team}' and a tmux_pane_id in .atm.toml.")
    elif toml.error_code == ERR_EMPTY_PANE:
        fix.append(f"Set tmux_pane_id for '{recipient}@{team}' in .atm.toml.")
    elif toml.error_code == ERR_AMBIGUOUS:
        fix.append(f"Make the .atm.toml pane mapping for '{recipient}@{team}' unique so the hook can select exactly one pane.")
    elif toml.error_code == ERR_NO_TOMLLIB:
        fix.append("Install tomli (Python < 3.11) or run the hook under Python 3.11+.")

    if cfg.error_code == ERR_FILE_MISSING:
        fix.append(f"Create {config_path} so Claude Code also has a pane mapping for '{recipient}@{team}'.")
    elif cfg.error_code == ERR_PARSE_ERROR:
        fix.append(f"Fix JSON syntax in {config_path}.")
    elif cfg.error_code == ERR_NOT_FOUND:
        fix.append(f"Add '{recipient}' with tmuxPaneId to {config_path}.")
    elif cfg.error_code == ERR_EMPTY_PANE:
        fix.append(f"Set tmuxPaneId for '{recipient}' in {config_path}.")

    if not fix:
        fix.append("Review .atm.toml and config.json pane mappings before retrying the nudge.")

    return {
        "status": "error",
        "error_code": toml.error_code or "nudge_resolution_failed",
        "recipient": recipient,
        "team": team,
        "detail": toml.error_msg or "Unable to resolve pane from .atm.toml",
        "call_to_action": call_to_action,
        "nudge_command": nudge_command,
        "fix": fix,
        "input": {
            "recipient": recipient,
            "team": team,
            "message": message,
            "cwd": cwd,
            "claude_project_dir": os.environ.get("CLAUDE_PROJECT_DIR"),
            "pwd": os.environ.get("PWD"),
        },
        "pane_resolution": {
            "authoritative_source": ".atm.toml",
            "recommended_pane": recommended_pane,
            "recommended_pane_source": recommended_source,
            "toml_path": toml_path,
            "toml_error_code": toml.error_code,
            "toml_error": toml.error_msg,
            "config_path": config_path,
            "config_pane": cfg.pane_id,
            "config_error_code": cfg.error_code,
            "config_error": cfg.error_msg,
        },
    }


def build_warning_payload(
    *,
    recipient: str,
    team: str,
    message: str,
    delivered_pane: str,
    toml: PaneLookup,
    cfg: PaneLookup,
) -> dict[str, object]:
    config_path = cfg.source_path or str(Path.home() / ".claude" / "teams" / team / "config.json")
    try:
        cwd = os.getcwd()
    except Exception:
        cwd = None
    if cfg.pane_id:
        detail = (
            f"Nudge sent to pane {delivered_pane} from .atm.toml for "
            f"'{recipient}@{team}', but config.json points to {cfg.pane_id}"
        )
        fix = [f"Update tmuxPaneId for '{recipient}' in {config_path} to '{delivered_pane}'."]
    else:
        detail = (
            f"Nudge sent to pane {delivered_pane} from .atm.toml for "
            f"'{recipient}@{team}', but config.json is not consistent enough to confirm the same pane"
        )
        fix = []
        if cfg.error_code == ERR_FILE_MISSING:
            fix.append(f"Create {config_path} and add '{recipient}' with tmuxPaneId '{delivered_pane}'.")
        elif cfg.error_code == ERR_PARSE_ERROR:
            fix.append(f"Fix JSON syntax in {config_path} and set tmuxPaneId for '{recipient}' to '{delivered_pane}'.")
        elif cfg.error_code == ERR_NOT_FOUND:
            fix.append(f"Add '{recipient}' with tmuxPaneId '{delivered_pane}' to {config_path}.")
        elif cfg.error_code == ERR_EMPTY_PANE:
            fix.append(f"Set tmuxPaneId for '{recipient}' in {config_path} to '{delivered_pane}'.")
        elif cfg.error_code == ERR_INVALID_STRUCTURE:
            fix.append(f"Repair the members structure in {config_path} and set tmuxPaneId for '{recipient}' to '{delivered_pane}'.")
        else:
            fix.append(f"Review {config_path} and align tmuxPaneId for '{recipient}' to '{delivered_pane}'.")

    return {
        "status": "warning",
        "error_code": "config_json_out_of_sync",
        "recipient": recipient,
        "team": team,
        "detail": detail,
        "call_to_action": [
            f"NOTICE: nudge already sent to pane {delivered_pane} from .atm.toml.",
            f"NOW fix config.json so Claude Code uses the same pane for '{recipient}@{team}'.",
            "If you need to resend manually, use nudge_command below and verify the pane id first.",
        ],
        "nudge_command": build_nudge_command(delivered_pane, recipient, message),
        "fix": fix,
        "input": {
            "recipient": recipient,
            "team": team,
            "message": message,
            "cwd": cwd,
            "claude_project_dir": os.environ.get("CLAUDE_PROJECT_DIR"),
            "pwd": os.environ.get("PWD"),
        },
        "pane_resolution": {
            "authoritative_source": ".atm.toml",
            "delivered_pane": delivered_pane,
            "toml_path": toml.source_path,
            "config_path": config_path,
            "config_pane": cfg.pane_id,
            "config_error_code": cfg.error_code,
            "config_error": cfg.error_msg,
        },
    }


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

    if pane_override:
        nudge_pane(pane_override, recipient, message)
        return 0

    toml = read_pane_from_toml(recipient, team)
    cfg = read_pane_from_config(recipient, team)

    if toml.pane_id:
        if cfg.pane_id and cfg.pane_id != toml.pane_id:
            log(
                f"warn: config mismatch for {recipient}@{team}: "
                f"toml={toml.pane_id} config={cfg.pane_id}"
            )
        nudge_pane(toml.pane_id, recipient, message)
        if cfg.pane_id != toml.pane_id or cfg.error_code:
            warning = build_warning_payload(
                recipient=recipient,
                team=team,
                message=message,
                delivered_pane=toml.pane_id,
                toml=toml,
                cfg=cfg,
            )
            emit_json_stderr(warning)
            emit_hook_result(
                "warn",
                warning["detail"],
                {
                    "recipient": recipient,
                    "team": team,
                    "delivered_pane": toml.pane_id,
                    "nudge_command": warning["nudge_command"],
                    "call_to_action": warning["call_to_action"],
                    "config_error_code": cfg.error_code,
                    "config_error": cfg.error_msg,
                },
            )
            return 0
        return 0

    payload = build_error_payload(
        recipient=recipient,
        team=team,
        message=message,
        toml=toml,
        cfg=cfg,
    )
    emit_json_stderr(payload)
    log(
        f"error: pane resolution failed for {recipient}@{team}: "
        f"toml={toml.error_code} config={cfg.error_code}"
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
