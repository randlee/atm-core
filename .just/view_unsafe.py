#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import argparse
import json
import subprocess
import sys

from lint_common import discover_repo_root
from lint_common import workspace_crates
from view_common import relative_artifact_path
from view_common import reset_view_dir
from view_common import write_json
from view_common import write_text


def geiger_command(manifest_path: Path, package_name: str, output_format: str) -> list[str]:
    return [
        "cargo",
        "geiger",
        "--package",
        package_name,
        "--manifest-path",
        str(manifest_path),
        "--all-targets",
        "--output-format",
        output_format,
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
    target_dir = reset_view_dir(repo_root, "unsafe")
    index_lines = ["Unsafe Surface View", ""]
    summary: list[dict[str, str]] = []

    for crate in workspace_crates(repo_root):
        crate_dir = target_dir / crate.crate_dir
        crate_dir.mkdir(parents=True, exist_ok=True)
        manifest_path = repo_root / crate.manifest_path

        text_result = run_command(geiger_command(manifest_path, crate.package_name, "Utf8"), repo_root)
        if text_result.returncode != 0:
            raise SystemExit(text_result.stderr.strip() or text_result.stdout.strip())
        json_result = run_command(geiger_command(manifest_path, crate.package_name, "Json"), repo_root)
        if json_result.returncode != 0:
            raise SystemExit(json_result.stderr.strip() or json_result.stdout.strip())

        text_path = crate_dir / "report.txt"
        json_path = crate_dir / "report.json"
        write_text(text_path, text_result.stdout)
        try:
            payload = json.loads(json_result.stdout)
        except Exception:
            payload = {"raw": json_result.stdout}
        write_json(json_path, payload)

        summary.append(
            {
                "crate": crate.crate_dir,
                "package": crate.package_name,
                "text": relative_artifact_path(repo_root, text_path),
                "json": relative_artifact_path(repo_root, json_path),
            }
        )
        index_lines.extend(
            [
                f"{crate.crate_dir}:",
                f"  text: {relative_artifact_path(repo_root, text_path)}",
                f"  json: {relative_artifact_path(repo_root, json_path)}",
                "",
            ]
        )

    index_path = target_dir / "index.txt"
    json_index_path = target_dir / "index.json"
    write_text(index_path, "\n".join(index_lines).rstrip() + "\n")
    write_json(json_index_path, {"crates": summary})
    print(f"unsafe view generated: {relative_artifact_path(repo_root, index_path)}")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Generate unsafe-surface artifacts.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
