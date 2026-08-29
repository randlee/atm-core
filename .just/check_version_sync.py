#!/usr/bin/env python3
from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Callable

from lint_common import load_lint_config
from lint_common import workspace_crate_section_lines
from lint_common import workspace_manifest_paths


KIT_RELEASE_ARTIFACTS = Path(".github/scripts/release_artifacts.py")


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


SEMVER_PATTERN = r"\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"


def extract_versions_from_url(url: str) -> list[str]:
    versions = re.findall(rf"/download/v(?P<version>{SEMVER_PATTERN})/", url)
    versions.extend(re.findall(rf"[_-](?P<version>{SEMVER_PATTERN})[/_]", url))
    return versions


def extract_version_from_url(url: str) -> str | None:
    """Return the first URL version for compatibility with existing callers."""

    versions = extract_versions_from_url(url)
    return versions[0] if versions else None


def version_sync_config(repo_root: Path) -> dict:
    config = load_lint_config(repo_root).get("version_sync", {})
    if not isinstance(config, dict):
        raise SystemExit("[version_sync] must be a TOML table")
    return config


def winget_settings(config: dict) -> tuple[str, str, str, str] | None:
    winget = config.get("winget", {})
    if not isinstance(winget, dict) or not winget.get("enabled", False):
        return None
    manifest_glob = winget.get("manifest_glob")
    fields = (
        winget.get("package_version_field", "PackageVersion"),
        winget.get("manifest_version_field", "ManifestVersion"),
        winget.get("installer_url_field", "InstallerUrl"),
    )
    if not isinstance(manifest_glob, str) or not manifest_glob.strip():
        raise SystemExit("[version_sync.winget].manifest_glob must be a non-empty string when enabled")
    if not all(isinstance(field, str) and field for field in fields):
        raise SystemExit("[version_sync.winget] field names must be non-empty strings")
    return (manifest_glob, *fields)


def winget_manifest_paths(repo_root: Path, manifest_glob: str) -> list[Path]:
    paths = sorted(repo_root.glob(manifest_glob))
    if not paths:
        fail(f"no Winget manifests found for glob {manifest_glob!r}")
    return paths


def winget_field_pattern(field_name: str) -> re.Pattern[str]:
    return re.compile(rf"^([ \t]*{re.escape(field_name)}:[ \t]*)(\S+)([ \t]*)$", re.MULTILINE)


def winget_field_values(text: str, field_name: str) -> list[str]:
    return [match.group(2) for match in winget_field_pattern(field_name).finditer(text)]


def replace_winget_field_values(
    text: str, field_name: str, transform: Callable[[str], str], *, expected_count: int | None = None
) -> tuple[str, int]:
    pattern = winget_field_pattern(field_name)
    matches = list(pattern.finditer(text))
    if not matches:
        return text, 0
    if expected_count is not None and len(matches) != expected_count:
        return text, len(matches)
    updated = pattern.sub(
        lambda match: f"{match.group(1)}{transform(match.group(2))}{match.group(3)}",
        text,
    )
    return updated, len(matches)


def replace_version_occurrences(value: str, old_version: str, new_version: str) -> str:
    pattern = re.compile(rf"(?<![0-9.]){re.escape(old_version)}(?![0-9])")
    return pattern.sub(new_version, value)


def validate_workspace_version(repo_root: Path) -> str:
    cargo_toml = tomllib.loads(read_text(repo_root / "Cargo.toml"))
    workspace_version = cargo_toml.get("workspace", {}).get("package", {}).get("version")
    if not workspace_version:
        fail("workspace version missing from Cargo.toml")
    return workspace_version


def validate_release_version_lockstep(repo_root: Path) -> None:
    """Delegate release-artifact version lockstep to the installed kit."""

    completed = subprocess.run(
        [
            sys.executable,
            str(repo_root / KIT_RELEASE_ARTIFACTS),
            "verify-version-lockstep",
            "--manifest",
            "release/publish-artifacts.toml",
            "--workspace-toml",
            "Cargo.toml",
        ],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if completed.returncode != 0:
        fail((completed.stderr or completed.stdout).strip() or "kit version-lockstep verification failed")


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
    publish_flags: dict[Path, bool] = {}
    for path in manifests:
        text = read_text(path)
        rel_manifest = path.relative_to(repo_root).as_posix()
        manifest = tomllib.loads(text)
        manifest_payloads[path] = (text, manifest, rel_manifest)
        member_dir = path.parent.resolve()
        expected_versions[member_dir] = expected_package_version(
            manifest,
            workspace_version,
            rel_manifest,
        )
        package = manifest.get("package", {}) if isinstance(manifest.get("package"), dict) else {}
        publish_value = package.get("publish", True)
        # publish can be bool or list (registry allowlist); treat anything other than
        # an explicit False as publishable for version-pin enforcement.
        publish_flags[member_dir] = publish_value is not False

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
                # Skip version-pin enforcement when the target crate is publish=false.
                # Versioned deps on publish=false crates break `cargo publish` (cargo
                # tries to resolve the pin on crates.io, where the crate never appears),
                # so the only legal shape for such deps is path-only.
                if not publish_flags.get(resolved_path, True):
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
    settings = winget_settings(config)
    if settings is None:
        return False
    manifest_glob, package_version_field, manifest_version_field, installer_url_field = settings
    manifest_paths = winget_manifest_paths(repo_root, manifest_glob)

    for manifest_path in manifest_paths:
        rel_manifest = manifest_path.relative_to(repo_root).as_posix()
        text = read_text(manifest_path)

        def extract_field(field_name: str) -> str:
            values = winget_field_values(text, field_name)
            if len(values) != 1:
                fail(f"{rel_manifest} is missing {field_name}")
            return values[0]

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

        installer_urls = winget_field_values(text, installer_url_field)
        if not installer_urls:
            fail(f"{rel_manifest} is missing {installer_url_field}")
        for installer_url in installer_urls:
            installer_versions = extract_versions_from_url(installer_url)
            if not installer_versions or any(version != workspace_version for version in installer_versions):
                fail(
                    f"{rel_manifest} {installer_url_field} versions ({installer_versions}) "
                    f"does not match workspace version ({workspace_version})"
                )
    return True


def sync_winget_manifests(
    repo_root: Path, old_version: str, workspace_version: str, config: dict
) -> list[Path]:
    """Update the configured Winget fields used by ``validate_winget_manifests``."""

    settings = winget_settings(config)
    if settings is None:
        return []
    manifest_glob, package_version_field, manifest_version_field, installer_url_field = settings
    manifest_paths = winget_manifest_paths(repo_root, manifest_glob)

    changed: list[Path] = []
    for manifest_path in manifest_paths:
        text = read_text(manifest_path)
        original = text
        for field_name in (package_version_field, manifest_version_field):
            text, count = replace_winget_field_values(
                text,
                field_name,
                lambda _old_value: workspace_version,
                expected_count=1,
            )
            if count != 1:
                fail(f"{manifest_path.relative_to(repo_root)} must contain exactly one {field_name} field")

        text, count = replace_winget_field_values(
            text,
            installer_url_field,
            lambda value: replace_version_occurrences(value, old_version, workspace_version),
        )
        if count == 0:
            fail(f"{manifest_path.relative_to(repo_root)} is missing {installer_url_field} version")

        if text != original:
            manifest_path.write_text(text, encoding="utf-8")
            changed.append(manifest_path)
    return changed


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
    validate_release_version_lockstep(repo_root)
    validate_crate_versions(repo_root, workspace_version)
    validate_lockfile(repo_root, workspace_version)

    executed_checks = [
        "installed release-contract version lockstep",
        "internal path deps",
        "Cargo.lock",
    ]
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
