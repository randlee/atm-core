"""Manifest and channel-contract parsing for the vendorable publish kit."""
from __future__ import annotations

import tomllib
from pathlib import Path


CHANNEL_CONTRACTS_FILE = "publish-channel-contracts.toml"
ROOT_CHANNELS = frozenset({"crates_io", "github_release"})


def _require_keys(entry: dict, required: tuple[str, ...], label: str) -> None:
    missing = [key for key in required if key not in entry]
    if missing:
        joined = ", ".join(missing)
        raise SystemExit(f"{label} missing required keys: {joined}")


def load_channel_contracts(path: Path) -> dict[str, dict]:
    """Load the vendorable, non-secret protocol for every supported channel."""
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    channels = data.get("channels")
    if not isinstance(channels, dict):
        raise SystemExit(f"{path}: [channels] must be a table")
    for name, contract in channels.items():
        if not isinstance(contract, dict):
            raise SystemExit(f"{path}: [channels.{name}] must be a table")
        _require_keys(contract, ("stage", "agent"), f"{path}: [channels.{name}]")
        if contract["stage"] not in {"root", "post_release"}:
            raise SystemExit(f"{path}: [channels.{name}].stage must be root or post_release")
        if not isinstance(contract["agent"], str) or not contract["agent"]:
            raise SystemExit(f"{path}: [channels.{name}].agent must be a non-empty string")
    missing_roots = ROOT_CHANNELS - set(channels)
    if missing_roots:
        raise SystemExit(f"{path}: missing required root channel(s): {', '.join(sorted(missing_roots))}")
    return channels


def load_manifest(path: Path, *, with_channel_contracts: bool = False) -> dict:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    crates = data.get("crates", [])
    if not crates:
        raise SystemExit("manifest must define [[crates]]")
    release_binaries = data.get("release_binaries", [])
    python_packages = data.get("python_packages", [])
    python_distributions = data.get("python_distributions", [])
    crates = sorted(crates, key=lambda item: (item["publish_order"], item["artifact"]))
    manifest = {
        "project": data.get("project", {}),
        "crates": crates,
        "release_binaries": release_binaries,
        "release_targets": data.get("release_targets", []),
        "python_packages": python_packages,
        "python_distributions": python_distributions,
        "channels": data.get("channels", {}),
    }
    if with_channel_contracts:
        manifest["channel_contracts"] = load_channel_contracts(
            path.parent / CHANNEL_CONTRACTS_FILE
        )
    return manifest


def workspace_members(workspace_toml: Path) -> set[str]:
    data = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    return set(data.get("workspace", {}).get("members", []))


def package_name(cargo_toml: Path) -> str:
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    return data["package"]["name"]


def workspace_version(workspace_toml: Path) -> str:
    data = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    return data["workspace"]["package"]["version"]


def _resolve_workspace_path(workspace_toml: Path, relative_path: str) -> Path:
    return workspace_toml.parent / relative_path


def _assert_workspace_inherited_version(workspace_toml: Path, relative_path: str) -> None:
    path = _resolve_workspace_path(workspace_toml, relative_path)
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    value = data.get("package", {}).get("version")
    if not isinstance(value, dict) or value.get("workspace") is not True:
        raise SystemExit(
            f"{relative_path}: [package].version must inherit workspace.package.version"
        )


def _assert_python_package_version(
    workspace_toml: Path,
    relative_path: str,
    expected_version: str,
) -> None:
    path = _resolve_workspace_path(workspace_toml, relative_path)
    actual_version = _python_project_version(path)
    if actual_version != expected_version:
        raise SystemExit(
            f"{relative_path}: [project].version mismatch: "
            f"expected {expected_version}, got {actual_version!r}"
        )


def _python_project_version(pyproject_toml: Path) -> str:
    data = tomllib.loads(pyproject_toml.read_text(encoding="utf-8"))
    project = data.get("project", {})
    version = project.get("version")
    if not isinstance(version, str):
        raise SystemExit(f"{pyproject_toml}: [project].version must be a string")
    return version


def _python_project_name(pyproject_toml: Path) -> str:
    data = tomllib.loads(pyproject_toml.read_text(encoding="utf-8"))
    project = data.get("project", {})
    name = project.get("name")
    if not isinstance(name, str):
        raise SystemExit(f"{pyproject_toml}: [project].name must be a string")
    return name


def _python_distribution_entries(manifest: dict) -> list[dict]:
    """Return normalized Python distribution entries from the release manifest."""
    packages = {entry["package"]: entry for entry in manifest["python_packages"]}
    entries: list[dict] = []
    for distribution in manifest["python_distributions"]:
        package = packages[distribution["name"]]
        source = distribution["source"]
        entries.append(
            {
                "artifact": package["artifact"],
                "name": distribution["name"],
                "source": source,
                "pyproject": package["manifest"],
                "cargo_manifest": distribution.get("cargo_manifest", f"{source}/Cargo.toml"),
                "module_path": distribution.get(
                    "module_path", f"{source}/python/{package['module']}"
                ),
                "sdist": distribution["sdist"],
                "wheels": distribution["wheels"],
            }
        )
    return entries


def _python_distribution_expectations(manifest: dict) -> dict[str, dict[str, int]]:
    return {
        entry["name"]: {
            "wheel": len(entry["wheels"]),
            "sdist": int(entry["sdist"]),
        }
        for entry in _python_distribution_entries(manifest)
    }


def _require_project(manifest: dict) -> dict:
    project = manifest["project"]
    _require_keys(
        project,
        ("name", "archive_prefix", "description", "homepage", "license"),
        "[project]",
    )
    return project


def _renderer_archive_path(manifest: dict) -> str:
    value = _require_project(manifest).get("renderer_archive_path")
    if not isinstance(value, str) or not value:
        raise SystemExit("[project].renderer_archive_path must be a non-empty string")
    return value


def _release_targets_by_name(manifest: dict) -> dict[str, dict]:
    targets: dict[str, dict] = {}
    for index, target in enumerate(manifest["release_targets"], start=1):
        _require_keys(target, ("target", "os", "archive"), f"[[release_targets]] #{index}")
        name = target["target"]
        if name in targets:
            raise SystemExit(f"duplicate release target: {name}")
        targets[name] = target
    if not targets:
        raise SystemExit("manifest must define [[release_targets]]")
    return targets


def _channel_config(manifest: dict, channel_name: str) -> dict:
    try:
        channel = manifest["channels"][channel_name]
    except KeyError as error:
        raise SystemExit(f"manifest must define [channels.{channel_name}]") from error
    if not isinstance(channel, dict):
        raise SystemExit(f"[channels.{channel_name}] must be a table")
    return channel


def _channel_contract(manifest: dict, channel_name: str) -> dict:
    try:
        contract = manifest["channel_contracts"][channel_name]
    except KeyError as error:
        raise SystemExit(f"channel contract missing for {channel_name}") from error
    return contract


def _channel_names(manifest: dict) -> tuple[str, ...]:
    channels = manifest["channels"]
    if not isinstance(channels, dict):
        raise SystemExit("[channels] must be a table")
    contracts = manifest["channel_contracts"]
    post_release_channels = {
        name for name, contract in contracts.items() if contract["stage"] == "post_release"
    }
    unknown = sorted(set(channels) - post_release_channels)
    if unknown:
        raise SystemExit("unsupported release channel(s): " + ", ".join(unknown))
    if not channels:
        raise SystemExit("manifest must define at least one [channels.<name>] table")
    return tuple(channels)
