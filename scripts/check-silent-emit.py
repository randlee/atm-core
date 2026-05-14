#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import argparse
import re
import sys


PATTERN = re.compile(
    r"let\s*_\s*=\s*.*?\.(?:emit|emit_event|emit_subsystem_event)\s*\(",
    re.DOTALL,
)


def discover_repo_root(explicit_root: str | None = None) -> Path:
    if explicit_root is not None:
        return Path(explicit_root).resolve()
    return Path(__file__).resolve().parent.parent


def workspace_manifest_paths(repo_root: Path) -> list[Path]:
    import tomllib

    root_manifest = tomllib.loads((repo_root / "Cargo.toml").read_text(encoding="utf-8"))
    workspace = root_manifest.get("workspace", {})
    if not isinstance(workspace, dict):
        return []

    members = workspace.get("members", [])
    if not isinstance(members, list) or not all(isinstance(item, str) for item in members):
        return []

    manifests: dict[Path, Path] = {}
    for pattern in members:
        for match in repo_root.glob(pattern):
            if match.is_dir():
                manifest_path = match / "Cargo.toml"
            elif match.is_file() and match.name == "Cargo.toml":
                manifest_path = match
            else:
                continue
            if manifest_path.exists():
                manifests[manifest_path.resolve()] = manifest_path
    return sorted(manifests.values())


def iter_workspace_rust_files(repo_root: Path) -> list[Path]:
    paths: list[Path] = []
    for manifest_path in workspace_manifest_paths(repo_root):
        crate_root = manifest_path.parent
        for directory_name in ("src", "tests"):
            directory = crate_root / directory_name
            if directory.exists():
                paths.extend(sorted(directory.rglob("*.rs")))
    return sorted(set(paths))


def is_test_only_path(path: Path) -> bool:
    if "tests" in path.parts:
        return True
    name = path.name
    return (
        name == "tests.rs"
        or name.startswith("test_")
        or name.endswith("_test.rs")
        or name.endswith("_tests.rs")
        or "test_support" in name
    )


def collect_findings(repo_root: Path) -> list[str]:
    findings: list[str] = []
    for rust_path in iter_workspace_rust_files(repo_root):
        relative_path = rust_path.relative_to(repo_root)
        if is_test_only_path(relative_path):
            continue
        text = rust_path.read_text(encoding="utf-8")
        for match in PATTERN.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            findings.append(
                f"{relative_path}:{line}: silent observability emit discard; use emit_or_warn/emit_event_or_warn"
            )
    return findings


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Enforce the no-silent-emit regression gate on workspace Rust sources.")
    parser.add_argument("--root", help="Explicit repo root.")
    args = parser.parse_args(argv[1:])

    repo_root = discover_repo_root(args.root)
    findings = collect_findings(repo_root)

    if findings:
        print("silent-emit failed")
        for finding in findings:
            print(finding)
        return 1

    print("silent-emit passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
