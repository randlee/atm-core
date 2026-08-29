#!/usr/bin/env python3
"""Create and publish a patch-bumped prerelease tag on the current branch."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path

from lint_common import discover_repo_root
from lint_common import workspace_manifest_paths


STABLE_VERSION = re.compile(r"^(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)$")


def git(repo_root: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        ["git", *args],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SystemExit(f"git {' '.join(args)} failed: {detail}")
    return result


def workspace_version(repo_root: Path) -> str:
    manifest = tomllib.loads((repo_root / "Cargo.toml").read_text(encoding="utf-8"))
    version = manifest.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str) or STABLE_VERSION.fullmatch(version) is None:
        raise SystemExit("workspace version must be a stable X.Y.Z value")
    return version


def patch_bump(version: str) -> str:
    match = STABLE_VERSION.fullmatch(version)
    if match is None:
        raise ValueError(f"workspace version must be a stable X.Y.Z value: {version!r}")
    return (
        f"{match.group('major')}.{match.group('minor')}."
        f"{int(match.group('patch')) + 1}"
    )


def current_branch(repo_root: Path) -> str:
    branch = git(repo_root, "branch", "--show-current").stdout.strip()
    if not branch:
        raise SystemExit("prerelease-tag refuses to run from a detached HEAD")
    if branch in {"develop", "main"}:
        raise SystemExit(f"prerelease-tag refuses to run on protected branch {branch}")
    return branch


def require_clean_tree(repo_root: Path) -> None:
    if git(repo_root, "status", "--porcelain").stdout:
        raise SystemExit("prerelease-tag requires a clean working tree")


def workspace_package_names(manifests: list[Path]) -> set[str]:
    names: set[str] = set()
    for manifest_path in manifests:
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        package = manifest.get("package", {})
        name = package.get("name") if isinstance(package, dict) else None
        if isinstance(name, str) and name:
            names.add(name)
    if not names:
        raise SystemExit("no workspace package manifests found")
    return names


def replace_package_version(text: str, section: str, old: str, new: str) -> tuple[str, bool]:
    section_re = re.compile(
        rf"(?ms)^(\[{re.escape(section)}\]\n)(.*?)(?=^\[|\Z)"
    )
    match = section_re.search(text)
    if match is None:
        return text, False
    body = match.group(2)
    version_re = re.compile(rf'^(\s*version\s*=\s*)"{re.escape(old)}"(\s*(?:#.*)?)$', re.MULTILINE)
    updated, count = version_re.subn(rf'\1"{new}"\2', body, count=1)
    if count == 0:
        return text, False
    return text[: match.start(2)] + updated + text[match.end(2) :], True


def update_manifest_versions(repo_root: Path, old: str, new: str) -> list[Path]:
    manifests = [repo_root / "Cargo.toml", *workspace_manifest_paths(repo_root)]
    if not manifests:
        raise SystemExit("no workspace manifests found")
    changed: list[Path] = []
    for manifest_path in manifests:
        before = manifest_path.read_text(encoding="utf-8")
        after, _did_change = replace_package_version(
            before,
            "workspace.package" if manifest_path == repo_root / "Cargo.toml" else "package",
            old,
            new,
        )
        lines = []
        for line in after.splitlines(keepends=True):
            if "path =" in line:
                line = line.replace(f'version = "{old}"', f'version = "{new}"')
            lines.append(line)
        after = "".join(lines)
        if after != before:
            manifest_path.write_text(after, encoding="utf-8")
            changed.append(manifest_path)

    if repo_root / "Cargo.toml" not in changed:
        raise SystemExit("workspace package version was not found in Cargo.toml")
    return changed


def update_lockfile(repo_root: Path, old: str, new: str, package_names: set[str]) -> None:
    path = repo_root / "Cargo.lock"
    text = path.read_text(encoding="utf-8")
    blocks = text.split("[[package]]")
    found: set[str] = set()
    updated_blocks: list[str] = [blocks[0]]
    for block in blocks[1:]:
        name_match = re.search(r'^name = "([^"]+)"$', block, re.MULTILINE)
        if name_match is not None and name_match.group(1) in package_names:
            version_re = re.compile(rf'^(version = )"{re.escape(old)}"$', re.MULTILINE)
            block, count = version_re.subn(r'\1"' + new + '"', block, count=1)
            if count != 1:
                raise SystemExit(f"Cargo.lock version for {name_match.group(1)} is not {old}")
            found.add(name_match.group(1))
        updated_blocks.append(block)
    missing = sorted(package_names - found)
    if missing:
        raise SystemExit(f"Cargo.lock is missing workspace package(s): {', '.join(missing)}")
    path.write_text("[[package]]".join(updated_blocks), encoding="utf-8")


def verify_lockstep(repo_root: Path) -> None:
    result = subprocess.run(
        [sys.executable, str(repo_root / ".just" / "check_version_sync.py")],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit((result.stderr or result.stdout).strip() or "version lockstep verification failed")


def remote_tag_exists(repo_root: Path, tag: str) -> bool:
    result = git(
        repo_root,
        "ls-remote",
        "--exit-code",
        "--refs",
        "origin",
        f"refs/tags/{tag}",
        check=False,
    )
    return result.returncode == 0


def describe_plan(branch: str, old: str, new: str) -> None:
    print("prerelease-tag dry run:")
    print(f"  current branch: {branch}")
    print(f"  workspace version: {old} -> {new}")
    print(f"  would commit: chore(release): bump workspace version to {new}")
    print(f"  would create tag: prerelease/v{new}")
    print(f"  would push: origin {branch} and prerelease/v{new}")


def execute(repo_root: Path, *, dry_run: bool) -> int:
    branch = current_branch(repo_root)
    require_clean_tree(repo_root)
    old = workspace_version(repo_root)
    new = patch_bump(old)
    tag = f"prerelease/v{new}"
    manifests = workspace_manifest_paths(repo_root)
    package_names = workspace_package_names(manifests)

    if dry_run:
        verify_lockstep(repo_root)
        describe_plan(branch, old, new)
        return 0

    if git(repo_root, "rev-parse", "--verify", f"refs/tags/{tag}", check=False).returncode == 0:
        raise SystemExit(f"tag already exists locally: {tag}")
    if remote_tag_exists(repo_root, tag):
        raise SystemExit(f"tag already exists on origin: {tag}")

    update_manifest_versions(repo_root, old, new)
    update_lockfile(repo_root, old, new, package_names)
    verify_lockstep(repo_root)
    git(
        repo_root,
        "add",
        "Cargo.toml",
        "Cargo.lock",
        *(str(path.relative_to(repo_root)) for path in manifests),
    )
    git(repo_root, "commit", "-m", f"chore(release): bump workspace version to {new}")
    git(repo_root, "tag", "-a", tag, "-m", f"Prerelease {new}")
    git(repo_root, "push", "origin", branch)
    git(repo_root, "push", "origin", tag)
    print(f"created and pushed {tag} from {branch} at {git(repo_root, 'rev-parse', 'HEAD').stdout.strip()}")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="show the bump/commit/tag/push plan without changing anything",
    )
    args = parser.parse_args(argv[1:])
    return execute(discover_repo_root(), dry_run=args.dry_run)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
