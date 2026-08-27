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

# Wizard command shape the atm-core adapter generates for the real Wyvern
# invocation (docs/plans/phase-aq/fixtures/wyvern-pick-member-contract.md):
# Wyvern has no `--picker <path>` flag, so PickerInput travels as the wizard
# command's opaque `config` field, and PickerOutput arrives nested under the
# terminal WizardResult's `.data`, not bare on stdout.
WIZARD_PAGE_ID = "pick-member"
WIZARD_PAGE_TITLE = "ATM Send-To"
WIZARD_PAGE_HTML = "pages/pick-member.html"


def fail(message: str) -> int:
    print(f"send-to picker: {message}", file=sys.stderr)
    return 1


def labels(rows: list[dict[str, Any]]) -> list[str]:
    return [str(row["label"]) for row in rows]


def selectable_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Rows a human may actually pick (PRD R4: dead/idle disabled).

    ``choose from list`` (macOS), zenity's ``--checklist``, and ``fzf`` have
    no notion of a disabled-but-visible row, so the only way to make a
    dead/idle member genuinely non-selectable is to omit it from the menu
    entirely; :func:`unavailable_rows` is what still surfaces them to the
    human, as a separate notice rather than a selectable choice.
    """
    return [row for row in rows if row["status"] == "active"]


def unavailable_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The complement of :func:`selectable_rows`: dead/idle members."""
    return [row for row in rows if row["status"] != "active"]


def notice_unavailable(rows: list[dict[str, Any]]) -> None:
    """Prints a one-line stderr notice naming any dead/idle members.

    This is the "separate unavailable line/notice" required by PRD R4 in
    place of a disabled-but-visible menu entry, which none of the supported
    picker backends can render.
    """
    excluded = unavailable_rows(rows)
    if not excluded:
        return
    names = ", ".join(labels(excluded))
    print(
        f"send-to picker: {len(excluded)} member(s) unavailable (dead/idle), excluded from selection: {names}",
        file=sys.stderr,
    )


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


def make_wizard_json(picker_input: Any) -> dict[str, Any]:
    """Wraps `picker_input` (PickerInput) as the wizard Command this adapter
    invokes Wyvern with -- `config` is the only channel a wizard page has for
    caller-supplied data (`WizardCommand::config`, "never inspected by the
    host"). See wyvern-pick-member-contract.md for the full invocation shape.
    """
    if not isinstance(picker_input, dict) or picker_input.get("schema_version") != SCHEMA_VERSION:
        raise ValueError("PickerInput schema_version is not supported")
    return {
        "type": "wizard",
        "page": {"id": WIZARD_PAGE_ID, "title": WIZARD_PAGE_TITLE, "html": WIZARD_PAGE_HTML},
        "config": picker_input,
    }


def unwrap_wizard_result(value: Any) -> Any:
    """Extracts and validates `PickerOutput` from a Wyvern `WizardResult`.

    A real Wyvern wizard finishes with `{"button": ..., "data": ...,
    "stack": [...]}` on stdout, not a bare `PickerOutput` object (this
    module's `pick-member.html` page emits `PickerOutput`-shaped JSON as its
    terminal page data, per `collectCurrentPageData()`). A non-`"finish"`
    button (cancel/dismiss) is treated exactly like a cancelled native
    picker: the caller falls back, never sends.
    """
    if not isinstance(value, dict):
        raise ValueError("Wyvern wizard result must be a JSON object")
    if value.get("button") != "finish":
        raise ValueError(f"Wyvern wizard result button was {value.get('button')!r}, not 'finish'")
    data = value.get("data")
    validate_output(data)
    return data


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--backend", choices=["fzf", "osascript", "zenity"])
    parser.add_argument("--validate", action="store_true")
    parser.add_argument(
        "--make-wizard-json",
        action="store_true",
        help="reads PickerInput on stdin, writes the wyvern wizard Command JSON (config=PickerInput) to stdout",
    )
    parser.add_argument(
        "--unwrap-wizard-result",
        action="store_true",
        help="reads a wyvern WizardResult on stdin, writes its validated .data (PickerOutput) to stdout",
    )
    args = parser.parse_args(argv[1:])
    try:
        value = json.load(sys.stdin)
        if args.validate:
            validate_output(value)
            json.dump(value, sys.stdout, separators=(",", ":"))
            sys.stdout.write("\n")
            return 0
        if args.make_wizard_json:
            wizard = make_wizard_json(value)
            json.dump(wizard, sys.stdout, separators=(",", ":"))
            sys.stdout.write("\n")
            return 0
        if args.unwrap_wizard_result:
            output = unwrap_wizard_result(value)
            json.dump(output, sys.stdout, separators=(",", ":"))
            sys.stdout.write("\n")
            return 0
        rows = []
        if not isinstance(value, dict) or value.get("schema_version") != SCHEMA_VERSION:
            raise ValueError("PickerInput schema_version is not supported")
        for team in value.get("teams", []):
            for member in team.get("members", []):
                rows.append(
                    {
                        "id": member["id"],
                        "status": member.get("status", "dead"),
                        "label": f"{team.get('name', team.get('id', '?'))} / {member.get('name', member['id'])} [{member.get('status', 'dead')}]",
                    }
                )
        # PRD R4: dead/idle members must be genuinely non-selectable, not
        # merely labeled. Notice them separately, then only ever offer
        # `active` rows to the selection paths below.
        notice_unavailable(rows)
        choices = selectable_rows(rows)
        selected = selected_by_environment(choices)
        if selected is None:
            backend = args.backend
            if backend is None:
                backend = "fzf"
            selected = external_selection(choices, backend)
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
