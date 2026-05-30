#!/usr/bin/env python3
from __future__ import annotations

from collections import defaultdict
from pathlib import Path
import argparse
import sys

from lint_boundaries import collect_boundary_violations
from lint_boundaries import parse_boundary_records
from lint_common import discover_repo_root
from lint_common import render_table
from view_common import relative_artifact_path
from view_common import reset_view_dir
from view_common import write_json
from view_common import write_text


def boundary_doc_rows(repo_root: Path, records) -> list[dict[str, str]]:
    counts: dict[str, dict[str, int]] = defaultdict(lambda: {"records": 0, "active": 0, "planned": 0, "retired": 0})
    for record in records:
        doc = record.source_path.as_posix()
        counts[doc]["records"] += 1
        if record.status_state == "unix_implemented_windows_pending":
            counts[doc]["active"] += 1
        else:
            counts[doc][record.status_state] += 1

    rows: list[dict[str, str]] = []
    for doc in sorted(counts):
        summary = counts[doc]
        rows.append(
            {
                "doc": doc,
                "records": str(summary["records"]),
                "active": str(summary["active"]),
                "planned": str(summary["planned"]),
                "retired": str(summary["retired"]),
            }
        )
    return rows


def render_summary_text(repo_root: Path, rows: list[dict[str, str]], violation_count: int) -> str:
    lines = [
        "Boundary Architecture View",
        "",
        f"docs analyzed: {len(rows)}",
        f"violations: {violation_count}",
        "",
        "boundary docs:",
    ]
    lines.extend(
        render_table(
            rows,
            [
                ("doc", "doc"),
                ("records", "records"),
                ("active", "active"),
                ("planned", "planned"),
                ("retired", "retired"),
            ],
        )
    )
    lines.append("")
    return "\n".join(lines) + "\n"


def run(repo_root: Path) -> int:
    target_dir = reset_view_dir(repo_root, "boundaries")
    records, _parse_violations = parse_boundary_records(repo_root)
    violations = collect_boundary_violations(repo_root)
    rows = boundary_doc_rows(repo_root, records)

    summary_path = target_dir / "summary.txt"
    json_path = target_dir / "summary.json"
    findings_path = target_dir / "findings.txt"

    write_text(summary_path, render_summary_text(repo_root, rows, len(violations)))
    write_json(
        json_path,
        {
            "docs": rows,
            "record_count": len(records),
            "violation_count": len(violations),
        },
    )
    write_text(findings_path, "\n".join(violation.render() for violation in violations) + ("\n" if violations else ""))

    print(f"boundaries view generated: {relative_artifact_path(repo_root, summary_path)}")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Generate boundary architecture artifacts.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
