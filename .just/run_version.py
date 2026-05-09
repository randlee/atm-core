#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

from lint_common import render_table
from lint_common import workspace_crates


VERSION_MODES = {"current", "latest"}


def workspace_version(repo_root: Path) -> str:
    manifest = tomllib.loads((repo_root / "Cargo.toml").read_text(encoding="utf-8"))
    version = manifest.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str) or not version:
        raise SystemExit("workspace version missing from Cargo.toml")
    return version


def render_current(repo_root: Path) -> int:
    version = workspace_version(repo_root)
    crates = workspace_crates(repo_root)
    rows = [
        {
            "crate": crate.crate_dir,
            "package": crate.package_name,
            "crate_path": crate.crate_path_name,
            "manifest": crate.manifest_path,
            "workspace_version": version,
        }
        for crate in crates
    ]
    print(f"workspace_version: {version}")
    print("")
    print("workspace crates:")
    for line in render_table(
        rows,
        [
            ("crate", "crate"),
            ("package", "package"),
            ("crate_path", "crate_path"),
            ("manifest", "manifest"),
            ("workspace_version", "workspace_version"),
        ],
    ):
        print(line)
    return 0


def cargo_outdated_command(manifest_path: Path) -> list[str]:
    return [
        "cargo",
        "outdated",
        "--format",
        "json",
        "--manifest-path",
        str(manifest_path),
        "--root-deps-only",
    ]


def run_outdated(manifest_path: Path, repo_root: Path) -> dict:
    completed = subprocess.run(
        cargo_outdated_command(manifest_path),
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if completed.returncode != 0:
        raise SystemExit(completed.stderr.strip() or completed.stdout.strip())
    return json.loads(completed.stdout)


def render_latest(repo_root: Path) -> int:
    if shutil.which("cargo-outdated") is None:
        print("cargo-outdated is not installed; install it to run `just version latest`", file=sys.stderr)
        return 2

    crates = workspace_crates(repo_root)
    rows: list[dict[str, str]] = []
    for crate in crates:
        payload = run_outdated(repo_root / crate.manifest_path, repo_root)
        for dependency in payload.get("dependencies", []):
            if not isinstance(dependency, dict):
                continue
            rows.append(
                {
                    "crate": crate.crate_dir,
                    "package": crate.package_name,
                    "dependency": str(dependency.get("name", "")),
                    "project": str(dependency.get("project", "")),
                    "compat": str(dependency.get("compat", "")),
                    "latest": str(dependency.get("latest", "")),
                    "kind": str(dependency.get("kind", "")),
                    "platform": str(dependency.get("platform", "---") or "---"),
                }
            )

    if not rows:
        print("latest dependency review: no root dependency upgrades found")
        return 0

    print("latest dependency review:")
    for line in render_table(
        rows,
        [
            ("crate", "crate"),
            ("package", "package"),
            ("dependency", "dependency"),
            ("project", "project"),
            ("compat", "compat"),
            ("latest", "latest"),
            ("kind", "kind"),
            ("platform", "platform"),
        ],
    ):
        print(line)
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Inspect current or latest dependency version state.")
    parser.add_argument("mode", nargs="?", default="current")
    args = parser.parse_args(argv[1:])

    repo_root = Path(__file__).resolve().parent.parent
    if args.mode not in VERSION_MODES:
        valid = ", ".join(sorted(VERSION_MODES))
        print(f"unknown version mode: {args.mode}", file=sys.stderr)
        print(f"expected one of: {valid}", file=sys.stderr)
        return 2

    if args.mode == "current":
        return render_current(repo_root)
    return render_latest(repo_root)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
