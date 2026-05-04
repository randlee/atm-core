#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import argparse
import subprocess
import sys

from lint_common import discover_repo_root
from view_common import relative_artifact_path
from view_common import reset_view_dir
from view_common import write_json
from view_common import write_text


def visualize_command(output_path: Path, repo_root: Path) -> list[str]:
    return [
        "cargo",
        "dep-insight",
        "visualize",
        "--no-open",
        "--out",
        str(output_path),
        str(repo_root),
    ]


def analyze_command(html_path: Path, json_path: Path, repo_root: Path) -> list[str]:
    return [
        "cargo",
        "dep-insight",
        "analyze",
        "--html",
        str(html_path),
        "--json",
        str(json_path),
        str(repo_root),
    ]


def run_command(command: list[str], repo_root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )


def run(repo_root: Path) -> int:
    target_dir = reset_view_dir(repo_root, "deps")
    index_path = target_dir / "index.html"
    report_path = target_dir / "report.html"
    json_path = target_dir / "report.json"

    visualize_result = run_command(visualize_command(index_path, repo_root), repo_root)
    if visualize_result.returncode != 0:
        raise SystemExit(visualize_result.stderr.strip() or visualize_result.stdout.strip())

    analyze_result = run_command(analyze_command(report_path, json_path, repo_root), repo_root)
    if analyze_result.returncode != 0:
        raise SystemExit(analyze_result.stderr.strip() or analyze_result.stdout.strip())

    summary_path = target_dir / "summary.txt"
    summary_json_path = target_dir / "summary.json"
    lines = [
        "Dependency Architecture View",
        "",
        f"graph_html: {relative_artifact_path(repo_root, index_path)}",
        f"report_html: {relative_artifact_path(repo_root, report_path)}",
        f"report_json: {relative_artifact_path(repo_root, json_path)}",
        "",
    ]
    write_text(summary_path, "\n".join(lines))
    write_json(
        summary_json_path,
        {
            "graph_html": relative_artifact_path(repo_root, index_path),
            "report_html": relative_artifact_path(repo_root, report_path),
            "report_json": relative_artifact_path(repo_root, json_path),
        },
    )
    print(f"deps view generated: {relative_artifact_path(repo_root, index_path)}")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Generate crate-dependency visualization artifacts.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
