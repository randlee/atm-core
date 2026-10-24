#!/usr/bin/env python3
"""atm_send.py <recipient> [<message>] [--team <team>] [-- <extra atm send args>]

Global ATM send wrapper. Fixes two recurring pain points with raw `atm send`:

1. Caller-context omission: raw `atm send` can silently resolve the wrong
   sender/team when `--as` and `--team` are omitted. This wrapper always
   passes `--as $ATM_IDENTITY` and `--team $ATM_TEAM` explicitly unless the
   caller already supplied those flags (`--from` is accepted as the old name
   of `--as`).

2. Recipient nudge: the daemon's built-in per-message nudge is the delivery
   mechanism. The wrapper has no nudge step of its own (its old tmux nudge
   script is gone and only produced false "NOT nudged" warnings); `atm send`
   reports a nudge it could not deliver, and the wrapper passes that through.
   `--no-nudge` and `--nudge-delay` are accepted and ignored for old callers.

Usage:
  atm_send.py <recipient> "message text"
  atm_send.py <recipient> "message text" --team other-team
  atm_send.py <recipient> --stdin < /path/to/body.txt

Any unrecognized options are passed through verbatim to `atm send` (e.g.
--requires-ack, --stdin, --summary). `--file` is refused: `atm send --file`
stores only a "File reference: <path>" body, never the file's contents; send
contents with `--stdin < <path>`, or call raw `atm send --file` when a
reference is intended.
"""
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path
from typing import NoReturn

SEND_OPTIONS_WITH_VALUES = {
    "--as",
    "--from",
    "--team",
    "--host",
    "--chat-id",
    "--file",
    "--template",
    "--vars",
    "--var",
    "--env-prefix",
    "--attach",
    "--category",
    "--tag",
    "--content-format",
    "--summary",
    "--task-id",
}
SEND_FLAG_OPTIONS = {
    "--stderr-logs",
    "--stdin",
    "--requires-ack",
    "--dry-run",
    "--json",
    "-h",
    "--help",
}


def die(message: str) -> NoReturn:
    print(f"atm_send: {message}", file=sys.stderr)
    raise SystemExit(1)


def has_explicit_flag(args: list[str], flag: str) -> bool:
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--":
            return False
        if arg == flag or arg.startswith(f"{flag}="):
            return True
        index += 1
    return False


def explicit_flag_value(args: list[str], flag: str) -> str | None:
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--":
            return None
        if arg == flag:
            if index + 1 < len(args):
                value = args[index + 1].strip()
                return value or None
            return None
        prefix = f"{flag}="
        if arg.startswith(prefix):
            value = arg[len(prefix) :].strip()
            return value or None
        index += 1
    return None


def remove_flag(args: list[str], flag: str) -> list[str]:
    result: list[str] = []
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == flag:
            index += 1
            continue
        result.append(arg)
        index += 1
    return result


def resolve_atm_command_cwd() -> str:
    current = Path.cwd()
    candidates: list[Path] = []
    try:
        common_dir = subprocess.run(
            ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
            cwd=current,
            capture_output=True,
            text=True,
            check=False,
        )
        if common_dir.returncode == 0:
            common_dir_path = Path(common_dir.stdout.strip())
            if common_dir_path.name == ".git":
                candidates.append(common_dir_path.parent)
    except OSError:
        pass

    try:
        repo_root = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=current,
            capture_output=True,
            text=True,
            check=False,
        )
        if repo_root.returncode == 0:
            candidates.append(Path(repo_root.stdout.strip()))
    except OSError:
        pass

    candidates.append(current)
    for candidate in candidates:
        if candidate and (candidate / ".atm.toml").is_file():
            return str(candidate)
    return str(current)


def normalize_send_args(args: list[str]) -> tuple[list[str], str | None, str | None]:
    """Split wrapper argv into normalized atm-send options plus TO/MESSAGE."""
    send_options: list[str] = []
    recipient: str | None = None
    message: str | None = None
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--":
            index += 1
            continue
        if arg in ("--no-nudge", "--nudge-delay") or arg.startswith("--nudge-delay="):
            if arg == "--nudge-delay":
                index += 2
            else:
                index += 1
            continue
        if arg in SEND_OPTIONS_WITH_VALUES:
            if index + 1 >= len(args):
                die(f"{arg} requires a value")
            send_options.extend([arg, args[index + 1]])
            index += 2
            continue
        matched_option = False
        for option in SEND_OPTIONS_WITH_VALUES:
            prefix = f"{option}="
            if arg.startswith(prefix):
                send_options.append(arg)
                index += 1
                matched_option = True
                break
        if matched_option:
            continue
        if arg in SEND_FLAG_OPTIONS or arg.startswith("-"):
            send_options.append(arg)
            index += 1
            continue
        value = arg.strip()
        if not value:
            index += 1
            continue
        if recipient is None:
            recipient = value
        elif message is None:
            message = value
        else:
            send_options.append(value)
        index += 1

    return send_options, recipient, message


def main(argv: list[str]) -> int:
    args = argv[1:]
    if not args:
        print(__doc__)
        return 1
    if any(arg in ("-h", "--help") for arg in args):
        print(__doc__)
        return 0

    passthrough, recipient, message = normalize_send_args(args)
    if not recipient:
        die("recipient is required")

    if has_explicit_flag(passthrough, "--file"):
        die("--file is refused: `atm send --file` stores only a file reference, not the file's contents. "
            "Send the contents with `--stdin < <path>`, or call raw `atm send --file` if a reference is intended.")
    passthrough = [f"--as{arg[len('--from'):]}" if arg == "--from" or arg.startswith("--from=") else arg for arg in passthrough]
    explicit_from = has_explicit_flag(passthrough, "--as")
    explicit_team = has_explicit_flag(passthrough, "--team")

    effective_identity = explicit_flag_value(passthrough, "--as")
    effective_team = explicit_flag_value(passthrough, "--team")
    stdin_mode = has_explicit_flag(passthrough, "--stdin")
    stdin_payload: bytes | None = sys.stdin.buffer.read() if stdin_mode else None

    send_cmd = ["atm", "send"]

    if not explicit_from:
        identity = os.environ.get("ATM_IDENTITY", "").strip()
        if not identity:
            die("ATM_IDENTITY is not set; refusing to send without a real --as identity")
        effective_identity = identity
        send_cmd.extend(["--as", identity])

    if not explicit_team:
        team = os.environ.get("ATM_TEAM", "").strip()
        if not team:
            die("ATM_TEAM is not set; refusing to send without an explicit --team")
        effective_team = team
        send_cmd.extend(["--team", team])

    send_cmd.extend(passthrough)
    send_cmd.append(recipient)
    if message is not None:
        send_cmd.append(message)
    child_env = os.environ.copy()
    if effective_identity:
        child_env["ATM_IDENTITY"] = effective_identity
    if effective_team:
        child_env["ATM_TEAM"] = effective_team
    command_cwd = resolve_atm_command_cwd()
    try:
        return subprocess.run(send_cmd, env=child_env, cwd=command_cwd, input=stdin_payload).returncode
    except OSError as error:
        die(f"cannot run atm send: {error}")


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
