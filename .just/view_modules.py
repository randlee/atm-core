#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import argparse
import shutil
import subprocess
import sys

from lint_common import discover_repo_root
from lint_common import workspace_crates
from lint_common import workspace_target_args
from view_common import relative_artifact_path
from view_common import reset_view_dir
from view_common import write_json
from view_common import write_text


def run_command(command: list[str], repo_root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )


def graphviz_svg_command(dot_path: Path, svg_path: Path) -> list[str]:
    return ["dot", "-Tsvg", str(dot_path), "-o", str(svg_path)]


def analysis_target_args(manifest_path: Path) -> list[str]:
    return workspace_target_args(manifest_path)


def run(repo_root: Path) -> int:
    target_dir = reset_view_dir(repo_root, "modules")
    index_lines = ["Module Architecture View", ""]
    summary: list[dict[str, str]] = []

    for crate in workspace_crates(repo_root):
        crate_dir = target_dir / crate.crate_dir
        crate_dir.mkdir(parents=True, exist_ok=True)
        manifest_path = repo_root / crate.manifest_path
        target_args = workspace_target_args(manifest_path)

        structure_cmd = [
            "cargo",
            "modules",
            "structure",
            "--package",
            crate.package_name,
            "--manifest-path",
            str(manifest_path),
            *target_args,
            "--no-fns",
        ]
        dependencies_cmd = [
            "cargo",
            "modules",
            "dependencies",
            "--package",
            crate.package_name,
            "--manifest-path",
            str(manifest_path),
            *target_args,
            "--no-fns",
            "--no-sysroot",
            "--layout",
            "dot",
        ]

        structure_result = run_command(structure_cmd, repo_root)
        if structure_result.returncode != 0:
            raise SystemExit(structure_result.stderr.strip() or structure_result.stdout.strip())
        dependencies_result = run_command(dependencies_cmd, repo_root)
        if dependencies_result.returncode != 0:
            raise SystemExit(dependencies_result.stderr.strip() or dependencies_result.stdout.strip())

        structure_path = crate_dir / "structure.txt"
        dependencies_path = crate_dir / "dependencies.dot"
        write_text(structure_path, structure_result.stdout)
        write_text(dependencies_path, dependencies_result.stdout)
        svg_path: Path | None = None
        if shutil.which("dot") is not None:
            svg_path = crate_dir / "dependencies.svg"
            svg_result = run_command(graphviz_svg_command(dependencies_path, svg_path), repo_root)
            if svg_result.returncode != 0:
                raise SystemExit(svg_result.stderr.strip() or svg_result.stdout.strip())

        item = {
            "crate": crate.crate_dir,
            "package": crate.package_name,
            "structure": relative_artifact_path(repo_root, structure_path),
            "dependencies": relative_artifact_path(repo_root, dependencies_path),
        }
        if svg_path is not None:
            item["svg"] = relative_artifact_path(repo_root, svg_path)
        summary.append(item)
        index_lines.extend(
            [
                f"{crate.crate_dir}:",
                f"  structure: {relative_artifact_path(repo_root, structure_path)}",
                f"  dependencies: {relative_artifact_path(repo_root, dependencies_path)}",
                *([f"  svg: {relative_artifact_path(repo_root, svg_path)}"] if svg_path is not None else []),
                "",
            ]
        )

    index_path = target_dir / "index.txt"
    json_path = target_dir / "index.json"
    write_text(index_path, "\n".join(index_lines).rstrip() + "\n")
    write_json(json_path, {"crates": summary})
    print(f"modules view generated: {relative_artifact_path(repo_root, index_path)}")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Generate module-structure artifacts.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
