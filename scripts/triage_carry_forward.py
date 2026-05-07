#!/usr/bin/env python3
"""Build carry-forward finding payloads from triage Turtle records."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

QUERY = """
PREFIX triage: <urn:atm:triage:>

SELECT ?finding_id ?category ?severity ?summary ?file ?line
WHERE {
  ?finding a triage:Finding ;
           triage:findingId ?finding_id ;
           triage:title ?summary ;
           triage:hasOccurrence ?occ .
  OPTIONAL { ?finding triage:category ?category . }
  OPTIONAL { ?finding triage:severity ?severity . }
  ?occ a triage:Occurrence ;
       triage:file ?file ;
       triage:status ?occ_status ;
       triage:branch ?branch .
  OPTIONAL { ?occ triage:line ?line . }
  FILTER(?branch = %BRANCH%)
  FILTER(?occ_status = "open")
}
ORDER BY ?finding_id ?file ?line
"""


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Render the branch-specific carry_forward_findings_json payload from "
            "one or more triage Turtle records."
        )
    )
    parser.add_argument(
        "--branch",
        required=True,
        help="Branch name to extract open occurrences for, e.g. R.17 or feature/pR-s17-watch-reconcile.",
    )
    parser.add_argument(
        "--ttl",
        dest="ttl_files",
        action="append",
        required=True,
        help="Path to a triage Turtle record. Repeat for multiple findings.",
    )
    parser.add_argument(
        "--pretty",
        action="store_true",
        help="Pretty-print the JSON array instead of emitting a compact string.",
    )
    return parser.parse_args(argv)


def require_oxigraph() -> None:
    if shutil.which("oxigraph") is None:
        raise SystemExit("oxigraph is required but was not found in PATH")


def validate_files(paths: list[Path]) -> list[Path]:
    validated: list[Path] = []
    for path in paths:
        if not path.is_file():
            raise SystemExit(f"missing triage record: {path}")
        validated.append(path.resolve())
    return validated


def load_records(store_dir: Path, ttl_files: list[Path]) -> None:
    cmd = [
        "oxigraph",
        "load",
        "--location",
        str(store_dir),
        "--file",
        *[str(path) for path in ttl_files],
    ]
    result = subprocess.run(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(result.stderr.strip() or "oxigraph load failed")


def query_records(store_dir: Path, branch: str) -> list[dict[str, object]]:
    cmd = [
        "oxigraph",
        "query",
        "--location",
        str(store_dir),
        "--query",
        QUERY.replace("%BRANCH%", json.dumps(branch)),
        "--results-format",
        "application/sparql-results+json",
    ]
    result = subprocess.run(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(result.stderr.strip() or "oxigraph query failed")

    data = json.loads(result.stdout)
    rows: list[dict[str, object]] = []
    for binding in data.get("results", {}).get("bindings", []):
        line_term = binding.get("line")
        rows.append(
            {
                "id": binding["finding_id"]["value"],
                "category": binding.get("category", {}).get("value"),
                "severity": binding.get("severity", {}).get("value"),
                "file": binding["file"]["value"],
                "line": int(line_term["value"]) if line_term else None,
                "summary": binding["summary"]["value"],
            }
        )
    return rows


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    require_oxigraph()
    ttl_files = validate_files([Path(path) for path in args.ttl_files])

    with tempfile.TemporaryDirectory(prefix="atm-triage-") as tmpdir:
        store_dir = Path(tmpdir) / "store"
        store_dir.mkdir(parents=True, exist_ok=True)
        load_records(store_dir, ttl_files)
        rows = query_records(store_dir, args.branch)

    if args.pretty:
        print(json.dumps(rows, indent=2))
    else:
        print(json.dumps(rows, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    sys.exit(main())
