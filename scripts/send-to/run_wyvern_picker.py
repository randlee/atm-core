#!/usr/bin/env python3
"""Run the real Wyvern wizard CLI and expose only the PickerOutput payload."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from probe_wyvern import main as probe_main


def fail(message: str) -> int:
    print(f"send-to: Wyvern picker failed: {message}", file=sys.stderr)
    return 1


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pin", required=True)
    parser.add_argument("--asset", required=True)
    args = parser.parse_args(argv[1:])
    asset = Path(args.asset)
    if not asset.is_file():
        return fail(f"picker asset is missing: {asset}")
    try:
        picker_input = json.load(sys.stdin)
    except (OSError, json.JSONDecodeError) as error:
        return fail(f"PickerInput is not valid JSON: {error}")
    if not isinstance(picker_input, dict):
        return fail("PickerInput must be a JSON object")

    probe_exit = probe_main(
        [
            "probe_wyvern.py",
            "--pin",
            args.pin,
            "--asset",
            str(asset),
        ]
    )
    if probe_exit != 0:
        return probe_exit

    wizard = {
        "type": "wizard",
        "page": {
            "id": "pick-member",
            "title": "ATM Send-To",
            "html": asset.name,
        },
        "config": picker_input,
    }
    binary = os.environ.get("ATM_SEND_TO_WYVERN_BIN", "wyvern")
    with tempfile.TemporaryDirectory(prefix="atm-wyvern-") as temporary:
        wizard_path = Path(temporary) / "wizard.json"
        wizard_path.write_text(json.dumps(wizard), encoding="utf-8")
        try:
            completed = subprocess.run(
                [binary, str(wizard_path), "--ui-root", str(asset.parent)],
                capture_output=True,
                text=True,
                check=False,
                env={**os.environ, "WYVERN_LOG": "off"},
            )
        except (OSError, subprocess.SubprocessError, ValueError) as error:
            return fail(str(error))
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "no diagnostic output"
        return fail(f"wizard exited {completed.returncode}: {detail}")
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        return fail(f"wizard returned invalid JSON instead of WizardResult: {error}")
    if not isinstance(result, dict):
        return fail("wizard returned a non-object result")
    if result.get("button") != "finish":
        return fail(f"selection cancelled ({result.get('button', 'unknown')} button)")
    picker_output = result.get("data")
    if not isinstance(picker_output, dict):
        return fail("WizardResult.data is not a PickerOutput object")
    json.dump(picker_output, sys.stdout, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
