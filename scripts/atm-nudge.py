#!/usr/bin/env python3
"""atm-nudge.py [--pane <id>] <recipient> [<message>]

Post-send hook for ATM: nudge a named agent's tmux pane after successful send.

Normal mode:
  atm-nudge.py <recipient>
  Resolves the target pane from canonical ATM roster state first via
  `atm members --team <team> --json`. If that lookup cannot produce a usable
  pane id, the script falls back to the repo-local `.atm.toml` pane mapping as
  a last-resort compatibility seam.

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
import tempfile
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
LOG_FILE = str(Path(tempfile.gettempdir()) / "atm-nudge.log")

ERR_FILE_MISSING = "file_missing"
ERR_NOT_FOUND = "not_found"
ERR_EMPTY_PANE = "empty_pane"
ERR_PARSE_ERROR = "parse_error"
ERR_NO_TOMLLIB = "no_tomllib"
ERR_AMBIGUOUS = "ambiguous_match"
ERR_INVALID_STRUCTURE = "invalid_structure"
ERR_COMMAND_FAILED = "command_failed"


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


def read_pane_from_roster(
    recipient: str,
    team: str,
    payload: dict[str, object] | None = None,
) -> PaneLookup:
    """Read the authoritative pane from canonical ATM roster state."""
    command = ["atm", "members", "--team", team, "--json"]
    env = dict(os.environ)
    env.setdefault("ATM_TEAM", team)
    if payload is not None:
        sender = payload.get("sender")
        if isinstance(sender, str) and sender.strip():
            env.setdefault("ATM_IDENTITY", sender.strip())

    source = "atm members --team <team> --json"
    try:
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            check=False,
            env=env,
        )
    except OSError as exc:
        return PaneLookup(
            None,
            ERR_COMMAND_FAILED,
            f"Cannot run {' '.join(command)}: {exc}",
            source,
        )

    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or f"exit status {result.returncode}"
        return PaneLookup(
            None,
            ERR_COMMAND_FAILED,
            f"{' '.join(command)} failed: {detail}",
            source,
        )

    try:
        response = json.loads(result.stdout)
    except Exception as exc:
        return PaneLookup(
            None,
            ERR_PARSE_ERROR,
            f"Cannot parse ATM roster JSON from {' '.join(command)}: {exc}",
            source,
        )

    members = response.get("members")
    if not isinstance(members, list):
        return PaneLookup(
            None,
            ERR_INVALID_STRUCTURE,
            f"{' '.join(command)} returned invalid members structure",
            source,
        )

    member = next(
        (entry for entry in members if isinstance(entry, dict) and entry.get("name") == recipient),
        None,
    )
    if member is None:
        return PaneLookup(
            None,
            ERR_NOT_FOUND,
            f"'{recipient}' not in canonical ATM roster for team '{team}'",
            source,
        )

    pane_id = str(member.get("tmux_pane_id", "")).strip()
    if not pane_id:
        return PaneLookup(
            None,
            ERR_EMPTY_PANE,
            f"'{recipient}' in canonical ATM roster for team '{team}' has empty tmux_pane_id",
            source,
        )

    return PaneLookup(pane_id, None, None, source)


def read_pane_from_toml(recipient: str, team: str) -> PaneLookup:
    """Read a fallback pane from the repo-local .atm.toml."""
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


def build_message(team: str, payload: dict[str, object] | None = None) -> str:
    payload = payload or {}
    is_ack = payload.get("is_ack") is True
    message_id = str(payload.get("message_id", "")).strip()
    description = ""
    for key in ("description", "summary"):
        value = payload.get(key)
        if isinstance(value, str) and value.strip():
            description = value.strip()
            break
    if is_ack:
        acknowledgement = (
            f"message {message_id} acknowledged"
            if message_id
            else "message acknowledged"
        )
        message_context = (
            f"<message-id>{message_id}</message-id>" if message_id else ""
        )
        return (
            f"<atm><action>read atm --team {team}</action>"
            f"<action>ack the message</action>"
            f"{message_context}"
            f"<action>{acknowledgement}</action>"
            f"<action>complete associated work immediately</action>"
            f'<when idle="immediate" busy="complete tasks based on established priority"/>'
            f'<console announce="concise" pause="false"/></atm>'
        )

    message_context = (
        f"<message-id>{message_id}</message-id>" if message_id else ""
    )
    description_context = (
        f"<description>{description}</description>" if description else ""
    )
    return (
        f"<atm><action>read atm --team {team}</action>"
        f"<action>ack the message</action>"
        f"{message_context}"
        f"{description_context}"
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
    roster: PaneLookup,
    toml: PaneLookup,
) -> dict[str, object]:
    recommended_pane = toml.pane_id or CODEX_DEFAULT_PANE
    recommended_source = ".atm.toml fallback" if toml.pane_id else "default"
    discovered_toml = discover_atm_toml()
    toml_path = toml.source_path or (str(discovered_toml) if discovered_toml else None)
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
    pane_hint = toml.pane_id or "<pane>"
    fix.append(
        f"Repair canonical ATM roster pane metadata with `atm teams update-member {team} {recipient} --pane-id {pane_hint}`."
    )
    if roster.error_code == ERR_COMMAND_FAILED:
        fix.append(
            f"Make sure `atm members --team {team} --json` succeeds from the hook environment and preserves ATM_IDENTITY/ATM_TEAM."
        )
    elif roster.error_code == ERR_NOT_FOUND:
        fix.append(
            f"Add or restore '{recipient}@{team}' in the canonical ATM roster before relying on automatic nudges."
        )
    elif roster.error_code == ERR_EMPTY_PANE:
        fix.append(
            f"Set tmux_pane_id for '{recipient}@{team}' in canonical ATM roster state via `atm teams update-member`."
        )
    elif roster.error_code == ERR_PARSE_ERROR:
        fix.append(
            "Investigate the `atm members --json` response; the hook could not parse canonical ATM roster output."
        )
    elif roster.error_code == ERR_INVALID_STRUCTURE:
        fix.append(
            "Investigate the `atm members --json` response shape; the canonical ATM roster output was not in the expected format."
        )

    if toml.error_code in {ERR_FILE_MISSING, ERR_PARSE_ERROR, ERR_INVALID_STRUCTURE}:
        fix.append("Fix or restore the repo-local .atm.toml so the compatibility fallback can resolve a pane if roster lookup fails again.")
    elif toml.error_code == ERR_NOT_FOUND:
        fix.append(f"Add [[rmux.windows.panes]] name='{recipient}' with env.ATM_TEAM='{team}' and a tmux_pane_id in .atm.toml as a last-resort fallback.")
    elif toml.error_code == ERR_EMPTY_PANE:
        fix.append(f"Set tmux_pane_id for '{recipient}@{team}' in .atm.toml if the fallback mapping should remain available.")
    elif toml.error_code == ERR_AMBIGUOUS:
        fix.append(f"Make the .atm.toml fallback mapping for '{recipient}@{team}' unique so the hook can select exactly one pane.")
    elif toml.error_code == ERR_NO_TOMLLIB:
        fix.append("Install tomli (Python < 3.11) or run the hook under Python 3.11+.")

    if not fix:
        fix.append("Review canonical ATM roster state and the repo-local .atm.toml fallback before retrying the nudge.")

    return {
        "status": "error",
        "error_code": roster.error_code or toml.error_code or "nudge_resolution_failed",
        "recipient": recipient,
        "team": team,
        "detail": roster.error_msg or toml.error_msg or "Unable to resolve pane from canonical ATM roster state or .atm.toml fallback",
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
            "authoritative_source": "atm roster",
            "recommended_pane": recommended_pane,
            "recommended_pane_source": recommended_source,
            "roster_lookup": roster.source_path,
            "roster_error_code": roster.error_code,
            "roster_error": roster.error_msg,
            "toml_path": toml_path,
            "toml_error_code": toml.error_code,
            "toml_error": toml.error_msg,
        },
    }


def build_warning_payload(
    *,
    recipient: str,
    team: str,
    message: str,
    roster: PaneLookup,
    delivered_pane: str,
    toml: PaneLookup,
) -> dict[str, object]:
    try:
        cwd = os.getcwd()
    except Exception:
        cwd = None
    detail = (
        f"Nudge sent to pane {delivered_pane} from .atm.toml fallback for "
        f"'{recipient}@{team}' because canonical ATM roster lookup did not yield a usable pane"
    )
    fix = [
        f"Repair canonical ATM roster pane metadata with `atm teams update-member {team} {recipient} --pane-id {delivered_pane}`."
    ]
    if roster.error_code == ERR_COMMAND_FAILED:
        fix.append(
            f"Make sure `atm members --team {team} --json` succeeds from the hook environment and preserves ATM_IDENTITY/ATM_TEAM."
        )
    elif roster.error_code == ERR_NOT_FOUND:
        fix.append(f"Add or restore '{recipient}@{team}' in canonical ATM roster state.")
    elif roster.error_code == ERR_EMPTY_PANE:
        fix.append(f"Set tmux_pane_id for '{recipient}@{team}' in canonical ATM roster state.")
    elif roster.error_code == ERR_PARSE_ERROR:
        fix.append("Investigate the `atm members --json` response; the hook could not parse canonical ATM roster output.")
    elif roster.error_code == ERR_INVALID_STRUCTURE:
        fix.append("Investigate the `atm members --json` response shape; the canonical ATM roster output was not in the expected format.")

    return {
        "status": "warning",
        "error_code": "roster_pane_fallback",
        "recipient": recipient,
        "team": team,
        "detail": detail,
        "call_to_action": [
            f"NOTICE: nudge already sent to pane {delivered_pane} from the .atm.toml fallback.",
            f"NOW repair canonical ATM roster state so future nudges use SQLite-backed pane metadata first for '{recipient}@{team}'.",
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
            "authoritative_source": "atm roster",
            "delivered_pane": delivered_pane,
            "delivered_source": ".atm.toml fallback",
            "roster_lookup": roster.source_path,
            "roster_error_code": roster.error_code,
            "roster_error": roster.error_msg,
            "toml_path": toml.source_path,
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
    payload = read_post_send_payload()
    message = message_arg if message_arg else build_message(team, payload)

    if pane_override:
        nudge_pane(pane_override, recipient, message)
        return 0

    roster = read_pane_from_roster(recipient, team, payload)
    toml = read_pane_from_toml(recipient, team)

    if roster.pane_id:
        nudge_pane(roster.pane_id, recipient, message)
        return 0

    if toml.pane_id:
        nudge_pane(toml.pane_id, recipient, message)
        if roster.error_code:
            warning = build_warning_payload(
                recipient=recipient,
                team=team,
                message=message,
                roster=roster,
                delivered_pane=toml.pane_id,
                toml=toml,
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
                    "roster_error_code": roster.error_code,
                    "roster_error": roster.error_msg,
                },
            )
        return 0

    payload = build_error_payload(
        recipient=recipient,
        team=team,
        message=message,
        roster=roster,
        toml=toml,
    )
    emit_json_stderr(payload)
    log(
        f"error: pane resolution failed for {recipient}@{team}: "
        f"roster={roster.error_code} toml={toml.error_code}"
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
