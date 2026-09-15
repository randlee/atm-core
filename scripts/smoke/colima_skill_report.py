#!/usr/bin/env python3
"""Convert one testbed skill run into a colima integration step payload."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import shutil
import sys
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from scripts.report_runtime import source_revision as _git_source_revision  # noqa: E402

FEATURE = "colima-hermes-skills"
PLATFORM = "linux"
HOST = "hermes-testbed"
REPO_ROOT = Path(__file__).resolve().parents[2]
RAW_FILES = ("result.txt", "herdr-doctor.json")
STEP = re.compile(r"^\s+\d+ (PASS|FAIL) (.*)$")
FIELD = re.compile(r"^(skill|agent|result): (.*)$", re.MULTILINE)
ATM_LINE = re.compile(r"^atm:\s+(\S+)", re.MULTILINE)
TRANSPORT_LINE = re.compile(r"^herdr transport: (\S+ \([^)]*\))", re.MULTILINE)
REF_LINE = re.compile(r"@ ([0-9a-f]{7,40})")


def case(name: str, status: str, detail: str, host: str = HOST) -> dict[str, Any]:
    return {"name": name, "status": status, "detail": detail, "origin": host, "destination": host, "attempt": 1}


def parse_report(text: str) -> list[dict[str, Any]]:
    """One case per skill step; the skill and the reporting agent prefix the step name."""
    fields = dict(FIELD.findall(text))
    label = f"{fields['skill']} / {fields['agent'].split()[0].split('@')[0]}"
    cases = []
    for line in text.splitlines():
        match = STEP.match(line)
        if match:
            name, _, detail = match.group(2).partition(" — ")
            cases.append(case(f"{label}: {name}", match.group(1), detail or name))
    return cases


def header_cases(result_text: str, report_cases: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The preflight rows the pane header shows: version plus herdr transport, and the fixture address."""
    version = ATM_LINE.search(result_text)
    transport = TRANSPORT_LINE.search(result_text)
    doctor_steps = [item for item in report_cases if item["name"].endswith(": Doctor passes")]
    status = "PASS" if doctor_steps and all(item["status"] == "PASS" for item in doctor_steps) else "FAIL"
    observed = transport.group(1) if transport else "not recorded"
    revision = REF_LINE.search(result_text)
    return [
        case("doctor", status, f"ATM {version.group(1) if version else 'unknown'}"),
        case("advertised host", "PASS", "127.0.0.1 (sentences enter by docker exec; --no-peer)"),
        case("herdr transport (atm doctor, fixture daemon)", "PASS" if observed.startswith("socket") else "FAIL", observed),
        case("testbed ref", "PASS" if revision else "FAIL", revision.group(1) if revision else "not recorded"),
    ]


def run_step(run_dir: Path, step_dir: Path) -> dict[str, Any]:
    result_text = (run_dir / "result.txt").read_text(encoding="utf-8")
    reports = sorted(run_dir.glob("report-*.txt"), key=lambda path: int(path.stem.split("-")[1]))
    report_cases = [item for path in reports for item in parse_report(path.read_text(encoding="utf-8"))]
    cases = header_cases(result_text, report_cases) + report_cases
    status = "PASS" if result_text.startswith("PASS") else "FAIL"
    run_id = run_dir.name.removesuffix(f"-{FEATURE}")
    step_dir.mkdir(parents=True, exist_ok=True)
    for path in [*reports, *(run_dir / name for name in RAW_FILES)]:
        if path.is_file():
            shutil.copy2(path, step_dir / path.name)
    source_revision = _source_revision()
    payload = {"feature": FEATURE, "host": HOST, "platform": PLATFORM, "run_id": run_id, "status": status, "source_revision": source_revision, "cases": cases}
    (step_dir / "step.json").write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return payload


def _source_revision() -> str | None:
    return _git_source_revision(REPO_ROOT)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("run_dir", type=Path, help="test.sh run directory or committed evidence directory")
    parser.add_argument("--out", type=Path, required=True, help="driver-owned step directory")
    args = parser.parse_args()
    run_dir = args.run_dir.resolve()
    payload = run_step(run_dir, args.out.resolve())
    print(f"colima hermes skills: {payload['status']}")
    return 0 if payload["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
