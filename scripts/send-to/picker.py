#!/usr/bin/env python3
"""Reference Send-To picker adapter.

This process owns only UI selection and the PickerOutput JSON contract.  It
does not resolve ATM addresses, inspect files, or write staging data.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from typing import Any

SCHEMA_VERSION = 1


def fail(message: str) -> int:
    print(f"send-to picker: {message}", file=sys.stderr)
    return 1


def labels(rows: list[dict[str, Any]]) -> list[str]:
    return [str(row["label"]) for row in rows]


def selected_by_environment(rows: list[dict[str, Any]]) -> list[str] | None:
    raw = os.environ.get("ATM_SEND_TO_SELECTION")
    if raw is None:
        return None
    requested = {item.strip() for item in raw.split(",") if item.strip()}
    return [row["id"] for row in rows if row["id"] in requested]


def external_selection(rows: list[dict[str, Any]], backend: str) -> list[str] | None:
    menu = labels(rows)
    if not menu:
        return []
    if backend == "fzf":
        command = ["fzf", "--multi", "--prompt=Send To> "]
        completed = subprocess.run(command, input="\n".join(menu), text=True, capture_output=True)
        if completed.returncode != 0:
            return None
        chosen = set(completed.stdout.splitlines())
    elif backend == "osascript":
        def apple_string(value: str) -> str:
            return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'

        script = "set chosen to choose from list {" + ", ".join(apple_string(item) for item in menu)
        script += "} with title \"ATM Send-To\" with prompt \"Choose recipients\" with multiple selections allowed"
        completed = subprocess.run(["osascript", "-e", script], text=True, capture_output=True)
        if completed.returncode != 0 or completed.stdout.strip() == "false":
            return None
        chosen = {item.strip() for item in completed.stdout.strip().split(",") if item.strip()}
    elif backend == "zenity":
        command = ["zenity", "--list", "--checklist", "--multiple", "--separator=\n", "--title=ATM Send-To", "--column=Select", "--column=Recipient"]
        for item in menu:
            command.extend(["FALSE", item])
        completed = subprocess.run(command, text=True, capture_output=True)
        if completed.returncode != 0:
            return None
        chosen = {item.strip() for item in completed.stdout.splitlines() if item.strip()}
    else:
        raise ValueError(f"unknown picker backend {backend}")
    return [row["id"] for row in rows if row["label"] in chosen]


def validate_output(value: Any) -> None:
    if not isinstance(value, dict) or value.get("schema_version") != SCHEMA_VERSION:
        raise ValueError("PickerOutput schema_version is not supported")
    recipients = value.get("recipients")
    if not isinstance(recipients, list) or not recipients or any(not isinstance(item, str) or not item for item in recipients):
        raise ValueError("PickerOutput recipients must be a non-empty string array")
    if len(set(recipients)) != len(recipients):
        raise ValueError("PickerOutput recipients must not contain duplicates")
    if set(value) - {"schema_version", "recipients", "note"}:
        raise ValueError("PickerOutput contains unknown fields")
    if "note" in value and not isinstance(value["note"], str):
        raise ValueError("PickerOutput note must be a string")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--backend", choices=["fzf", "osascript", "zenity"])
    parser.add_argument("--validate", action="store_true")
    args = parser.parse_args(argv[1:])
    try:
        value = json.load(sys.stdin)
        if args.validate:
            validate_output(value)
            json.dump(value, sys.stdout, separators=(",", ":"))
            sys.stdout.write("\n")
            return 0
        rows = []
        if not isinstance(value, dict) or value.get("schema_version") != SCHEMA_VERSION:
            raise ValueError("PickerInput schema_version is not supported")
        for team in value.get("teams", []):
            for member in team.get("members", []):
                rows.append({"id": member["id"], "label": f"{team.get('name', team.get('id', '?'))} / {member.get('name', member['id'])} [{member.get('status', 'dead')}]"})
        selected = selected_by_environment(rows)
        if selected is None:
            backend = args.backend
            if backend is None:
                backend = "fzf"
            selected = external_selection(rows, backend)
        if selected is None:
            return fail("selection cancelled")
        if not selected:
            return fail("at least one recipient must be selected")
        output: dict[str, Any] = {"schema_version": SCHEMA_VERSION, "recipients": selected}
        if os.environ.get("ATM_SEND_TO_NOTE"):
            output["note"] = os.environ["ATM_SEND_TO_NOTE"]
        validate_output(output)
        json.dump(output, sys.stdout, separators=(",", ":"))
        sys.stdout.write("\n")
        return 0
    except (OSError, ValueError, json.JSONDecodeError, KeyError, subprocess.SubprocessError) as error:
        return fail(str(error))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
