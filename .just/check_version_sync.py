#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

from lint_common import load_lint_config
from lint_common import workspace_crate_section_lines
from lint_common import workspace_manifest_paths


def fail(message: str) -> None:
    raise SystemExit(message)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def dependency_sections(manifest: dict) -> list[tuple[str, dict]]:
    sections: list[tuple[str, dict]] = []
    for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
        dependencies = manifest.get(section_name)
        if isinstance(dependencies, dict):
            sections.append((section_name, dependencies))

    targets = manifest.get("target", {})
    if isinstance(targets, dict):
        for target_name, target in targets.items():
            if not isinstance(target, dict):
                continue
            for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
                dependencies = target.get(section_name)
                if isinstance(dependencies, dict):
                    sections.append((f"target.{target_name}.{section_name}", dependencies))
    return sections


def extract_version_from_url(url: str) -> str | None:
    match = re.search(r"/download/v(?P<version>\d+\.\d+\.\d+)/", url)
    if match:
        return match.group("version")
    match = re.search(r"[_-](?P<version>\d+\.\d+\.\d+)[_/]", url)
    if match:
        return match.group("version")
    return None


def version_sync_config(repo_root: Path) -> dict:
    config = load_lint_config(repo_root).get("version_sync", {})
    if not isinstance(config, dict):
        raise SystemExit("[version_sync] must be a TOML table")
    return config


def validate_workspace_version(repo_root: Path) -> str:
    cargo_toml = tomllib.loads(read_text(repo_root / "Cargo.toml"))
    workspace_version = cargo_toml.get("workspace", {}).get("package", {}).get("version")
    if not workspace_version:
        fail("workspace version missing from Cargo.toml")
    return workspace_version


def expected_package_version(manifest: dict, workspace_version: str, manifest_label: str) -> str:
    package = manifest.get("package", {})
    if not isinstance(package, dict):
        fail(f"{manifest_label} missing [package] table")

    version_value = package.get("version")
    if isinstance(version_value, str) and version_value.strip():
        return version_value
    if isinstance(version_value, dict) and version_value.get("workspace") is True:
        return workspace_version
    fail(
        f"{manifest_label} must define [package].version either as a non-empty string or version.workspace = true"
    )


def validate_crate_versions(repo_root: Path, workspace_version: str) -> None:
    manifests = workspace_manifest_paths(repo_root)
    if not manifests:
        fail("no workspace member manifests found")

    workspace_member_dirs = {manifest_path.parent.resolve() for manifest_path in manifests}
    manifest_payloads: dict[Path, tuple[str, dict, str]] = {}
    expected_versions: dict[Path, str] = {}
    for path in manifests:
        text = read_text(path)
        rel_manifest = path.relative_to(repo_root).as_posix()
        manifest = tomllib.loads(text)
        manifest_payloads[path] = (text, manifest, rel_manifest)
        expected_versions[path.parent.resolve()] = expected_package_version(
            manifest,
            workspace_version,
            rel_manifest,
        )

    for path, (_text, manifest, rel_manifest) in manifest_payloads.items():
        for section_name, dependencies in dependency_sections(manifest):
            for dependency_name, dependency in dependencies.items():
                if not isinstance(dependency, dict):
                    continue
                dependency_path = dependency.get("path")
                if not isinstance(dependency_path, str):
                    continue
                resolved_path = (path.parent / dependency_path).resolve()
                if resolved_path not in workspace_member_dirs:
                    continue
                dependency_version = expected_versions[resolved_path]
                pinned_version = dependency.get("version")
                if pinned_version != dependency_version:
                    fail(
                        f"{rel_manifest} [{section_name}.{dependency_name}]: "
                        f'internal path dependency version must match target crate version "{dependency_version}"'
                    )


def validate_lockfile(repo_root: Path, workspace_version: str) -> None:
    lock = tomllib.loads(read_text(repo_root / "Cargo.lock"))
    packages = lock.get("package", [])
    versions: dict[str, str] = {}
    workspace_packages: dict[str, str] = {}
    for manifest_path in workspace_manifest_paths(repo_root):
        manifest = tomllib.loads(read_text(manifest_path))
        rel_manifest = manifest_path.relative_to(repo_root).as_posix()
        package_name = manifest.get("package", {}).get("name")
        if isinstance(package_name, str):
            workspace_packages[package_name] = expected_package_version(
                manifest,
                workspace_version,
                rel_manifest,
            )
    for package in packages:
        name = package.get("name")
        version = package.get("version")
        if name in workspace_packages and isinstance(version, str):
            versions[name] = version

    for package_name in sorted(workspace_packages):
        version = versions.get(package_name)
        if version is None:
            fail(f"{package_name} missing from Cargo.lock")
        expected_version = workspace_packages[package_name]
        if version != expected_version:
            fail(
                f"Cargo.lock version for {package_name} ({version}) "
                f'does not match expected crate version ({expected_version})'
            )


def validate_winget_manifests(repo_root: Path, workspace_version: str, config: dict) -> bool:
    winget = config.get("winget", {})
    if not isinstance(winget, dict) or not winget.get("enabled", False):
        return False

    manifest_glob = winget.get("manifest_glob")
    if not isinstance(manifest_glob, str) or not manifest_glob.strip():
        raise SystemExit("[version_sync.winget].manifest_glob must be a non-empty string when enabled")

    package_version_field = winget.get("package_version_field", "PackageVersion")
    manifest_version_field = winget.get("manifest_version_field", "ManifestVersion")
    installer_url_field = winget.get("installer_url_field", "InstallerUrl")
    if not all(isinstance(item, str) and item for item in (package_version_field, manifest_version_field, installer_url_field)):
        raise SystemExit("[version_sync.winget] field names must be non-empty strings")

    manifest_paths = sorted(repo_root.glob(manifest_glob))
    if not manifest_paths:
        fail(f"no Winget manifests found for glob {manifest_glob!r}")

    for manifest_path in manifest_paths:
        rel_manifest = manifest_path.relative_to(repo_root).as_posix()
        text = read_text(manifest_path)

        def extract_field(field_name: str) -> str:
            match = re.search(rf"^{re.escape(field_name)}:\s*(?P<value>\S+)\s*$", text, re.MULTILINE)
            if match is None:
                fail(f"{rel_manifest} is missing {field_name}")
            return match.group("value")

        package_version = extract_field(package_version_field)
        if package_version != workspace_version:
            fail(
                f"{rel_manifest} {package_version_field} ({package_version}) "
                f"does not match workspace version ({workspace_version})"
            )

        manifest_version = extract_field(manifest_version_field)
        if manifest_version != workspace_version:
            fail(
                f"{rel_manifest} {manifest_version_field} ({manifest_version}) "
                f"does not match workspace version ({workspace_version})"
            )

        installer_urls = re.findall(rf"^\s*{re.escape(installer_url_field)}:\s*(?P<value>\S+)\s*$", text, re.MULTILINE)
        if not installer_urls:
            fail(f"{rel_manifest} is missing {installer_url_field}")
        for installer_url in installer_urls:
            installer_version = extract_version_from_url(installer_url)
            if installer_version != workspace_version:
                fail(
                    f"{rel_manifest} {installer_url_field} version ({installer_version}) "
                    f"does not match workspace version ({workspace_version})"
                )
    return True


def validate_release_wiring(repo_root: Path, config: dict) -> bool:
    release_wiring = config.get("release_wiring", {})
    if not isinstance(release_wiring, dict) or not release_wiring.get("enabled", False):
        return False

    file_path = release_wiring.get("file")
    fragments = release_wiring.get("required_fragments", [])
    if not isinstance(file_path, str) or not file_path.strip():
        raise SystemExit("[version_sync.release_wiring].file must be a non-empty string when enabled")
    if not isinstance(fragments, list) or not all(isinstance(item, str) for item in fragments):
        raise SystemExit("[version_sync.release_wiring].required_fragments must be an array of strings")

    workflow_path = repo_root / file_path
    text = read_text(workflow_path)
    for fragment in fragments:
        if fragment not in text:
            fail(
                f"{file_path} no longer guarantees release wiring from the shared workspace version: "
                f"missing {fragment!r}"
            )
    return True


def success_message(workspace_version: str, executed_checks: list[str]) -> str:
    return (
        f"version sync check passed: workspace_version={workspace_version}; "
        + ", ".join(executed_checks)
        + " are aligned."
    )


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    config = version_sync_config(repo_root)
    workspace_version = validate_workspace_version(repo_root)
    validate_crate_versions(repo_root, workspace_version)
    validate_lockfile(repo_root, workspace_version)

    executed_checks = ["workspace member versions", "internal path deps", "Cargo.lock"]
    if validate_winget_manifests(repo_root, workspace_version, config):
        executed_checks.append("winget")
    if validate_release_wiring(repo_root, config):
        executed_checks.append("release wiring")

    for line in workspace_crate_section_lines(repo_root):
        print(line)
    print(success_message(workspace_version, executed_checks))
    return 0


if __name__ == "__main__":
    sys.exit(main())
