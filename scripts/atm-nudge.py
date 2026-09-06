#!/usr/bin/env python3
"""OBSOLETE Phase AD helper retained only for manual troubleshooting.

atm-nudge.py [--pane <id>] <recipient> [<message>]

ATM now ships the built-in `atm internal-nudge` path as the default post-send
emitter. This helper survives only as an explicit repo-local override or
manual troubleshooting tool, and it must resolve pane routing from canonical
ATM roster state or an explicit `--pane`.

Legacy helper: nudge a named agent's tmux pane after successful send.

Normal mode:
  atm-nudge.py <recipient>
  Resolves the target pane from canonical ATM roster state via
  `atm members --team <team> --json`.

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

ERR_NOT_FOUND = "not_found"
ERR_EMPTY_PANE = "empty_pane"
ERR_PARSE_ERROR = "parse_error"
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
    from_value = str(payload.get("from", "")).strip()
    is_ack = payload.get("is_ack") is True
    message_id = str(payload.get("message_id", "")).strip()
    task_id = str(payload.get("task_id", "")).strip()
    requires_ack = payload.get("requires_ack") is True
    description = ""
    for key in ("description", "summary"):
        value = payload.get(key)
        if isinstance(value, str) and value.strip():
            description = value.strip()
            break
    base_attrs: list[str] = []
    if from_value:
        base_attrs.append(f'from="{from_value}"')
    if message_id:
        base_attrs.append(f'message-id="{message_id}"')
    base = "<atm" + (f" {' '.join(base_attrs)}" if base_attrs else "")
    if is_ack:
        if task_id:
            return f'{base} kind="ack" task-id="{task_id}"/>'
        return f'{base} kind="ack"/>'

    read_action = f"atm read --message-id {message_id}" if message_id else "atm read"
    body = [f"{base}>", f"<action>{read_action}</action>"]
    if requires_ack:
        body.append("<action>ack the message</action>")
    if task_id:
        body.append(f"<task id=\"{task_id}\">{description}</task>")
    elif description:
        body.append(f"<description>{description}</description>")
    else:
        body.append("<description></description>")
    body.extend(
        [
            "<action>execute the assigned task</action>",
            '<when idle="immediate" busy="after-current-task"/>',
            '<console announce="concise" pause="false"/>',
            "</atm>",
        ]
    )
    return (
        "".join(body)
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
) -> dict[str, object]:
    recommended_pane = CODEX_DEFAULT_PANE
    nudge_command = build_nudge_command(recommended_pane, recipient, message)
    try:
        cwd = os.getcwd()
    except Exception:
        cwd = None

    call_to_action = [
        "STOP: the ATM message was NOT delivered automatically.",
        f"Run nudge_command NOW to deliver the message manually using suggested pane {recommended_pane}.",
        "VERIFY the pane id before running it; the suggested pane may be stale or incorrect.",
        "THEN repair canonical ATM roster state so future sends work automatically.",
    ]

    fix: list[str] = []
    pane_hint = recommended_pane
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

    if not fix:
        fix.append("Review canonical ATM roster state before retrying the nudge.")

    return {
        "status": "error",
        "error_code": roster.error_code or "nudge_resolution_failed",
        "recipient": recipient,
        "team": team,
        "detail": roster.error_msg or "Unable to resolve pane from canonical ATM roster state",
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
            "recommended_pane_source": "manual default",
            "roster_lookup": roster.source_path,
            "roster_error_code": roster.error_code,
            "roster_error": roster.error_msg,
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

    if roster.pane_id:
        nudge_pane(roster.pane_id, recipient, message)
        return 0

    payload = build_error_payload(
        recipient=recipient,
        team=team,
        message=message,
        roster=roster,
    )
    emit_json_stderr(payload)
    log(
        f"error: pane resolution failed for {recipient}@{team}: "
        f"roster={roster.error_code}"
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
