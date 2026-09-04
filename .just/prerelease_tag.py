#!/usr/bin/env python3
"""Create and publish a patch-bumped prerelease tag on the current branch."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

from lint_common import discover_repo_root
from check_version_sync import sync_winget_manifests
from check_version_sync import version_sync_config


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


def python_project_paths(repo_root: Path) -> list[Path]:
    return sorted((repo_root / "crates").glob("*/pyproject.toml"))


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


def sync_python_version(repo_root: Path, pyproject: Path) -> None:
    result = subprocess.run(
        [
            sys.executable,
            str(repo_root / ".github" / "scripts" / "release_artifacts.py"),
            "sync-python-version",
            "--workspace-toml",
            "Cargo.toml",
            "--pyproject",
            str(pyproject.relative_to(repo_root)),
        ],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SystemExit(f"python version sync failed for {pyproject}: {detail}")


def update_manifest_versions(repo_root: Path, old: str, new: str) -> dict[Path, str]:
    root_manifest = repo_root / "Cargo.toml"
    python_projects = python_project_paths(repo_root)
    before = {
        manifest_path: manifest_path.read_text(encoding="utf-8")
        for manifest_path in [root_manifest, *python_projects]
    }
    original = before[root_manifest]
    after, _did_change = replace_package_version(original, "workspace.package", old, new)
    # Workspace dependency versions live here so member manifests remain stable
    # across release bumps and do not need per-crate rewrites.
    lines = []
    for line in after.splitlines(keepends=True):
        if "path =" in line:
            line = line.replace(f'version = "{old}"', f'version = "{new}"')
        lines.append(line)
    root_manifest.write_text("".join(lines), encoding="utf-8")

    if before[root_manifest] == root_manifest.read_text(encoding="utf-8"):
        raise SystemExit("workspace package version was not found in Cargo.toml")

    for pyproject in python_projects:
        sync_python_version(repo_root, pyproject)

    winget_paths = sync_winget_manifests(repo_root, old, new, version_sync_config(repo_root))

    tracked_manifests = [root_manifest, *python_projects]
    tracked_manifests.extend(winget_paths)
    return {
        path: path.read_text(encoding="utf-8")
        for path in tracked_manifests
        if path not in before or path.read_text(encoding="utf-8") != before[path]
    }


def update_lockfile(repo_root: Path) -> str:
    result = subprocess.run(
        ["cargo", "update", "--workspace", "--offline"],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SystemExit(f"cargo update --workspace --offline failed: {detail}")
    return (repo_root / "Cargo.lock").read_text(encoding="utf-8")


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


def validate_candidate(repo_root: Path, version: str, manifest_updates: dict[Path, str]) -> None:
    for manifest_path, text in manifest_updates.items():
        if manifest_path.suffix != ".toml":
            continue
        try:
            tomllib.loads(text)
        except tomllib.TOMLDecodeError as error:
            raise SystemExit(f"updated manifest is invalid: {manifest_path}: {error}") from error

    plan_result = subprocess.run(
        [
            sys.executable,
            str(repo_root / ".github" / "scripts" / "release_artifacts.py"),
            "build-plan",
            "--manifest",
            "release/publish-artifacts.toml",
        ],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if plan_result.returncode != 0:
        raise SystemExit((plan_result.stderr or plan_result.stdout).strip() or "build-plan failed")
    plan = json.loads(plan_result.stdout)
    workspace_toml = plan["workspace_toml"]
    for command in (
        [
            "verify-version",
            "--manifest",
            "release/publish-artifacts.toml",
            "--workspace-toml",
            workspace_toml,
            "--version",
            version,
        ],
        [
            "verify-version-lockstep",
            "--manifest",
            "release/publish-artifacts.toml",
            "--workspace-toml",
            workspace_toml,
        ],
    ):
        result = subprocess.run(
            [sys.executable, str(repo_root / ".github" / "scripts" / "release_artifacts.py"), *command],
            cwd=repo_root,
            text=True,
            capture_output=True,
            check=False,
        )
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise SystemExit(f"release version validation failed: {detail}")
    verify_lockstep(repo_root)


def candidate_changes(repo_root: Path, old: str, new: str) -> dict[Path, str]:
    with tempfile.TemporaryDirectory(prefix="prerelease-tag-") as directory:
        candidate = Path(directory) / repo_root.name
        copy_tracked_files(repo_root, candidate)
        candidate_updates = update_manifest_versions(candidate, old, new)
        lockfile = update_lockfile(candidate)
        candidate_updates[candidate / "Cargo.lock"] = lockfile
        validate_candidate(candidate, new, candidate_updates)
        return {
            repo_root / path.relative_to(candidate): text
            for path, text in candidate_updates.items()
        }


def copy_tracked_files(repo_root: Path, destination: Path) -> None:
    """Copy only the clean checkout's tracked files into a candidate tree."""

    destination.mkdir(parents=True, exist_ok=True)
    result = git(repo_root, "ls-files", "-z")
    for encoded_path in result.stdout.encode().split(b"\0"):
        if not encoded_path:
            continue
        relative_path = Path(os.fsdecode(encoded_path))
        source = repo_root / relative_path
        target = destination / relative_path
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_symlink():
            target.symlink_to(os.readlink(source))
        else:
            shutil.copy2(source, target)


def remote_tag_exists(repo_root: Path, tag: str) -> bool:
    # TODO(sc-publish): share tag checks with vendored release.yml; see https://github.com/randlee/sc-publish/issues/79.
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


def write_and_commit(repo_root: Path, changes: dict[Path, str], message: str) -> None:
    touched = list(changes)
    committed = False
    try:
        for path, text in changes.items():
            path.write_text(text, encoding="utf-8")
        git(
            repo_root,
            "add",
            *(str(path.relative_to(repo_root)) for path in touched),
        )
        git(repo_root, "commit", "-m", message)
        committed = True
    except BaseException:
        if not committed and touched:
            relative_paths = [str(path.relative_to(repo_root)) for path in touched]
            git(repo_root, "reset", "HEAD", "--", *relative_paths, check=False)
            git(repo_root, "checkout", "--", *relative_paths, check=False)
        raise


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

    if dry_run:
        verify_lockstep(repo_root)
        describe_plan(branch, old, new)
        return 0

    if git(repo_root, "rev-parse", "--verify", f"refs/tags/{tag}", check=False).returncode == 0:
        raise SystemExit(f"tag already exists locally: {tag}")
    if remote_tag_exists(repo_root, tag):
        raise SystemExit(f"tag already exists on origin: {tag}")

    changes = candidate_changes(repo_root, old, new)
    write_and_commit(repo_root, changes, f"chore(release): bump workspace version to {new}")
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
