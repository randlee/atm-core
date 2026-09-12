#!/usr/bin/env python3
"""Check that every requirement is compared to every change (Rand, 2026-09-08).

Modes:
  req_ledger.py skeleton [--requirements docs/requirements.md]
      Print a JSON ledger with one row per exact, backtick-delimited `REQ-*`
      id found in requirements.md. req-qa fills every verdict.
  req_ledger.py check <ledger.json> [--requirements docs/requirements.md]
      Exit 0 only if the ledger names every id exactly once, every verdict is
      one of untouched | compliant | violated, and every judged row cites
      evidence and a non-empty note. Exit 1 with defects listed.
  req_ledger.py ids-check [--requirements docs/requirements.md]
      Verify every REQ-* mention in requirements.md is backtick-delimited.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REQ_RE = re.compile(r"`(REQ-[A-Z0-9]+(?:-[A-Z0-9]+)*)`")
REQ_MENTION_RE = re.compile(
    r"(?<![A-Z0-9_-])REQ-[A-Z0-9]+(?:-[A-Z0-9]+)*(?:-\*)?(?![A-Z0-9_-])"
)
VERDICTS = {"untouched", "compliant", "violated"}


def requirement_ids(path: Path) -> dict[str, int]:
    ids: dict[str, int] = {}
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        for req in REQ_RE.findall(line):
            ids.setdefault(req, number)
    if not ids:
        sys.exit(f"req_ledger: no REQ-* ids found in {path}")
    return ids


def ids_check(path: Path) -> list[str]:
    defects: list[str] = []
    text = path.read_text(encoding="utf-8")
    for number, line in enumerate(text.splitlines(keepends=True), start=1):
        for match in REQ_MENTION_RE.finditer(line):
            token = match.group(0)
            before = line[match.start() - 1] if match.start() else ""
            after = line[match.end()] if match.end() < len(line) else ""
            if before != "`" or after != "`":
                defects.append(f"line {number}: {token} is not enclosed in backticks")
    return defects


def skeleton(reqs: dict[str, int], requirements: Path) -> dict:
    return {
        "requirements_source": str(requirements),
        "requirement_count": len(reqs),
        "rows": [
            {"id": req, "line": line, "verdict": None, "evidence_refs": [], "note": ""}
            for req, line in sorted(reqs.items(), key=lambda item: item[1])
        ],
    }


def check(ledger_path: Path, reqs: dict[str, int]) -> list[str]:
    defects: list[str] = []
    try:
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
        rows = ledger["rows"]
    except (OSError, ValueError, KeyError, TypeError) as error:
        return [f"ledger unreadable: {error}"]
    if not isinstance(rows, list):
        return ["rows must be a list"]
    seen: dict[str, int] = {}
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            defects.append(f"row {index}: expected an object")
            continue
        req = row.get("id")
        if req not in reqs:
            defects.append(f"row {index}: unknown id {req!r}")
            continue
        seen[req] = seen.get(req, 0) + 1
        verdict = row.get("verdict")
        if verdict not in VERDICTS:
            defects.append(f"{req}: verdict {verdict!r} not in {sorted(VERDICTS)}")
        elif verdict != "untouched":
            if not row.get("evidence_refs"):
                defects.append(f"{req}: verdict {verdict} without evidence_refs")
            if not isinstance(row.get("note"), str) or not row["note"].strip():
                defects.append(f"{req}: verdict {verdict} without a non-empty note")
    for req in reqs:
        if req not in seen:
            defects.append(f"{req}: missing from ledger")
    for req, count in seen.items():
        if count > 1:
            defects.append(f"{req}: listed {count} times")
    return defects


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("mode", choices=["skeleton", "check", "ids-check"])
    parser.add_argument("ledger", nargs="?")
    parser.add_argument("--requirements", default="docs/requirements.md")
    args = parser.parse_args()
    requirements = Path(args.requirements)
    if args.mode == "ids-check":
        defects = ids_check(requirements)
        if defects:
            print(f"req_ledger ids: FAIL ({len(defects)} defects)")
            for defect in defects:
                print(f"  - {defect}")
            sys.exit(1)
        reqs = requirement_ids(requirements)
        print(f"req_ledger ids: PASS ({len(reqs)} ids; every mention is backticked)")
        return
    reqs = requirement_ids(requirements)
    if args.mode == "skeleton":
        json.dump(skeleton(reqs, requirements), sys.stdout, indent=2)
        print()
        return
    if not args.ledger:
        parser.error("check needs a ledger path")
    defects = check(Path(args.ledger), reqs)
    if defects:
        print(f"req_ledger: FAIL ({len(defects)} defects, {len(reqs)} requirements)")
        for defect in defects:
            print(f"  - {defect}")
        sys.exit(1)
    print(f"req_ledger: PASS ({len(reqs)} requirements, every one judged)")


if __name__ == "__main__":
    main()
