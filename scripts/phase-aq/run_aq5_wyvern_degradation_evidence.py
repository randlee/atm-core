#!/usr/bin/env python3
"""Run the AQ5.2a six-case Wyvern degradation live-evidence scenario.

This drives the real `scripts/send-to/atm-send-to.sh` pipeline script through
every degradation case the sprint doc's deliverable 3a contract lists --
`wyvern` absent from `PATH`, below the pin, unparsable `--version`, a hanging
`--version` child, an unrecognized `PickerOutput.schema_version`, and a
missing page asset -- and asserts each one falls back to the native picker,
still completes a successful send, and surfaces a one-line stderr note (never
a failed send, never a blocked gesture, per PRD deliverable 3a).

Unlike the AQ1.9 restart matrix and AQ2.5 queue-delivery-trigger runners,
this scenario is deliberately **daemon-free**: the Wyvern degradation
contract lives entirely in `atm-send-to.sh`'s probe/fallback logic and
`picker.py`'s `PickerOutput` validation, both of which are exercised through
the script's existing test-only seams (`ATM_BIN`, `ATM_SEND_TO_WYVERN_BIN`,
`ATM_SEND_TO_WYVERN_ASSET`, `ATM_SEND_TO_NATIVE_PICKER`,
`ATM_SEND_TO_SELECTION`) -- the same seams `.just/tests/test_send_to_surface.py
::test_wyvern_degradation_cases_fall_back_and_still_send` uses. A scripted
stub `atm` binary (not the real daemon-backed CLI) stands in for `atm teams`
/`atm send` so this harness proves the pipeline's degradation behavior
without requiring a live, owned `atm-daemon` -- there is nothing daemon-shaped
in the contract this scenario is evidencing. `--daemon` is accepted only for
parity with the other Phase AQ live-evidence harnesses' CLI surface (see
`.github/workflows/phase-aq-evidence.yml`); it is not invoked.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "send-to" / "atm-send-to.sh"

CASES = ("absent", "below", "unparsable", "hang", "missing-asset", "unknown-schema")

PICKER_INPUT: dict[str, Any] = {
    "schema_version": 1,
    "teams": [
        {
            "id": "atm-dev",
            "name": "atm-dev",
            "members": [
                {
                    "id": "cipher@atm-dev",
                    "name": "cipher",
                    "host": "m4",
                    "cwd": "/work",
                    "status": "active",
                },
            ],
        }
    ],
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="local", help="evidence host label, for example local or clean-runner-<os>")
    parser.add_argument(
        "--evidence-dir",
        type=Path,
        default=None,
        help="directory for the JSON and Markdown evidence files",
    )
    parser.add_argument(
        "--daemon",
        type=Path,
        default=Path(os.environ.get("ATM_DAEMON_BIN", ROOT / "target" / "debug" / "atm-daemon")),
        help="accepted for CLI-surface parity with the other Phase AQ live-evidence harnesses; unused (this scenario is daemon-free, see module docstring)",
    )
    parser.add_argument(
        "--atm",
        type=Path,
        default=Path(os.environ.get("ATM_BIN", ROOT / "target" / "debug" / "atm")),
        help="accepted for CLI-surface parity; unused (a scripted stub `atm` stands in, see module docstring)",
    )
    return parser.parse_args()


def executable(path: Path) -> None:
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def make_stub_atm(directory: Path, input_json: Path, send_log: Path) -> Path:
    path = directory / "atm"
    path.write_text(
        "#!/bin/sh\n"
        "if [ \"$1\" = teams ]; then\n"
        f"  cat {input_json}\n"
        "elif [ \"$1\" = send ]; then\n"
        f"  cat > {send_log}\n"
        "  echo send-called >&2\n"
        "fi\n"
    )
    executable(path)
    return path


def make_native_picker(directory: Path) -> Path:
    path = directory / "native-picker"
    path.write_text(
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' "
        "'{\"schema_version\":1,\"recipients\":[\"cipher@atm-dev\"]}'\n"
    )
    executable(path)
    return path


def make_wyvern_stub(directory: Path, case: str) -> Path:
    path = directory / f"wyvern-{case}"
    if case == "absent":
        # Never written: absent-from-PATH is simulated by pointing
        # ATM_SEND_TO_WYVERN_BIN at a path that does not exist.
        return path
    if case == "below":
        version_body = "printf 'wyvern 0.4.0\\n'\n"
    elif case == "unparsable":
        version_body = "printf 'wyvern development\\n'\n"
    elif case == "hang":
        # Comfortably exceeds probe_wyvern.py's 1.5s bounded deadline;
        # subprocess's own timeout kills this before completion.
        version_body = "sleep 30\n"
    else:
        version_body = "printf 'wyvern 0.5.0\\n'\n"
    # A real Wyvern wizard's terminal stdout is the full WizardResult
    # envelope (`{"button":"finish","data":<PickerOutput>,"stack":[...]}`),
    # not a bare PickerOutput object -- this stub mirrors that exact shape
    # (docs/plans/phase-aq/fixtures/wyvern-pick-member-contract.md).
    wizard_result = (
        '{"button":"finish","data":{"schema_version":99,"recipients":["cipher@atm-dev"]},"stack":[]}'
        if case == "unknown-schema"
        else '{"button":"finish","data":{"schema_version":1,"recipients":["cipher@atm-dev"]},"stack":[]}'
    )
    path.write_text(
        f"#!/bin/sh\nif [ \"$1\" = --version ]; then\n{version_body}"
        f"else\nprintf '%s\\n' '{wizard_result}'\nfi\n"
    )
    executable(path)
    return path


def run_case(case: str, directory: Path) -> dict[str, Any]:
    input_json = directory / "input.json"
    input_json.write_text(json.dumps(PICKER_INPUT))
    send_log = directory / f"send-{case}.log"
    stub_atm = make_stub_atm(directory, input_json, send_log)
    native_picker = make_native_picker(directory)
    asset = directory / "pick-member.html"
    asset.write_text("external Wyvern asset")
    wyvern = make_wyvern_stub(directory, case)
    one_file = directory / "one.txt"
    one_file.write_text("evidence attachment")

    env = os.environ.copy()
    env["ATM_BIN"] = str(stub_atm)
    env["ATM_SEND_TO_WYVERN_BIN"] = str(wyvern)
    env["ATM_SEND_TO_WYVERN_ASSET"] = str(asset if case != "missing-asset" else directory / "absent.html")
    env["ATM_SEND_TO_NATIVE_PICKER"] = str(native_picker)
    env["ATM_SEND_TO_SELECTION"] = "cipher@atm-dev"
    env.pop("ATM_SEND_TO_PICKER", None)

    completed = subprocess.run(
        [str(SCRIPT), str(one_file)],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    send_happened = send_log.exists()
    fell_back_with_notice = "Wyvern" in completed.stderr
    passed = completed.returncode == 0 and send_happened and fell_back_with_notice
    return {
        "case": case,
        "returncode": completed.returncode,
        "send_called": send_happened,
        "stderr_notes_wyvern_fallback": fell_back_with_notice,
        "stderr": completed.stderr.strip(),
        "pass": passed,
    }


def write_evidence(args: argparse.Namespace, results: list[dict[str, Any]]) -> tuple[Path, Path]:
    evidence_dir = args.evidence_dir or ROOT / "docs" / "plans" / "phase-aq" / "evidence" / "AQ5"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    json_path = evidence_dir / f"wyvern-degradation-{args.host}.json"
    markdown_path = evidence_dir / f"wyvern-degradation-{args.host}.md"

    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout.strip()
    overall_pass = all(result["pass"] for result in results)
    payload = {
        "schema_version": 1,
        "sprint": "AQ5.2a",
        "host": args.host,
        "commit": commit,
        "overall_pass": overall_pass,
        "cases": results,
    }
    json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    lines = [
        "# AQ5.2a Wyvern degradation matrix evidence",
        "",
        f"Host: `{args.host}`",
        f"Commit: `{commit}`",
        f"Status: **{'PASS' if overall_pass else 'FAIL'}**",
        "",
        "Every case must fall back to the native picker, still complete a",
        "successful send, and note the fallback on stderr (PRD deliverable",
        "3a / AC2a). No case in this matrix requires Wyvern to be installed.",
        "",
        "| Case | Exit code | Send called | Fallback noted on stderr | Pass |",
        "| --- | --- | --- | --- | --- |",
    ]
    for result in results:
        lines.append(
            f"| {result['case']} | {result['returncode']} | {result['send_called']} | "
            f"{result['stderr_notes_wyvern_fallback']} | {result['pass']} |"
        )
    markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path, markdown_path


def main() -> int:
    args = parse_args()
    if not SCRIPT.is_file():
        raise SystemExit(f"pipeline script does not exist: {SCRIPT}")
    results = []
    with tempfile.TemporaryDirectory() as raw:
        directory = Path(raw)
        for case in CASES:
            results.append(run_case(case, directory))

    json_path, markdown_path = write_evidence(args, results)
    overall_pass = all(result["pass"] for result in results)
    print(f"{'PASS' if overall_pass else 'FAIL'} AQ5.2a Wyvern degradation evidence: {json_path}")
    print(f"transcript: {markdown_path}")
    return 0 if overall_pass else 1


if __name__ == "__main__":
    raise SystemExit(main())
