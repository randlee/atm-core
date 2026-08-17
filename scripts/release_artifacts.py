#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import subprocess
import tomllib
from pathlib import Path

from release_manifest import (
    _channel_config,
    _channel_contract,
    _channel_names,
    _channel_preflight_result,
    _homebrew_formulas_for_tag,
    _validate_homebrew_formulas,
    _python_distribution_entries,
    _python_project_name,
    _python_project_version,
    _release_targets_by_name,
    _renderer_archive_path,
    _require_keys,
    _require_project,
    load_channel_contracts,
    load_manifest as _load_publish_manifest,
    package_name,
    workspace_members,
    workspace_version,
)
from release_versions import (
    cmd_sync_python_version,
    cmd_sync_readme_version,
    cmd_verify_python_release_assets,
    cmd_verify_python_version,
    cmd_verify_readme_version,
    cmd_verify_version,
    cmd_verify_version_lockstep,
)
from release_planning import (
    cmd_emit_inventory,
    cmd_list_preflight,
    cmd_list_publish_plan,
    cmd_validate_preflight_checks,
    cmd_validate_publish_order,
    cmd_validate_release_binaries,
)
from release_workspace import (
    crate_is_publishable as _crate_is_publishable,
    missing_publish_metadata_fields as _missing_publish_metadata_fields,
    workspace_dependency_names as _workspace_dependency_names,
    workspace_package_defaults as _workspace_package_defaults,
    workspace_package_map as _workspace_package_map,
    workspace_path_dependencies as _workspace_path_dependencies,
)


def _relative_path(value: object, label: str) -> Path:
    if not isinstance(value, str) or not value:
        raise SystemExit(f"{label} must be a non-empty relative path")
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise SystemExit(f"{label} must be a safe relative path")
    return path


def load_manifest(path: Path, *, with_channel_contracts: bool = False) -> dict:
    """Load generic publish data plus ATM's packaged-document inventory."""
    manifest = _load_publish_manifest(path, with_channel_contracts=with_channel_contracts)
    raw = tomllib.loads(path.read_text(encoding="utf-8"))
    installed_docs = raw.get("installed_docs")
    if not isinstance(installed_docs, dict):
        raise SystemExit("manifest must define [installed_docs]")
    source_root = _relative_path(installed_docs.get("source_root"), "installed_docs.source_root")
    install_root = _relative_path(installed_docs.get("install_root"), "installed_docs.install_root")
    entrypoint = _relative_path(installed_docs.get("entrypoint"), "installed_docs.entrypoint")
    try:
        entrypoint.relative_to(install_root)
    except ValueError as error:
        raise SystemExit("installed_docs.entrypoint must live under installed_docs.install_root") from error
    manifest["installed_docs"] = {
        "source_root": source_root,
        "install_root": install_root,
        "entrypoint": entrypoint,
    }
    return manifest


def manifest_repo_root(manifest_path: Path) -> Path:
    return manifest_path.resolve().parent.parent


def installed_docs_source_root(manifest_path: Path) -> Path:
    return manifest_repo_root(manifest_path) / load_manifest(manifest_path)["installed_docs"]["source_root"]


def installed_doc_source_files(manifest_path: Path) -> list[Path]:
    source_root = installed_docs_source_root(manifest_path)
    if not source_root.is_dir():
        raise SystemExit(f"installed docs source root does not exist: {source_root}")
    return sorted(path for path in source_root.rglob("*") if path.is_file())


def installed_doc_members(manifest_path: Path) -> list[Path]:
    manifest = load_manifest(manifest_path)
    source_root = installed_docs_source_root(manifest_path)
    install_root = manifest["installed_docs"]["install_root"]
    return [install_root / path.relative_to(source_root) for path in installed_doc_source_files(manifest_path)]


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


def _normalize_pypi_name(name: str) -> str:
    """Return the PEP 503 canonical project name used for public lookups."""
    return re.sub(r"[-_.]+", "-", name).lower()


def _url_from_contract(template: str, name: str, version: str) -> str:
    return template.format(name=name, version=version)


def _public_registry_checks(
    contracts: dict[str, dict], channel_name: str, name: str, version: str | None
) -> list[dict[str, str | None]]:
    """Build contract-derived public registry checks for one candidate artifact."""
    try:
        contract = contracts[channel_name]
    except KeyError as error:
        raise SystemExit(f"channel contract missing for {channel_name}") from error
    if not contract.get("public_registry_checks", False):
        raise SystemExit(f"{channel_name} does not support a public registry inquiry")

    normalized_name = _normalize_pypi_name(name) if channel_name == "pypi" else name
    registry_contracts: list[dict[str, str]]
    if channel_name == "crates_io":
        registry_contracts = [
            {
                "name": "crates.io",
                "project_lookup_url": contract["project_lookup_url"],
                "version_lookup_url": contract["version_lookup_url"],
                "version_policy": "must_be_absent",
            }
        ]
    else:
        registry_contracts = contract.get("registries", [])

    checks: list[dict[str, str]] = []
    for registry in registry_contracts:
        check: dict[str, str | None] = {
            "channel": channel_name,
            "agent": contract["agent"],
            "registry": registry["name"],
            "name": name,
            "normalized_name": normalized_name,
            "expected_version": version,
            "project_lookup_url": _url_from_contract(
                registry["project_lookup_url"], normalized_name, version or ""
            ),
            "version_lookup_url": (
                _url_from_contract(registry["version_lookup_url"], normalized_name, version)
                if version
                else None
            ),
            "version_policy": registry["version_policy"],
        }
        checks.append(check)
    return checks


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
    if "homebrew" in channel_names:
        _validate_homebrew_bundle_destinations(binaries)
        _validate_homebrew_formulas(
            _channel_config(manifest, "homebrew"),
            {binary["name"] for binary in binaries},
        )
    if "scoop" in channel_names:
        _validate_scoop_channel(manifest)
    workspace_toml = Path(args.workspace_toml)
    workspace_root = workspace_toml.parent
    members = workspace_members(workspace_toml)
    missing = []
    for crate in manifest["crates"]:
        if crate["cargo_toml"].removesuffix("/Cargo.toml") not in members:
            missing.append(crate["cargo_toml"])
    if missing:
        raise SystemExit(f"manifest references non-member crates: {', '.join(missing)}")
    seen = set()
    for crate in manifest["crates"]:
        artifact = crate["artifact"]
        if artifact in seen:
            raise SystemExit(f"duplicate artifact: {artifact}")
        seen.add(artifact)
        actual = package_name(workspace_root / crate["cargo_toml"])
        if actual != crate["package"]:
            raise SystemExit(f"{crate['cargo_toml']}: package mismatch: manifest={crate['package']} actual={actual}")
    python_artifacts = set()
    python_packages_by_name: dict[str, dict] = {}
    for index, package in enumerate(manifest["python_packages"], start=1):
        _require_keys(package, ("artifact", "package", "manifest", "module", "publish"), f"[[python_packages]] #{index}")
        artifact = package["artifact"]
        if artifact in seen or artifact in python_artifacts:
            raise SystemExit(f"duplicate artifact: {artifact}")
        python_artifacts.add(artifact)
        manifest_path = workspace_root / package["manifest"]
        if not manifest_path.is_file():
            raise SystemExit(f"{manifest_path}: missing Python package manifest")
        _python_project_version(manifest_path)
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
        source = workspace_root / distribution["source"]
        if not source.is_dir():
            raise SystemExit(f"[[python_distributions]] #{index}: source directory does not exist: {source}")
        if not isinstance(distribution["sdist"], bool):
            raise SystemExit(f"[[python_distributions]] #{index}: sdist must be a boolean")
        wheels = distribution["wheels"]
        if not isinstance(wheels, list) or not all(isinstance(entry, str) for entry in wheels):
            raise SystemExit(f"[[python_distributions]] #{index}: wheels must be a list of strings")
        build_system = distribution.get("build_system", "maturin")
        if build_system not in {"maturin", "setuptools"}:
            raise SystemExit(
                f"[[python_distributions]] #{index}: unsupported build_system {build_system!r}"
            )
        if build_system == "maturin":
            cargo_manifest = workspace_root / distribution.get(
                "cargo_manifest", str(Path(distribution["source"]) / "Cargo.toml")
            )
            if not cargo_manifest.is_file():
                raise SystemExit(
                    f"[[python_distributions]] #{index}: missing Maturin Cargo manifest: {cargo_manifest}"
                )
        package = python_packages_by_name[distribution["name"]]
        module_root = workspace_root / distribution.get(
            "module_path", str(Path(distribution["source"]) / "python" / package["module"])
        )
        if not module_root.is_dir():
            raise SystemExit(
                f"[[python_distributions]] #{index}: Python module path does not exist: {module_root}"
            )
    manifest_packages = {crate["package"] for crate in manifest["crates"]}
    published_manifest_packages = {
        crate["package"] for crate in manifest["crates"] if crate["publish"]
    }
    workspace_defaults = _workspace_package_defaults(workspace_toml)
    workspace_packages = _workspace_package_map(workspace_toml)
    omitted = []
    for member in members:
        cargo_toml = workspace_root / member / "Cargo.toml"
        if cargo_toml.is_file() and _crate_is_publishable(cargo_toml):
            package = package_name(cargo_toml)
            if package not in manifest_packages:
                omitted.append(package)
    if omitted:
        raise SystemExit(
            "publishable workspace crate(s) missing from manifest: " + ", ".join(sorted(omitted))
        )

    metadata_errors = []
    for crate in manifest["crates"]:
        if crate["publish"]:
            missing_fields = _missing_publish_metadata_fields(
                workspace_root / crate["cargo_toml"], workspace_defaults
            )
            if missing_fields:
                metadata_errors.append(
                    f"{crate['package']}: missing required publish metadata field(s): "
                    + ", ".join(missing_fields)
                )
    if metadata_errors:
        raise SystemExit("publish metadata violation(s):\n  - " + "\n  - ".join(metadata_errors))

    dependency_errors = []
    for crate in manifest["crates"]:
        if not crate["publish"]:
            continue
        for dependency in sorted(
            _workspace_dependency_names(workspace_root / crate["cargo_toml"], workspace_toml)
        ):
            dependency_toml = workspace_packages.get(dependency)
            if dependency_toml is not None and not _crate_is_publishable(dependency_toml):
                dependency_errors.append(
                    f"{crate['package']} has runtime/build path dependency {dependency} "
                    "whose Cargo.toml sets publish = false"
                )
            elif dependency not in published_manifest_packages:
                dependency_errors.append(
                    f"{crate['package']} has runtime/build path dependency {dependency} "
                    "missing from the publish manifest"
                )
    if dependency_errors:
        raise SystemExit("publish dependency violation(s):\n  - " + "\n  - ".join(dependency_errors))

    print("ok: all publishable workspace crates are present in the manifest")
    print("ok: all publishable manifest crates define required publish metadata")
    print("ok: published crates have publishable runtime/build workspace dependencies")
    return 0


def cmd_python_wheel_matrix(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    include = [
        {
            "artifact": distribution["artifact"],
            "name": distribution["name"],
            "os": os_name,
            "source": distribution["source"],
            "pyproject": distribution["pyproject"],
            "build_system": distribution["build_system"],
            "cargo_manifest": distribution.get("cargo_manifest"),
        }
        for distribution in _python_distribution_entries(manifest)
        for os_name in distribution["wheels"]
    ]
    if not include:
        raise SystemExit("manifest must define at least one Python wheel build")
    print(json.dumps({"include": include}, separators=(",", ":")))
    return 0


def cmd_python_sdist_matrix(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    include = [
        {
            "artifact": distribution["artifact"],
            "name": distribution["name"],
            "source": distribution["source"],
            "pyproject": distribution["pyproject"],
            "build_system": distribution["build_system"],
            "cargo_manifest": distribution.get("cargo_manifest"),
        }
        for distribution in _python_distribution_entries(manifest)
        if distribution["sdist"]
    ]
    print(json.dumps({"include": include}, separators=(",", ":")))
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

    print(
        json.dumps(
            {
                "repository_secrets": repository_secrets,
                "repository_secret_channels": repository_secret_channels,
                "environment_secrets": environment_secrets,
                "liveness_checks": liveness_checks,
                "liveness_channel_checks": liveness_channel_checks,
                "root_channels": root_channels,
                "post_release_channels": post_release_channels,
            },
            separators=(",", ":"),
        )
    )
    return 0


def cmd_cargo_build_bin_args(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    print(" ".join(f"--bin {entry['name']}" for entry in manifest["release_binaries"]))
    return 0


def cargo_search_version_exists(crate: str, version: str) -> bool:
    result = subprocess.run(
        ["cargo", "search", crate, "--limit", "1"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    return f'{crate} = "{version}"' in result.stdout


def cmd_check_version_unpublished(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    published = []
    for crate in manifest["crates"]:
        if cargo_search_version_exists(crate["package"], args.version):
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

    p = sub.add_parser("list-publish-plan")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_list_publish_plan)

    p = sub.add_parser("list-preflight")
    p.add_argument("--manifest", required=True)
    p.add_argument("--mode", choices=("full", "locked"), required=True)
    p.set_defaults(func=cmd_list_preflight)

    p = sub.add_parser("validate-preflight-checks")
    p.add_argument("--manifest", required=True)
    p.add_argument("--workspace-toml", required=True)
    p.set_defaults(func=cmd_validate_preflight_checks)

    p = sub.add_parser("validate-publish-order")
    p.add_argument("--manifest", required=True)
    p.add_argument("--workspace-toml", required=True)
    p.set_defaults(func=cmd_validate_publish_order)

    p = sub.add_parser("validate-release-binaries")
    p.add_argument("--manifest", required=True)
    p.add_argument("--required", action="append", default=[])
    p.set_defaults(func=cmd_validate_release_binaries)

    p = sub.add_parser("emit-inventory")
    p.add_argument("--manifest", required=True)
    p.add_argument("--version", required=True)
    p.add_argument("--tag", required=True)
    p.add_argument("--commit", required=True)
    p.add_argument("--source-ref", required=True)
    p.add_argument("--generated-at")
    p.add_argument("--output", required=True)
    p.set_defaults(func=cmd_emit_inventory)

    p = sub.add_parser("python-wheel-matrix")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_python_wheel_matrix)

    p = sub.add_parser("python-sdist-matrix")
    p.add_argument("--manifest", required=True)
    p.set_defaults(func=cmd_python_sdist_matrix)

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
