"""Workspace package validation helpers for the release manifest CLI."""

from __future__ import annotations

import tomllib
from pathlib import Path

from release_manifest import package_name, workspace_members


def workspace_package_defaults(workspace_toml: Path) -> dict:
    """Return inheritable package metadata declared by the Rust workspace."""
    data = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    package = data.get("workspace", {}).get("package", {})
    return package if isinstance(package, dict) else {}


def crate_is_publishable(cargo_toml: Path) -> bool:
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    publish = data.get("package", {}).get("publish")
    return publish is not False and not (isinstance(publish, list) and not publish)


def workspace_package_map(workspace_toml: Path) -> dict[str, Path]:
    root = workspace_toml.parent
    packages: dict[str, Path] = {}
    for member in workspace_members(workspace_toml):
        cargo_toml = root / member / "Cargo.toml"
        if cargo_toml.is_file():
            packages[package_name(cargo_toml)] = cargo_toml
    return packages


def package_field_value(package: dict, field: str, workspace_defaults: dict) -> str | None:
    value = package.get(field)
    if isinstance(value, str) and value.strip():
        return value.strip()
    if isinstance(value, dict) and value.get("workspace") is True:
        inherited = workspace_defaults.get(field)
        if isinstance(inherited, str) and inherited.strip():
            return inherited.strip()
    return None


def missing_publish_metadata_fields(cargo_toml: Path, workspace_defaults: dict) -> list[str]:
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    package = data.get("package", {})
    if not isinstance(package, dict):
        return ["package"]
    missing: list[str] = []
    if package_field_value(package, "description", workspace_defaults) is None:
        missing.append("description")
    if (
        package_field_value(package, "license", workspace_defaults) is None
        and package_field_value(package, "license-file", workspace_defaults) is None
    ):
        missing.append("license or license-file")
    return missing


def workspace_dependency_names(cargo_toml: Path, workspace_toml: Path) -> set[str]:
    """Return runtime/build dependencies that resolve to workspace packages."""
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    workspace_root = workspace_toml.parent.resolve()
    workspace_data = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    workspace_dependencies = workspace_data.get("workspace", {}).get("dependencies", {})
    workspace_packages = set(workspace_package_map(workspace_toml))
    crate_dir = cargo_toml.parent
    dependencies: set[str] = set()

    def resolve(name: str, spec: object) -> str | None:
        if isinstance(spec, str):
            return name if name in workspace_packages else None
        if not isinstance(spec, dict):
            return None
        if spec.get("workspace") is True:
            workspace_spec = workspace_dependencies.get(name, {})
            if isinstance(workspace_spec, dict):
                package = workspace_spec.get("package", name)
                if "path" in workspace_spec or package in workspace_packages:
                    return package
            return name if name in workspace_packages else None
        package = spec.get("package", name)
        if "path" in spec:
            dependency_path = (crate_dir / spec["path"]).resolve()
            if dependency_path.is_relative_to(workspace_root):
                return package
        return package if package in workspace_packages else None

    def collect(table: object) -> None:
        if not isinstance(table, dict):
            return
        for name, spec in table.items():
            package = resolve(name, spec)
            if package:
                dependencies.add(package)

    collect(data.get("dependencies", {}))
    collect(data.get("build-dependencies", {}))
    for target_data in data.get("target", {}).values():
        if isinstance(target_data, dict):
            collect(target_data.get("dependencies", {}))
            collect(target_data.get("build-dependencies", {}))
    return dependencies


def workspace_path_dependencies(cargo_toml: Path, workspace_toml: Path) -> set[str]:
    """Return dependencies declared through a workspace-local path."""
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    workspace_root = workspace_toml.parent.resolve()
    crate_dir = cargo_toml.parent
    dependencies: set[str] = set()

    def collect(table: object) -> None:
        if not isinstance(table, dict):
            return
        for name, spec in table.items():
            if not isinstance(spec, dict) or "path" not in spec:
                continue
            if (crate_dir / spec["path"]).resolve().is_relative_to(workspace_root):
                dependencies.add(str(spec.get("package", name)))

    collect(data.get("dependencies", {}))
    collect(data.get("build-dependencies", {}))
    for target_data in data.get("target", {}).values():
        if isinstance(target_data, dict):
            collect(target_data.get("dependencies", {}))
            collect(target_data.get("build-dependencies", {}))
    return dependencies
