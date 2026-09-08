#!/usr/bin/env python3
"""Every requirement compared to every change in every sprint (Rand, 2026-09-08).

Modes:
  req_ledger.py skeleton [--requirements docs/requirements.md]
      Print a JSON ledger with one row per `REQ-*` id found in requirements.md
      (id, line, verdict null). req-qa fills every verdict.
  req_ledger.py check <ledger.json> [--requirements docs/requirements.md]
      Exit 0 only if the ledger names every id exactly once, every verdict is
      one of untouched | compliant | violated, and every compliant/violated
      row cites at least one evidence ref. Exit 1 with the defects listed.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REQ_RE = re.compile(r"`(REQ-[A-Z0-9]+(?:-[A-Z0-9]+)*)`")
VERDICTS = {"untouched", "compliant", "violated"}


def requirement_ids(path: Path) -> dict[str, int]:
    ids: dict[str, int] = {}
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        for req in REQ_RE.findall(line):
            ids.setdefault(req, number)
    if not ids:
        sys.exit(f"req_ledger: no REQ-* ids found in {path}")
    return ids


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
    seen: dict[str, int] = {}
    for index, row in enumerate(rows):
        req = row.get("id")
        if req not in reqs:
            defects.append(f"row {index}: unknown id {req!r}")
            continue
        seen[req] = seen.get(req, 0) + 1
        verdict = row.get("verdict")
        if verdict not in VERDICTS:
            defects.append(f"{req}: verdict {verdict!r} not in {sorted(VERDICTS)}")
        elif verdict != "untouched" and not row.get("evidence_refs"):
            defects.append(f"{req}: verdict {verdict} without evidence_refs")
    for req in reqs:
        if req not in seen:
            defects.append(f"{req}: missing from ledger")
    for req, count in seen.items():
        if count > 1:
            defects.append(f"{req}: listed {count} times")
    return defects


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("mode", choices=["skeleton", "check"])
    parser.add_argument("ledger", nargs="?")
    parser.add_argument("--requirements", default="docs/requirements.md")
    args = parser.parse_args()
    reqs = requirement_ids(Path(args.requirements))
    if args.mode == "skeleton":
        json.dump(skeleton(reqs, Path(args.requirements)), sys.stdout, indent=2)
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
