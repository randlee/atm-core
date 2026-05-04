#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(message)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def extract_version_from_url(url: str) -> str | None:
    match = re.search(r"/download/v(?P<version>\d+\.\d+\.\d+)/", url)
    if match:
        return match.group("version")
    match = re.search(r"[_-](?P<version>\d+\.\d+\.\d+)[_/]", url)
    if match:
        return match.group("version")
    return None


def validate_workspace_version(repo_root: Path) -> str:
    cargo_toml = tomllib.loads(read_text(repo_root / "Cargo.toml"))
    workspace_version = cargo_toml.get("workspace", {}).get("package", {}).get("version")
    if not workspace_version:
        fail("workspace version missing from Cargo.toml")
    return workspace_version


def validate_crate_versions(repo_root: Path, workspace_version: str) -> None:
    manifests = sorted((repo_root / "crates").glob("*/Cargo.toml"))
    if not manifests:
        fail("no crate manifests found under crates/")

    manifest_texts = {path: read_text(path) for path in manifests}
    for path, text in manifest_texts.items():
        if "version.workspace = true" not in text:
            fail(f"{path.relative_to(repo_root)} must use version.workspace = true")

    atm_toml_path = repo_root / "crates" / "atm" / "Cargo.toml"
    atm_text = manifest_texts.get(atm_toml_path)
    if atm_text is None:
        fail("crates/atm/Cargo.toml missing")

    dep_match = re.search(
        r'agent-team-mail-core".*?version\s*=\s*"(?P<version>\d+\.\d+\.\d+)"',
        atm_text,
    )
    if dep_match is None:
        fail("crates/atm/Cargo.toml is missing the agent-team-mail-core path dependency version pin")
    dep_version = dep_match.group("version")
    if dep_version != workspace_version:
        fail(
            "crates/atm/Cargo.toml agent-team-mail-core dependency version "
            f"({dep_version}) does not match workspace version ({workspace_version})"
        )


def validate_lockfile(repo_root: Path, workspace_version: str) -> None:
    lock = tomllib.loads(read_text(repo_root / "Cargo.lock"))
    packages = lock.get("package", [])
    versions: dict[str, str] = {}
    workspace_packages: set[str] = set()
    for manifest_path in sorted((repo_root / "crates").glob("*/Cargo.toml")):
        manifest = tomllib.loads(read_text(manifest_path))
        package_name = manifest.get("package", {}).get("name")
        if isinstance(package_name, str):
            workspace_packages.add(package_name)
    for package in packages:
        name = package.get("name")
        version = package.get("version")
        if name in workspace_packages:
            versions[name] = version

    for package_name in sorted(workspace_packages):
        version = versions.get(package_name)
        if version is None:
            fail(f"{package_name} missing from Cargo.lock")
        if version != workspace_version:
            fail(
                f"Cargo.lock version for {package_name} ({version}) "
                f"does not match workspace version ({workspace_version})"
            )


def validate_winget_manifest(repo_root: Path, workspace_version: str) -> None:
    manifest_path = repo_root / ".winget" / "randlee.agent-team-mail.yaml"
    text = read_text(manifest_path)

    version_match = re.search(r"^PackageVersion:\s*(?P<version>\d+\.\d+\.\d+)\s*$", text, re.MULTILINE)
    if version_match is None:
        fail(f"{manifest_path.relative_to(repo_root)} is missing PackageVersion")
    package_version = version_match.group("version")
    if package_version != workspace_version:
        fail(
            f"{manifest_path.relative_to(repo_root)} PackageVersion ({package_version}) "
            f"does not match workspace version ({workspace_version})"
        )

    manifest_version_match = re.search(
        r"^ManifestVersion:\s*(?P<version>\d+\.\d+\.\d+)\s*$",
        text,
        re.MULTILINE,
    )
    if manifest_version_match is None:
        fail(f"{manifest_path.relative_to(repo_root)} is missing ManifestVersion")
    manifest_version = manifest_version_match.group("version")
    if manifest_version != workspace_version:
        fail(
            f"{manifest_path.relative_to(repo_root)} ManifestVersion ({manifest_version}) "
            f"does not match workspace version ({workspace_version})"
        )

    installer_match = re.search(r"^\s*InstallerUrl:\s*(?P<url>\S+)\s*$", text, re.MULTILINE)
    if installer_match is None:
        fail(f"{manifest_path.relative_to(repo_root)} is missing InstallerUrl")
    installer_url = installer_match.group("url")
    installer_version = extract_version_from_url(installer_url)
    if installer_version != workspace_version:
        fail(
            f"{manifest_path.relative_to(repo_root)} InstallerUrl version ({installer_version}) "
            f"does not match workspace version ({workspace_version})"
        )


def validate_homebrew_release_wiring(repo_root: Path) -> None:
    workflow_path = repo_root / ".github" / "workflows" / "release.yml"
    text = read_text(workflow_path)

    required_fragments = (
        "update-homebrew:",
        "repository: randlee/homebrew-tap",
        "for formula in homebrew-tap/Formula/agent-team-mail.rb homebrew-tap/Formula/atm.rb; do",
        'version=\'${{ needs.gate-and-tag.outputs.release_version }}\'',
        'tarball_url="https://github.com/randlee/atm-core/releases/download/${tag}/atm_${version}_aarch64-apple-darwin.tar.gz"',
        'sed -i "s|version \\"[^\\"]*\\"|version \\"${version}\\"|g" "$formula"',
    )

    for fragment in required_fragments:
        if fragment not in text:
            fail(
                ".github/workflows/release.yml no longer guarantees Homebrew formulas "
                f"are updated from the shared release version: missing {fragment!r}"
            )


def success_message(workspace_version: str) -> str:
    return (
        f"version sync check passed: workspace_version={workspace_version}; "
        "workspace, crate pin, Cargo.lock, winget, and Homebrew release wiring are aligned."
    )


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    workspace_version = validate_workspace_version(repo_root)
    validate_crate_versions(repo_root, workspace_version)
    validate_lockfile(repo_root, workspace_version)
    validate_winget_manifest(repo_root, workspace_version)
    validate_homebrew_release_wiring(repo_root)
    print(success_message(workspace_version))
    return 0


if __name__ == "__main__":
    sys.exit(main())
