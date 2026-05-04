#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import argparse
import sys

from check_line_counts import collect_file_counts
from check_line_counts import crate_totals
from check_line_counts import format_table
from check_line_counts import limit_summary
from check_line_counts import load_config
from lint_common import discover_repo_root
from view_common import relative_artifact_path
from view_common import reset_view_dir
from view_common import write_json
from view_common import write_text


def run(repo_root: Path) -> int:
    target_dir = reset_view_dir(repo_root, "lines")
    config = load_config(repo_root)
    counts = collect_file_counts(repo_root, config)
    totals = crate_totals(counts)
    rows = [
        {
            "crate": count.crate_name,
            "crate_root": count.crate_root,
            "file": Path(count.path).relative_to(count.crate_root).as_posix(),
            "total": count.total_lines,
            "prod": count.production_lines,
            "test": count.test_lines,
            "prod_test": count.scoped_code_lines,
        }
        for count in counts
    ]
    totals_rows = [
        {
            "crate": total.crate_name,
            "total": total.total_lines,
            "prod": total.production_lines,
            "test": total.test_lines,
            "prod_test": total.scoped_code_lines,
        }
        for total in totals
    ]
    summary_path = target_dir / "summary.txt"
    table_path = target_dir / "table.txt"
    json_path = target_dir / "summary.json"

    write_text(
        summary_path,
        "\n".join(
            [
                "Line Count Architecture View",
                "",
                f"active limits: {limit_summary(config)}",
                f"crates analyzed: {len(totals_rows)}",
                f"files analyzed: {len(rows)}",
                "",
            ]
        ),
    )
    write_text(table_path, "\n".join(format_table(counts)) + "\n")
    write_json(
        json_path,
        {
            "limits": {
                "summary": limit_summary(config),
                "max_total_lines": config.max_total_lines,
                "max_production_lines": config.max_production_lines,
                "max_scoped_code_lines": config.max_scoped_code_lines,
                "exclusions": config.exclusions or {},
            },
            "files": rows,
            "crate_totals": totals_rows,
        },
    )

    print(f"lines view generated: {relative_artifact_path(repo_root, summary_path)}")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Generate line-count architecture artifacts.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
