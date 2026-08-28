#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import shutil
import tarfile
import tomllib
import zipfile
from email import message_from_bytes
from pathlib import Path

from release_manifest import (
    _assert_python_package_version,
    _assert_workspace_inherited_version,
    _channel_config,
    _channel_contract,
    _channel_names,
    _channel_preflight_result,
    _homebrew_formulas_for_tag,
    _public_registry_checks,
    _validate_homebrew_formulas,
    _python_distribution_entries,
    _python_distribution_expectations,
    _python_project_name,
    _python_project_version,
    _release_targets_by_name,
    _renderer_archive_path,
    _require_keys,
    _require_project,
    load_channel_contracts,
    load_manifest,
    manifest_python_upload_tool,
    manifest_rust_toolchain,
    manifest_workspace_toml,
    registry_version_state,
    package_name,
    workspace_members,
    workspace_version,
    validate_publish_order,
)


def _channel_dispatch_config(manifest: dict, channel_name: str) -> tuple[str, dict[str, str]]:
    channel = _channel_config(manifest, channel_name)
    _require_keys(channel, ("workflow", "dispatch_inputs"), f"[channels.{channel_name}]")
    workflow = channel["workflow"]
    dispatch_inputs = channel["dispatch_inputs"]
    if not isinstance(workflow, str) or not workflow:
        raise SystemExit(f"[channels.{channel_name}].workflow must be a non-empty string")
    if not isinstance(dispatch_inputs, dict) or not all(
        isinstance(key, str) and isinstance(value, str)
        for key, value in dispatch_inputs.items()
    ):
        raise SystemExit(
            f"[channels.{channel_name}].dispatch_inputs must be a string-to-string table"
        )
    if "tag" in dispatch_inputs:
        raise SystemExit(f"[channels.{channel_name}].dispatch_inputs must not override tag")
    return workflow, dispatch_inputs

def _channel_credential_rehearsal(
    manifest: dict, channel_name: str
) -> tuple[str, dict[str, str]] | None:
    """Return a safe channel rehearsal for credentials not safely probed in preflight."""
    channel = _channel_config(manifest, channel_name)
    rehearsal_inputs = channel.get("credential_rehearsal_inputs")
    if rehearsal_inputs is None:
        return None
    if not isinstance(rehearsal_inputs, dict) or not all(
        isinstance(key, str) and isinstance(value, str)
        for key, value in rehearsal_inputs.items()
    ):
        raise SystemExit(
            f"[channels.{channel_name}].credential_rehearsal_inputs "
            "must be a string-to-string table"
        )
    if "tag" in rehearsal_inputs:
        raise SystemExit(
            f"[channels.{channel_name}].credential_rehearsal_inputs must not override tag"
        )
    workflow, _ = _channel_dispatch_config(manifest, channel_name)
    return workflow, rehearsal_inputs

def _post_release_channel_preflight(manifest: dict, channel_name: str) -> dict[str, object]:
    """Return the non-secret readiness contract a channel worker must consume."""
    contract = _channel_contract(manifest, channel_name)
    if contract["stage"] != "post_release":
        raise SystemExit(f"channel contract {channel_name} is not a post-release channel")

    rehearsal = _channel_credential_rehearsal(manifest, channel_name)
    rehearsal_plan = None
    if rehearsal is not None:
        workflow, inputs = rehearsal
        rehearsal_plan = {"workflow": workflow, "inputs": inputs}

    return {
        "agent": contract["agent"],
        "repository_secrets": contract.get("repository_secrets", []),
        "environment_secrets": contract.get("environment_secrets", []),
        "liveness_checks": contract.get("liveness_checks", []),
        "public_registry_checks": contract.get("public_registry_checks", False),
        "credential_rehearsal": rehearsal_plan,
    }

def _root_channel_preflight(manifest: dict) -> list[dict[str, object]]:
    """Return non-secret requirements for root-workflow publish channels."""
    channels: list[dict[str, object]] = []
    if manifest["crates"]:
        contract = _channel_contract(manifest, "crates_io")
        channels.append(
            {
                "name": "crates_io",
                "agent": contract["agent"],
                "repository_secrets": contract.get("repository_secrets", []),
                "environment_secrets": contract.get("environment_secrets", []),
                "liveness_checks": contract.get("liveness_checks", []),
                "public_registry_checks": contract.get("public_registry_checks", False),
                "credential_rehearsal": None,
            }
        )
    contract = _channel_contract(manifest, "github_release")
    channels.append(
        {
            "name": "github_release",
            "agent": contract["agent"],
            "repository_secrets": contract.get("repository_secrets", []),
            "environment_secrets": contract.get("environment_secrets", []),
            "liveness_checks": contract.get("liveness_checks", []),
            "github_actions_permissions": contract.get("github_actions_permissions", []),
            "public_registry_checks": contract.get("public_registry_checks", False),
            "credential_rehearsal": None,
        }
    )
    return channels
def cmd_channel_preflight_results(args: argparse.Namespace) -> int:
    """Emit one non-secret result for every root and post-release channel."""
    try:
        outcomes = json.loads(args.outcomes)
    except json.JSONDecodeError as error:
        raise SystemExit(f"invalid preflight outcomes JSON: {error.msg}") from error
    if not isinstance(outcomes, dict) or not all(
        isinstance(name, str)
        and (
            isinstance(outcome, str)
            or (
                isinstance(outcome, dict)
                and all(
                    isinstance(channel, str) and isinstance(status, str)
                    for channel, status in outcome.items()
                )
            )
        )
        for name, outcome in outcomes.items()
    ):
        raise SystemExit(
            "preflight outcomes must map each check to a string or channel-status object"
        )

    manifest = load_manifest(Path(args.manifest), with_channel_contracts=True)
    contracts = [
        *_root_channel_preflight(manifest),
        *[
            {"name": channel_name, **_post_release_channel_preflight(manifest, channel_name)}
            for channel_name in _channel_names(manifest)
        ],
    ]
    tag = args.tag or None
    results = [
        _channel_preflight_result(channel, outcomes, tag) for channel in contracts
    ]
    print(json.dumps({"tag": tag, "channels": results}, separators=(",", ":")))
    return 0


def cmd_public_registry_check_plan(args: argparse.Namespace) -> int:
    """Emit non-secret public name/version checks for Release Preflight."""
    manifest = load_manifest(Path(args.manifest), with_channel_contracts=True)
    checks: list[dict[str, str | None]] = []

    for crate in manifest["crates"]:
        checks.extend(
            _public_registry_checks(
                manifest["channel_contracts"], "crates_io", crate["package"], args.version
            )
        )

    for distribution in _python_distribution_entries(manifest):
        checks.extend(
            _public_registry_checks(
                manifest["channel_contracts"], "pypi", distribution["name"], args.version
            )
        )
    print(json.dumps({"checks": checks}, separators=(",", ":")))
    return 0


def cmd_public_registry_inquiry_plan(args: argparse.Namespace) -> int:
    """Emit a direct, read-only candidate name/version lookup plan from contracts."""
    contracts = load_channel_contracts(Path(args.contracts))
    checks = _public_registry_checks(contracts, args.channel, args.name, args.version)
    print(json.dumps({"checks": checks}, separators=(",", ":")))
    return 0


def _channel_renderer_target(manifest: dict, channel_name: str) -> dict | None:
    """Return the published Linux renderer asset required by a channel workflow."""
    if channel_name not in ("homebrew", "scoop"):
        return None

    channel = _channel_config(manifest, channel_name)
    _require_keys(channel, ("renderer_target",), f"[channels.{channel_name}]")
    target_name = channel["renderer_target"]
    targets = _release_targets_by_name(manifest)
    try:
        target = targets[target_name]
    except KeyError as error:
        raise SystemExit(
            f"[channels.{channel_name}].renderer_target references unknown release target: {target_name}"
        ) from error
    if target["os"] != "ubuntu-latest" or target["archive"] != "tar.gz":
        raise SystemExit(
            f"[channels.{channel_name}].renderer_target must name an ubuntu-latest tar.gz release target"
        )
    return target


def _release_asset_pattern(project: dict, target: dict) -> str:
    return (
        rf"^{re.escape(project['archive_prefix'])}_.*_"
        rf"{re.escape(target['target'])}\.{re.escape(target['archive'])}$"
    )


def _release_binaries(manifest: dict) -> list[dict]:
    binaries = manifest["release_binaries"]
    if not binaries:
        raise SystemExit("manifest must define [[release_binaries]]")
    for index, binary in enumerate(binaries, start=1):
        _require_keys(binary, ("name",), f"[[release_binaries]] #{index}")
        for bundle in binary.get("bundled_paths", []):
            _require_keys(bundle, ("source", "destination"), "bundled_paths entry")
    return binaries


def _validate_homebrew_bundle_destinations(binaries: list[dict]) -> None:
    """Require explicit, safe Homebrew Pathname components for bundled assets."""
    for binary in binaries:
        for bundle in binary.get("bundled_paths", []):
            components = bundle.get("homebrew_destination_components")
            if not isinstance(components, list) or not components or not all(
                isinstance(component, str) and component for component in components
            ):
                raise SystemExit(
                    "bundled_paths entry must define non-empty "
                    "homebrew_destination_components when Homebrew is configured"
                )
            if re.fullmatch(r"[a-z_][a-z0-9_]*", components[0]) is None:
                raise SystemExit(
                    "bundled_paths homebrew_destination_components[0] must be a "
                    "lowercase Homebrew Pathname helper"
                )


def _validate_scoop_channel(manifest: dict) -> None:
    """Require the generic Scoop workflow inputs to be manifest-declared."""
    channel = _channel_config(manifest, "scoop")
    _require_keys(
        channel,
        ("bucket_repository", "manifest_path", "manifest_template", "binary"),
        "[channels.scoop]",
    )
    for key in ("bucket_repository", "manifest_path", "manifest_template", "binary"):
        if not isinstance(channel[key], str) or not channel[key]:
            raise SystemExit(f"[channels.scoop].{key} must be a non-empty string")


def _channel_asset_patterns(manifest: dict, channel_name: str) -> list[str]:
    project = _require_project(manifest)
    targets = _release_targets_by_name(manifest)
    channel = _channel_config(manifest, channel_name)
    if channel_name == "homebrew":
        assets = channel.get("assets", [])
        if not assets:
            raise SystemExit("[channels.homebrew] must define [[channels.homebrew.assets]]")
        target_names = []
        for asset in assets:
            _require_keys(asset, ("key", "target"), "[[channels.homebrew.assets]]")
            target_names.append(asset["target"])
    elif channel_name in ("winget", "scoop"):
        _require_keys(channel, ("installer_target",), f"[channels.{channel_name}]")
        target_names = [channel["installer_target"]]
    else:
        return []

    renderer_target = _channel_renderer_target(manifest, channel_name)
    if renderer_target is not None:
        target_names.append(renderer_target["target"])

    missing = [name for name in target_names if name not in targets]
    if missing:
        raise SystemExit(
            f"[channels.{channel_name}] references unknown release target(s): {', '.join(missing)}"
        )
    return [
        _release_asset_pattern(project, targets[name])
        for name in dict.fromkeys(target_names)
    ]


def cmd_validate_manifest(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest), with_channel_contracts=True)
    _require_project(manifest)
    _release_targets_by_name(manifest)
    binaries = _release_binaries(manifest)
    channel_names = _channel_names(manifest)
    for channel_name in channel_names:
        _channel_dispatch_config(manifest, channel_name)
        _channel_credential_rehearsal(manifest, channel_name)
        _channel_asset_patterns(manifest, channel_name)
        if channel_name in ("homebrew", "scoop"):
            _renderer_archive_path(manifest)
        # Values only read at publish time must still fail validation early.
        required_channel_strings = {
            "pypi": ("test_repository", "production_repository"),
            "winget": ("identifier",),
        }.get(channel_name, ())
        channel = _channel_config(manifest, channel_name)
        for key in required_channel_strings:
            if not isinstance(channel.get(key), str) or not channel[key]:
                raise SystemExit(f"[channels.{channel_name}].{key} must be a non-empty string")
    if "homebrew" in channel_names:
        _validate_homebrew_bundle_destinations(binaries)
        _validate_homebrew_formulas(
            _channel_config(manifest, "homebrew"),
            {binary["name"] for binary in binaries},
        )
    if "scoop" in channel_names:
        _validate_scoop_channel(manifest)
    seen = set()
    if manifest["crates"]:
        members = workspace_members(Path(args.workspace_toml))
        missing = []
        for crate in manifest["crates"]:
            if crate["cargo_toml"].removesuffix("/Cargo.toml") not in members:
                missing.append(crate["cargo_toml"])
        if missing:
            raise SystemExit(f"manifest references non-member crates: {', '.join(missing)}")
        for crate in manifest["crates"]:
            artifact = crate["artifact"]
            if artifact in seen:
                raise SystemExit(f"duplicate artifact: {artifact}")
            seen.add(artifact)
            actual = package_name(Path(crate["cargo_toml"]))
            if actual != crate["package"]:
                raise SystemExit(f"{crate['cargo_toml']}: package mismatch: manifest={crate['package']} actual={actual}")
    python_artifacts = set()
    python_packages_by_name: dict[str, dict] = {}
    python_distributions_by_name = {entry["name"]: entry for entry in manifest["python_distributions"]}
    for index, package in enumerate(manifest["python_packages"], start=1):
        _require_keys(package, ("artifact", "package", "manifest", "module", "publish"), f"[[python_packages]] #{index}")
        artifact = package["artifact"]
        if artifact in seen or artifact in python_artifacts:
            raise SystemExit(f"duplicate artifact: {artifact}")
        python_artifacts.add(artifact)
        manifest_path = Path(package["manifest"])
        if not manifest_path.is_file():
            raise SystemExit(f"{manifest_path}: missing Python package manifest")
        distribution = python_distributions_by_name.get(package["package"], {})
        cargo_manifest = distribution.get("cargo_manifest")
        python_package_version = _python_project_version(manifest_path)
        if not python_package_version and cargo_manifest:
            cargo_data = tomllib.loads((Path(args.workspace_toml).parent / cargo_manifest).read_text(encoding="utf-8"))
            python_package_version = cargo_data.get("package", {}).get("version")
            if isinstance(python_package_version, dict) and python_package_version.get("workspace") is True:
                python_package_version = workspace_version(Path(args.workspace_toml))
        if not python_package_version:
            raise SystemExit(f"{manifest_path}: missing [project].version")
        actual_package_name = _python_project_name(manifest_path)
        if actual_package_name != package["package"]:
            raise SystemExit(
                f"{manifest_path}: python package mismatch: manifest={package['package']} actual={actual_package_name}"
            )
        python_packages_by_name[package["package"]] = package
    for index, distribution in enumerate(manifest["python_distributions"], start=1):
        _require_keys(distribution, ("name", "source", "sdist", "wheels"), f"[[python_distributions]] #{index}")
        if distribution["name"] not in python_packages_by_name:
            raise SystemExit(
                f"[[python_distributions]] #{index}: no matching [[python_packages]] entry for {distribution['name']}"
            )
        source = Path(distribution["source"])
        if not source.is_dir():
            raise SystemExit(f"[[python_distributions]] #{index}: source directory does not exist: {source}")
        if not isinstance(distribution["sdist"], bool):
            raise SystemExit(f"[[python_distributions]] #{index}: sdist must be a boolean")
        wheels = distribution["wheels"]
        if not isinstance(wheels, list) or not all(isinstance(entry, str) for entry in wheels):
            raise SystemExit(f"[[python_distributions]] #{index}: wheels must be a list of strings")
        cargo_manifest = distribution.get("cargo_manifest")
        if cargo_manifest and not (Path(cargo_manifest)).is_file():
            raise SystemExit(
                f"[[python_distributions]] #{index}: missing Maturin Cargo manifest: {cargo_manifest}"
            )
        package = python_packages_by_name[distribution["name"]]
        module_root = Path(distribution.get("module_path", source / "python" / package["module"]))
        if not module_root.is_dir():
            raise SystemExit(
                f"[[python_distributions]] #{index}: Python module path does not exist: {module_root}"
            )
    # Resolves each distribution's build system; a missing or unsupported
    # build_system is a manifest validation failure.
    _python_distribution_entries(manifest)
    print("manifest validation passed")
    return 0


def cmd_list_publish_plan(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    for crate in manifest["crates"]:
        print(f"{crate['package']}|{crate['wait_after_publish_seconds']}")
    return 0


def _python_matrix_entry(distribution: dict) -> dict[str, str]:
    return {
        "artifact": distribution["artifact"],
        "name": distribution["name"],
        "source": distribution["source"],
        "pyproject": distribution["pyproject"],
        "cargo_manifest": distribution["cargo_manifest"] or "",
        "build_system": distribution["build_system"],
    }


def cmd_python_wheel_matrix(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    # An empty matrix is valid: Rust-only consumers build no Python wheels,
    # and release.yml skips the wheel jobs when the matrix is empty.
    include = [
        {**_python_matrix_entry(distribution), "os": os_name}
        for distribution in _python_distribution_entries(manifest)
        for os_name in distribution["wheels"]
    ]
    print(json.dumps({"include": include}, separators=(",", ":")))
    return 0


def cmd_python_sdist_matrix(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    include = [
        _python_matrix_entry(distribution)
        for distribution in _python_distribution_entries(manifest)
        if distribution["sdist"]
    ]
    print(json.dumps({"include": include}, separators=(",", ":")))
    return 0


def cmd_build_plan(args: argparse.Namespace) -> int:
    """Emit the manifest-derived flags that gate optional build legs."""
    manifest = load_manifest(Path(args.manifest))
    entries = _python_distribution_entries(manifest)
    plan = {
        "has_crates": bool(manifest["crates"]),
        "has_python_wheels": any(entry["wheels"] for entry in entries),
        "has_python_sdists": any(entry["sdist"] for entry in entries),
        "python_upload_tool": manifest_python_upload_tool(manifest),
        "workspace_toml": manifest_workspace_toml(manifest),
        "rust_toolchain": manifest_rust_toolchain(manifest),
    }
    print(json.dumps(plan, separators=(",", ":")))
    return 0


def cmd_release_asset_patterns(args: argparse.Namespace) -> int:
    """Print one required-asset regex per manifest release target."""
    manifest = load_manifest(Path(args.manifest))
    project = _require_project(manifest)
    for target in _release_targets_by_name(manifest).values():
        print(_release_asset_pattern(project, target))
    return 0


def cmd_release_target_matrix(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    print(json.dumps({"include": list(_release_targets_by_name(manifest).values())}, separators=(",", ":")))
    return 0


def cmd_release_package_config(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    targets = _release_targets_by_name(manifest)
    try:
        target = targets[args.target]
    except KeyError as error:
        raise SystemExit(f"unknown release target: {args.target}") from error
    binaries = _release_binaries(manifest)
    print(
        json.dumps(
            {"project": _require_project(manifest), "target": target, "binaries": binaries},
            separators=(",", ":"),
        )
    )
    return 0


def cmd_channel_config(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    project = _require_project(manifest)
    channel = dict(_channel_config(manifest, args.channel))
    if args.channel == "homebrew" and args.tag is not None:
        channel["formulas"] = _homebrew_formulas_for_tag(
            channel,
            args.tag,
            {binary["name"] for binary in _release_binaries(manifest)},
        )
    result = {
        "project": project,
        "channel": channel,
        "asset_patterns": _channel_asset_patterns(manifest, args.channel),
        "release_binaries": manifest["release_binaries"],
        "release_targets": _release_targets_by_name(manifest),
    }
    print(json.dumps(result, separators=(",", ":")))
    return 0


def cmd_channel_dispatch_plan(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest), with_channel_contracts=True)
    channels = []
    for channel_name in _channel_names(manifest):
        workflow, dispatch_inputs = _channel_dispatch_config(manifest, channel_name)
        preflight = _post_release_channel_preflight(manifest, channel_name)
        rehearsal = preflight["credential_rehearsal"]
        rehearsal_plan = None
        if rehearsal is not None:
            rehearsal_plan = {
                "workflow": rehearsal["workflow"],
                "inputs": {"tag": args.tag, **rehearsal["inputs"]},
            }
        channels.append(
            {
                "name": channel_name,
                "agent": preflight["agent"],
                "workflow": workflow,
                "inputs": {"tag": args.tag, **dispatch_inputs},
                "credential_rehearsal": rehearsal_plan,
                "preflight": preflight,
            }
        )
    print(json.dumps({"channels": channels}, separators=(",", ":")))
    return 0


def cmd_preflight_secret_plan(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest), with_channel_contracts=True)
    channel_names = _channel_names(manifest)
    repository_secrets: list[str] = []
    repository_secret_channels: list[dict[str, object]] = []
    liveness_checks: list[dict[str, str]] = []
    liveness_channel_checks: list[dict[str, str]] = []
    environment_secrets: list[dict[str, str]] = []
    root_channels = _root_channel_preflight(manifest)

    for channel in root_channels:
        repository_secrets.extend(channel["repository_secrets"])
        if channel["repository_secrets"]:
            repository_secret_channels.append(
                {"name": channel["name"], "secrets": channel["repository_secrets"]}
            )
        environment_secrets.extend(channel["environment_secrets"])
        liveness_checks.extend(channel["liveness_checks"])
        liveness_channel_checks.extend(
            {"channel": channel["name"], **check}
            for check in channel["liveness_checks"]
        )
    post_release_channels = []
    for channel_name in channel_names:
        channel_preflight = _post_release_channel_preflight(manifest, channel_name)
        repository_secrets.extend(channel_preflight["repository_secrets"])
        if channel_preflight["repository_secrets"]:
            repository_secret_channels.append(
                {"name": channel_name, "secrets": channel_preflight["repository_secrets"]}
            )
        environment_secrets.extend(channel_preflight["environment_secrets"])
        liveness_checks.extend(channel_preflight["liveness_checks"])
        liveness_channel_checks.extend(
            {"channel": channel_name, **check}
            for check in channel_preflight["liveness_checks"]
        )
        post_release_channels.append({"name": channel_name, **channel_preflight})

    # Workflow-consumed GitHub environments are contract-declared so the
    # preflight can verify they exist before any release dispatch.
    github_environments: list[str] = []
    contracts = manifest["channel_contracts"]
    for contract_name in ("crates_io", "github_release", *channel_names):
        for environment in contracts.get(contract_name, {}).get("environments", []):
            if environment not in github_environments:
                github_environments.append(environment)
    for secret in environment_secrets:
        if secret["environment"] not in github_environments:
            github_environments.append(secret["environment"])

    print(
        json.dumps(
            {
                "repository_secrets": repository_secrets,
                "repository_secret_channels": repository_secret_channels,
                "environment_secrets": environment_secrets,
                "github_environments": github_environments,
                "liveness_checks": liveness_checks,
                "liveness_channel_checks": liveness_channel_checks,
                "root_channels": root_channels,
                "post_release_channels": post_release_channels,
            },
            separators=(",", ":"),
        )
    )
    return 0


def _python_distribution_name_from_wheel(path: Path, expected: set[str]) -> str:
    with zipfile.ZipFile(path) as archive:
        metadata = [name for name in archive.namelist() if name.endswith(".dist-info/METADATA")]
        if len(metadata) != 1:
            raise SystemExit(f"{path}: expected exactly one wheel METADATA file")
        name = message_from_bytes(archive.read(metadata[0])).get("Name")
    if name not in expected:
        raise SystemExit(f"{path}: unexpected Python distribution {name!r}")
    return name


def _python_distribution_name_from_sdist(path: Path, expected: set[str]) -> str | None:
    with tarfile.open(path, "r:gz") as archive:
        metadata = [member for member in archive.getmembers() if member.name.endswith("/PKG-INFO")]
        if not metadata:
            return None
        if len(metadata) != 1:
            raise SystemExit(f"{path}: expected exactly one sdist PKG-INFO file")
        extracted = archive.extractfile(metadata[0])
        if extracted is None:
            raise SystemExit(f"{path}: unable to read sdist PKG-INFO")
        name = message_from_bytes(extracted.read()).get("Name")
    if name not in expected:
        raise SystemExit(f"{path}: unexpected Python distribution {name!r}")
    return name


def cmd_verify_python_release_assets(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    asset_dir = Path(args.asset_dir)
    if not asset_dir.is_dir():
        raise SystemExit(f"Python asset directory does not exist: {asset_dir}")
    expected = _python_distribution_expectations(manifest)
    found = {name: {"wheel": 0, "sdist": 0} for name in expected}
    destination = Path(args.copy_to) if args.copy_to else None
    if destination:
        destination.mkdir(parents=True, exist_ok=True)

    for asset in sorted(asset_dir.iterdir()):
        if not asset.is_file():
            continue
        if asset.suffix == ".whl":
            name = _python_distribution_name_from_wheel(asset, set(expected))
            found[name]["wheel"] += 1
        elif asset.name.endswith(".tar.gz"):
            name = _python_distribution_name_from_sdist(asset, set(expected))
            if name is None:
                continue
            found[name]["sdist"] += 1
        else:
            continue
        if destination:
            shutil.copy2(asset, destination / asset.name)

    if found != expected:
        raise SystemExit(
            "published GitHub Release Python assets mismatch: "
            f"expected {expected}, found {found}"
        )
    print(f"verified Python release assets: {expected}")
    return 0


def cmd_verify_version(args: argparse.Namespace) -> int:
    version = workspace_version(Path(args.workspace_toml))
    if version != args.version:
        raise SystemExit(f"workspace version mismatch: expected {args.version}, got {version}")
    manifest = load_manifest(Path(args.manifest))
    for crate in manifest["crates"]:
        data = tomllib.loads(Path(crate["cargo_toml"]).read_text(encoding='utf-8'))
        pkg_version = data["package"]["version"]
        if isinstance(pkg_version, str):
            actual = pkg_version
        elif isinstance(pkg_version, dict) and pkg_version.get("workspace") is True:
            actual = version
        else:
            raise SystemExit(f"{crate['package']}: unsupported version shape: {pkg_version!r}")
        if actual != version:
            raise SystemExit(f"{crate['package']}: version mismatch: expected {version}, got {actual}")
    print("version verification passed")
    return 0


def cmd_verify_version_lockstep(args: argparse.Namespace) -> int:
    workspace_toml = Path(args.workspace_toml)
    version = workspace_version(workspace_toml)
    manifest = load_manifest(Path(args.manifest))
    checked_cargo_manifests: set[str] = set()
    for crate in manifest["crates"]:
        cargo_toml = crate["cargo_toml"]
        _assert_workspace_inherited_version(
            workspace_toml,
            cargo_toml,
            allow_literal_base=not crate.get("publish", True),
        )
        checked_cargo_manifests.add(cargo_toml)
    for distribution in _python_distribution_entries(manifest):
        cargo_toml = distribution["cargo_manifest"]
        if cargo_toml and cargo_toml not in checked_cargo_manifests:
            _assert_workspace_inherited_version(workspace_toml, cargo_toml, allow_literal_base=True)
            checked_cargo_manifests.add(cargo_toml)
    for package in manifest["python_packages"]:
        distribution = next(
            (entry for entry in _python_distribution_entries(manifest) if entry["name"] == package["package"]),
            None,
        )
        _assert_python_package_version(
            workspace_toml,
            package["manifest"],
            version,
            cargo_manifest=distribution["cargo_manifest"] if distribution else None,
        )
    print("version lockstep verification passed")
    return 0


def cmd_verify_python_version(args: argparse.Namespace) -> int:
    version = workspace_version(Path(args.workspace_toml))
    if version != args.version:
        raise SystemExit(f"workspace version mismatch: expected {args.version}, got {version}")
    actual = _python_project_version(Path(args.pyproject))
    if actual != version:
        raise SystemExit(f"python package version mismatch: expected {version}, got {actual}")
    print("python version verification passed")
    return 0


def cmd_sync_python_version(args: argparse.Namespace) -> int:
    version = workspace_version(Path(args.workspace_toml))
    pyproject = Path(args.pyproject)
    lines = pyproject.read_text(encoding="utf-8").splitlines()
    output: list[str] = []
    in_project = False
    updated = False

    for line in lines:
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            in_project = stripped == "[project]"
        if in_project and re.match(r'^\s*version\s*=\s*"[^"]+"\s*$', line):
            output.append(re.sub(r'"[^"]+"', f'"{version}"', line, count=1))
            updated = True
            continue
        output.append(line)

    if not updated:
        raise SystemExit(f"{pyproject}: could not find [project].version to rewrite")

    pyproject.write_text("\n".join(output) + "\n", encoding="utf-8")
    print(f"synced python package version to {version}")
    return 0


def _readme_dependency_crate(manifest: dict) -> str:
    project = manifest["project"]
    dependency_crate = project.get("readme_dependency_crate")
    if not isinstance(dependency_crate, str) or not dependency_crate:
        raise SystemExit("[project].readme_dependency_crate must be a non-empty string")
    if dependency_crate not in {crate["package"] for crate in manifest["crates"]}:
        raise SystemExit(
            "[project].readme_dependency_crate must name a package declared in [[crates]]"
        )
    return dependency_crate


def _readme_version_checks(
    version: str, dependency_crate: str
) -> tuple[tuple[str, str, str], ...]:
    minor_version = version.rsplit(".", 1)[0]
    return (
        (
            f"{dependency_crate} dependency example",
            rf'({re.escape(dependency_crate)}\s*=\s*")[^"]+(")',
            version,
        ),
        ("Status table Version row", rf'(\|\s*Version\s*\|\s*)[^\s|]+(\s*\|)', version),
        ("Status table Stability row", rf'(\|\s*Stability\s*\|\s*stable\s+)\S+(\s+release line\s*\|)', minor_version),
    )


def cmd_verify_readme_version(args: argparse.Namespace) -> int:
    version = workspace_version(Path(args.workspace_toml))
    dependency_crate = _readme_dependency_crate(load_manifest(Path(args.manifest)))
    readme = Path(args.readme)
    text = readme.read_text(encoding="utf-8")

    mismatches = []
    for label, pattern, expected in _readme_version_checks(version, dependency_crate):
        match = re.search(pattern, text)
        if match is None:
            raise SystemExit(f"{readme}: could not locate {label}")
        found = text[match.end(1):match.start(2)]
        if found != expected:
            mismatches.append(f"{label}: expected {expected}, found {found}")

    if mismatches:
        raise SystemExit(
            f"{readme}: stale version reference(s) (run 'sync-readme-version' to fix):\n"
            + "\n".join(mismatches)
        )
    print("readme version verification passed")
    return 0


def cmd_sync_readme_version(args: argparse.Namespace) -> int:
    version = workspace_version(Path(args.workspace_toml))
    dependency_crate = _readme_dependency_crate(load_manifest(Path(args.manifest)))
    readme = Path(args.readme)
    text = readme.read_text(encoding="utf-8")

    updated = 0
    for label, pattern, expected in _readme_version_checks(version, dependency_crate):
        new_text, count = re.subn(pattern, rf'\g<1>{expected}\g<2>', text, count=1)
        if count == 0:
            raise SystemExit(f"{readme}: could not locate {label}")
        text = new_text
        updated += count

    readme.write_text(text, encoding="utf-8")
    print(f"synced {updated} readme version reference(s) to {version}")
    return 0


def cmd_cargo_build_bin_args(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    print(" ".join(f"--bin {entry['name']}" for entry in manifest["release_binaries"]))
    return 0


def cmd_check_version_unpublished(args: argparse.Namespace) -> int:
    """Detect already-published crates via the contract's exact version_lookup_url."""
    manifest = load_manifest(Path(args.manifest), with_channel_contracts=True)
    published = []
    for crate in manifest["crates"]:
        check = _public_registry_checks(
            manifest["channel_contracts"], "crates_io", crate["package"], args.version
        )[0]
        if registry_version_state(check["version_lookup_url"]) == "published":
            published.append(crate["artifact"])
    if published:
        raise SystemExit("release version already published for: " + ", ".join(sorted(published)))
    print(f"ok: no publishable artifacts found at version {args.version}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("validate-manifest")
    p.add_argument("--manifest", required=True)
    p.add_argument("--workspace-toml", required=True)
    p.set_defaults(func=cmd_validate_manifest)

    p = sub.add_parser("validate-publish-order")
    p.add_argument("--manifest", required=True)
    p.add_argument("--workspace-toml", required=True)
    p.set_defaults(func=validate_publish_order)

    p = sub.add_parser("list-publish-plan")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_list_publish_plan)

    p = sub.add_parser("python-wheel-matrix")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_python_wheel_matrix)

    p = sub.add_parser("python-sdist-matrix")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_python_sdist_matrix)

    p = sub.add_parser("build-plan")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_build_plan)

    p = sub.add_parser("release-asset-patterns")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_release_asset_patterns)

    p = sub.add_parser("release-target-matrix")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_release_target_matrix)

    p = sub.add_parser("release-package-config")
    p.add_argument("--manifest", required=True)
    p.add_argument("--target", required=True)
    p.set_defaults(func=cmd_release_package_config)

    p = sub.add_parser("channel-config")
    p.add_argument("--manifest", required=True)
    p.add_argument("--channel", required=True)
    p.add_argument("--tag")
    p.set_defaults(func=cmd_channel_config)

    p = sub.add_parser("channel-dispatch-plan")
    p.add_argument("--manifest", required=True)
    p.add_argument("--tag", required=True)
    p.set_defaults(func=cmd_channel_dispatch_plan)

    p = sub.add_parser("preflight-secret-plan")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_preflight_secret_plan)

    p = sub.add_parser("channel-preflight-results")
    p.add_argument("--manifest", required=True)
    p.add_argument("--outcomes", required=True)
    p.add_argument("--tag", required=True)
    p.set_defaults(func=cmd_channel_preflight_results)

    p = sub.add_parser("public-registry-check-plan")
    p.add_argument("--manifest", required=True)
    p.add_argument("--version", required=True)
    p.set_defaults(func=cmd_public_registry_check_plan)

    p = sub.add_parser("public-registry-inquiry-plan")
    p.add_argument("--contracts", required=True)
    p.add_argument("--channel", choices=("crates_io", "pypi"), required=True)
    p.add_argument("--name", required=True)
    p.add_argument("--version")
    p.set_defaults(func=cmd_public_registry_inquiry_plan)

    p = sub.add_parser("verify-python-release-assets")
    p.add_argument("--manifest", required=True)
    p.add_argument("--asset-dir", required=True)
    p.add_argument("--copy-to")
    p.set_defaults(func=cmd_verify_python_release_assets)

    p = sub.add_parser("verify-version")
    p.add_argument("--manifest", required=True)
    p.add_argument("--workspace-toml", required=True)
    p.add_argument("--version", required=True)
    p.set_defaults(func=cmd_verify_version)

    p = sub.add_parser("verify-python-version")
    p.add_argument("--workspace-toml", required=True)
    p.add_argument("--pyproject", required=True)
    p.add_argument("--version", required=True)
    p.set_defaults(func=cmd_verify_python_version)

    p = sub.add_parser("verify-version-lockstep")
    p.add_argument("--manifest", required=True)
    p.add_argument("--workspace-toml", required=True)
    p.set_defaults(func=cmd_verify_version_lockstep)

    p = sub.add_parser("sync-python-version")
    p.add_argument("--workspace-toml", required=True)
    p.add_argument("--pyproject", required=True)
    p.set_defaults(func=cmd_sync_python_version)

    p = sub.add_parser("verify-readme-version")
    p.add_argument("--manifest", required=True)
    p.add_argument("--workspace-toml", required=True)
    p.add_argument("--readme", required=True)
    p.set_defaults(func=cmd_verify_readme_version)

    p = sub.add_parser("sync-readme-version")
    p.add_argument("--manifest", required=True)
    p.add_argument("--workspace-toml", required=True)
    p.add_argument("--readme", required=True)
    p.set_defaults(func=cmd_sync_readme_version)

    p = sub.add_parser("cargo-build-bin-args")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_cargo_build_bin_args)

    p = sub.add_parser("check-version-unpublished")
    p.add_argument("--manifest", required=True)
    p.add_argument("--version", required=True)
    p.set_defaults(func=cmd_check_version_unpublished)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
