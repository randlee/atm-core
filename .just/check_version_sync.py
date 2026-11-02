#!/usr/bin/env python3
from __future__ import annotations

import re
import tomllib
from pathlib import Path
from typing import Callable

from lint_common import load_lint_config


def fail(message: str) -> None:
    raise SystemExit(message)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


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
    pattern = re.compile(rf"(?<![0-9.]){re.escape(old_version)}(?![0-9.])")
    return pattern.sub(new_version, value)


def validate_workspace_version(repo_root: Path) -> str:
    cargo_toml = tomllib.loads(read_text(repo_root / "Cargo.toml"))
    workspace_version = cargo_toml.get("workspace", {}).get("package", {}).get("version")
    if not workspace_version:
        fail("workspace version missing from Cargo.toml")
    return workspace_version


def sync_winget_manifests(
    repo_root: Path, old_version: str, workspace_version: str, config: dict
) -> list[Path]:
    """Update the configured Winget version fields for a release."""

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
