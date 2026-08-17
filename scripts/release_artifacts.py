#!/usr/bin/env python3
"""Release artifact manifest utilities for the retained ATM release surface."""

from __future__ import annotations

import argparse
import json
import shutil
import re
import subprocess
import sys
import tomllib
import tarfile
import zipfile
from datetime import datetime, timezone
from email import message_from_bytes
from pathlib import Path

from release_manifest import (
    _channel_config as publish_channel_config,
    _channel_contract as publish_channel_contract,
    _channel_names as publish_channel_names,
    _release_targets_by_name as publish_release_targets_by_name,
    _require_keys as publish_require_keys,
    _require_project as publish_require_project,
    load_channel_contracts as load_publish_channel_contracts,
    load_manifest as load_publish_kit_manifest,
)

PREFLIGHT_FULL = "full"
PREFLIGHT_LOCKED = "locked"
INSTALLED_DOC_REVIEW_FIELD = "reviewed_for_release"


def require_relpath(obj: dict, key: str, label: str) -> Path:
    value = Path(require_str(obj, key, label))
    if value.is_absolute():
        raise SystemExit(f"{label}.{key} must be a relative path")
    return value


def manifest_repo_root(manifest_path: Path) -> Path:
    return manifest_path.resolve().parent.parent


def installed_docs_source_root(manifest_path: Path) -> Path:
    manifest = load_manifest(manifest_path)
    return manifest_repo_root(manifest_path) / manifest["installed_docs"]["source_root"]
HOMEBREW_PLATFORM_TRIPLES = {
    ("on_macos", "on_arm"): "aarch64-apple-darwin",
    ("on_macos", "on_intel"): "x86_64-apple-darwin",
    ("on_linux", "on_intel"): "x86_64-unknown-linux-gnu",
}


def load_manifest(path: Path) -> dict:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    if data.get("schema_version") != 1:
        raise SystemExit("unsupported manifest schema_version")

    crates = data.get("crates")
    if not isinstance(crates, list) or not crates:
        raise SystemExit("manifest must define non-empty [[crates]]")
    binaries = data.get("release_binaries")
    if not isinstance(binaries, list) or not binaries:
        raise SystemExit("manifest must define non-empty [[release_binaries]]")

    required = {
        "artifact",
        "package",
        "cargo_toml",
        "required",
        "publish",
        "publish_order",
        "preflight_check",
        "wait_after_publish_seconds",
        "verify_install",
    }
    seen_artifacts: set[str] = set()
    seen_packages: set[str] = set()
    for idx, crate in enumerate(crates):
        if not isinstance(crate, dict):
            raise SystemExit(f"crates[{idx}] must be a table")
        missing = sorted(required - set(crate))
        if missing:
            raise SystemExit(f"crates[{idx}] missing fields: {', '.join(missing)}")
        artifact = require_str(crate, "artifact", f"crates[{idx}]")
        package = require_str(crate, "package", f"crates[{idx}]")
        require_str(crate, "cargo_toml", f"crates[{idx}]")
        mode = require_str(crate, "preflight_check", f"crates[{idx}]")
        if mode not in {PREFLIGHT_FULL, PREFLIGHT_LOCKED}:
            raise SystemExit(f"{artifact}: invalid preflight_check {mode!r}")
        if artifact in seen_artifacts:
            raise SystemExit(f"duplicate artifact {artifact}")
        if package in seen_packages:
            raise SystemExit(f"duplicate package {package}")
        seen_artifacts.add(artifact)
        seen_packages.add(package)

    seen_bins: set[str] = set()
    for idx, entry in enumerate(binaries):
        if not isinstance(entry, dict):
            raise SystemExit(f"release_binaries[{idx}] must be a table")
        name = require_str(entry, "name", f"release_binaries[{idx}]")
        if name in seen_bins:
            raise SystemExit(f"duplicate release binary {name}")
        seen_bins.add(name)

    installed_docs = data.get("installed_docs")
    if not isinstance(installed_docs, dict):
        raise SystemExit("manifest must define [installed_docs]")
    source_root = require_relpath(installed_docs, "source_root", "installed_docs")
    install_root = require_relpath(installed_docs, "install_root", "installed_docs")
    entrypoint = require_relpath(installed_docs, "entrypoint", "installed_docs")
    try:
        entrypoint.relative_to(install_root)
    except ValueError as exc:
        raise SystemExit("installed_docs.entrypoint must live under installed_docs.install_root") from exc

    crates.sort(key=lambda item: (item["publish_order"], item["artifact"]))
    return {
        "crates": crates,
        "release_binaries": binaries,
        "installed_docs": {
            "source_root": source_root,
            "install_root": install_root,
            "entrypoint": entrypoint,
        },
    }


def require_str(obj: dict, key: str, label: str) -> str:
    value = obj.get(key)
    if not isinstance(value, str) or not value.strip():
        raise SystemExit(f"{label}.{key} must be a non-empty string")
    return value


def cargo_search_version_exists(crate: str, version: str) -> bool:
    result = subprocess.run(
        ["cargo", "search", crate, "--limit", "1"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    return f'{crate} = "{version}"' in result.stdout


def emit_inventory(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    generated_at = args.generated_at or datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    items = []
    for crate in manifest["crates"]:
        if not crate["publish"]:
            continue
        verify = [f'cargo search {crate["package"]} --limit 1 | grep -F \'{crate["package"]} = "{args.version}"\'']
        if crate["verify_install"]:
            verify.append(f"cargo install {crate['package']} --version {args.version} --locked --force")
        items.append(
            {
                "artifact": crate["artifact"],
                "version": args.version,
                "sourceRef": args.source_ref,
                "publishTarget": "crates.io",
                "required": crate["required"],
                "publish": crate["publish"],
                "verifyCommands": verify,
            }
        )
    items.sort(key=lambda item: item["artifact"])
    payload = {
        "releaseVersion": args.version,
        "releaseTag": args.tag,
        "releaseCommit": args.commit,
        "generatedAt": generated_at,
        "items": items,
    }
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return 0


def list_cargo_tomls(args: argparse.Namespace) -> int:
    for crate in load_manifest(Path(args.manifest))["crates"]:
        print(crate["cargo_toml"])
    return 0


def list_artifacts(args: argparse.Namespace) -> int:
    for crate in load_manifest(Path(args.manifest))["crates"]:
        if args.publishable_only and not crate["publish"]:
            continue
        print(crate["artifact"])
    return 0


def list_preflight(args: argparse.Namespace) -> int:
    for crate in load_manifest(Path(args.manifest))["crates"]:
        if crate["publish"] and crate["preflight_check"] == args.mode:
            print(crate["package"])
    return 0


def list_publish_plan(args: argparse.Namespace) -> int:
    crates = [crate for crate in load_manifest(Path(args.manifest))["crates"] if crate["publish"]]
    for crate in crates:
        print(f'{crate["package"]}|{crate["wait_after_publish_seconds"]}')
    return 0


def list_release_binaries(args: argparse.Namespace) -> int:
    for entry in load_manifest(Path(args.manifest))["release_binaries"]:
        print(entry["name"])
    return 0


def installed_doc_source_files(manifest_path: Path) -> list[Path]:
    source_root = installed_docs_source_root(manifest_path)
    if not source_root.is_dir():
        raise SystemExit(f"installed docs source root does not exist: {source_root}")
    return sorted(path for path in source_root.rglob("*") if path.is_file())


def installed_doc_members(manifest_path: Path) -> list[Path]:
    manifest = load_manifest(manifest_path)
    source_root = installed_docs_source_root(manifest_path)
    install_root = manifest["installed_docs"]["install_root"]
    members: list[Path] = []
    for path in installed_doc_source_files(manifest_path):
        members.append(install_root / path.relative_to(source_root))
    return members


def list_installed_doc_members(args: argparse.Namespace) -> int:
    for member in installed_doc_members(Path(args.manifest)):
        print(member.as_posix())
    return 0


def stage_install_docs(args: argparse.Namespace) -> int:
    manifest_path = Path(args.manifest)
    manifest = load_manifest(manifest_path)
    repo_root = manifest_repo_root(manifest_path)
    source_root = repo_root / manifest["installed_docs"]["source_root"]
    install_root = Path(args.output_root) / manifest["installed_docs"]["install_root"]
    if install_root.exists():
        shutil.rmtree(install_root)
    install_root.mkdir(parents=True, exist_ok=True)
    for source_path in installed_doc_source_files(manifest_path):
        destination = install_root / source_path.relative_to(source_root)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_path, destination)
    print(install_root)
    return 0


def validate_release_binaries(args: argparse.Namespace) -> int:
    binaries = {entry["name"] for entry in load_manifest(Path(args.manifest))["release_binaries"]}
    missing = [name for name in args.required if name not in binaries]
    if missing:
        print("missing required release binaries:")
        for name in missing:
            print(f"  - {name}")
        return 1
    print("ok: required release binaries are present in the manifest")
    return 0


def cargo_build_bin_args(args: argparse.Namespace) -> int:
    print(" ".join(f'--bin {entry["name"]}' for entry in load_manifest(Path(args.manifest))["release_binaries"]))
    return 0


def check_version_unpublished(args: argparse.Namespace) -> int:
    published = []
    for crate in load_manifest(Path(args.manifest))["crates"]:
        if crate["publish"] and cargo_search_version_exists(crate["package"], args.version):
            published.append(crate["artifact"])
    if published:
        raise SystemExit("release version already published for: " + ", ".join(sorted(published)))
    print(f"ok: no publishable artifacts found at version {args.version}")
    return 0


def workspace_members(workspace_toml: Path) -> list[str]:
    data = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    members = data.get("workspace", {}).get("members", [])
    if not isinstance(members, list):
        raise SystemExit("Cargo.toml [workspace].members must be a list")
    return members


def crate_name(crate_toml: Path) -> str | None:
    data = tomllib.loads(crate_toml.read_text(encoding="utf-8"))
    return data.get("package", {}).get("name")


def crate_is_publishable(crate_toml: Path) -> bool:
    data = tomllib.loads(crate_toml.read_text(encoding="utf-8"))
    publish = data.get("package", {}).get("publish")
    if publish is False:
        return False
    if isinstance(publish, list) and len(publish) == 0:
        return False
    return True


def workspace_package_defaults(workspace_toml: Path) -> dict:
    data = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    package = data.get("workspace", {}).get("package", {})
    if not isinstance(package, dict):
        return {}
    return package


def package_field_value(
    package: dict,
    field: str,
    *,
    workspace_defaults: dict,
) -> str | None:
    value = package.get(field)
    if isinstance(value, str) and value.strip():
        return value.strip()
    if isinstance(value, dict) and value.get("workspace") is True:
        inherited = workspace_defaults.get(field)
        if isinstance(inherited, str) and inherited.strip():
            return inherited.strip()
    return None


def missing_publish_metadata_fields(crate_toml: Path, workspace_defaults: dict) -> list[str]:
    data = tomllib.loads(crate_toml.read_text(encoding="utf-8"))
    package = data.get("package", {})
    if not isinstance(package, dict):
        return ["package"]

    missing: list[str] = []
    if package_field_value(package, "description", workspace_defaults=workspace_defaults) is None:
        missing.append("description")

    license_value = package_field_value(package, "license", workspace_defaults=workspace_defaults)
    license_file_value = package_field_value(package, "license-file", workspace_defaults=workspace_defaults)
    if license_value is None and license_file_value is None:
        missing.append("license or license-file")
    return missing


def load_publish_kit(path: Path, *, with_channel_contracts: bool = False) -> dict:
    """Load the repository-owned release inventory plus the shared channel contract."""
    return load_publish_kit_manifest(path, with_channel_contracts=with_channel_contracts)


def publish_asset_pattern(project: dict, target: dict) -> str:
    return (
        rf"^{re.escape(project['archive_prefix'])}_.*_"
        rf"{re.escape(target['target'])}\.{re.escape(target['archive'])}$"
    )


def publish_channel_asset_patterns(manifest: dict, channel_name: str) -> list[str]:
    project = publish_require_project(manifest)
    targets = publish_release_targets_by_name(manifest)
    channel = publish_channel_config(manifest, channel_name)
    if channel_name == "homebrew":
        target_names = [asset["target"] for asset in channel.get("assets", [])]
        if not target_names:
            raise SystemExit("[channels.homebrew] must define [[channels.homebrew.assets]]")
    elif channel_name in {"winget", "scoop"}:
        publish_require_keys(channel, ("installer_target",), f"[channels.{channel_name}]")
        target_names = [channel["installer_target"]]
    else:
        return []
    if channel_name in {"homebrew", "scoop"}:
        renderer_target = channel.get("renderer_target")
        if not isinstance(renderer_target, str) or not renderer_target:
            raise SystemExit(f"[channels.{channel_name}].renderer_target must be a non-empty string")
        target_names.append(renderer_target)
    missing = [target for target in target_names if target not in targets]
    if missing:
        raise SystemExit(
            f"[channels.{channel_name}] references unknown release target(s): {', '.join(missing)}"
        )
    return [publish_asset_pattern(project, targets[name]) for name in dict.fromkeys(target_names)]


def validate_publish_kit_manifest(manifest_path: Path, workspace_toml: Path) -> None:
    """Validate the generic publish-kit data without embedding project release facts."""
    manifest = load_publish_kit(manifest_path, with_channel_contracts=True)
    publish_require_project(manifest)
    publish_release_targets_by_name(manifest)
    if not manifest["release_binaries"]:
        raise SystemExit("manifest must define [[release_binaries]]")
    for index, binary in enumerate(manifest["release_binaries"], start=1):
        publish_require_keys(binary, ("name",), f"[[release_binaries]] #{index}")
        for bundle in binary.get("bundled_paths", []):
            publish_require_keys(bundle, ("source", "destination"), "bundled_paths entry")
            if not Path(bundle["source"]).is_dir():
                raise SystemExit(f"bundled path source does not exist: {bundle['source']}")

    for channel_name in publish_channel_names(manifest):
        channel = publish_channel_config(manifest, channel_name)
        publish_require_keys(channel, ("workflow", "dispatch_inputs"), f"[channels.{channel_name}]")
        if not isinstance(channel["workflow"], str) or not channel["workflow"]:
            raise SystemExit(f"[channels.{channel_name}].workflow must be a non-empty string")
        if not isinstance(channel["dispatch_inputs"], dict):
            raise SystemExit(f"[channels.{channel_name}].dispatch_inputs must be a table")
        publish_channel_asset_patterns(manifest, channel_name)

    workspace_version_value = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    python_packages = {package["package"]: package for package in manifest["python_packages"]}
    for index, package in enumerate(manifest["python_packages"], start=1):
        publish_require_keys(
            package,
            ("artifact", "package", "manifest", "module", "publish"),
            f"[[python_packages]] #{index}",
        )
        pyproject = Path(package["manifest"])
        if not pyproject.is_file():
            raise SystemExit(f"{pyproject}: missing Python package manifest")
        project = tomllib.loads(pyproject.read_text(encoding="utf-8")).get("project", {})
        if project.get("name") != package["package"]:
            raise SystemExit(f"{pyproject}: package name does not match the release manifest")
        version = project.get("version")
        dynamic = project.get("dynamic", [])
        if version is not None and version != workspace_version_value:
            raise SystemExit(f"{pyproject}: version must match workspace.package.version")
        if version is None and "version" not in dynamic:
            raise SystemExit(f"{pyproject}: project version must be static or declared dynamic")

    for index, distribution in enumerate(manifest["python_distributions"], start=1):
        publish_require_keys(distribution, ("name", "source", "sdist", "wheels"), f"[[python_distributions]] #{index}")
        if distribution["name"] not in python_packages:
            raise SystemExit(f"[[python_distributions]] #{index}: unknown Python package")
        if not Path(distribution["source"]).is_dir():
            raise SystemExit(f"[[python_distributions]] #{index}: source directory does not exist")
        if not isinstance(distribution["sdist"], bool) or not isinstance(distribution["wheels"], list):
            raise SystemExit(f"[[python_distributions]] #{index}: invalid build matrix")
        build_system = distribution.get("build_system", "maturin")
        if build_system not in {"maturin", "setuptools"}:
            raise SystemExit(f"[[python_distributions]] #{index}: unsupported build_system {build_system!r}")
        if build_system == "maturin" and not Path(distribution.get("cargo_manifest", "")).is_file():
            raise SystemExit(f"[[python_distributions]] #{index}: missing Maturin Cargo manifest")
        module_path = Path(distribution.get("module_path", ""))
        if not module_path.is_dir():
            raise SystemExit(f"[[python_distributions]] #{index}: Python module path does not exist")


def publish_channel_preflight(manifest: dict, channel_name: str) -> dict[str, object]:
    contract = publish_channel_contract(manifest, channel_name)
    return {
        "agent": contract["agent"],
        "repository_secrets": contract.get("repository_secrets", []),
        "environment_secrets": contract.get("environment_secrets", []),
        "liveness_checks": contract.get("liveness_checks", []),
        "github_actions_permissions": contract.get("github_actions_permissions", []),
        "public_registry_checks": contract.get("public_registry_checks", False),
    }


def channel_config(args: argparse.Namespace) -> int:
    manifest = load_publish_kit(Path(args.manifest), with_channel_contracts=True)
    channel = publish_channel_config(manifest, args.channel)
    print(
        json.dumps(
            {
                "project": publish_require_project(manifest),
                "channel": channel,
                "preflight": publish_channel_preflight(manifest, args.channel),
                "asset_patterns": publish_channel_asset_patterns(manifest, args.channel),
                "release_binaries": manifest["release_binaries"],
                "release_targets": publish_release_targets_by_name(manifest),
            },
            separators=(",", ":"),
        )
    )
    return 0


def release_target_matrix(args: argparse.Namespace) -> int:
    manifest = load_publish_kit(Path(args.manifest))
    print(
        json.dumps(
            {"include": list(publish_release_targets_by_name(manifest).values())},
            separators=(",", ":"),
        )
    )
    return 0


def release_package_config(args: argparse.Namespace) -> int:
    manifest = load_publish_kit(Path(args.manifest))
    targets = publish_release_targets_by_name(manifest)
    if args.target not in targets:
        raise SystemExit(f"unknown release target: {args.target}")
    print(
        json.dumps(
            {
                "project": publish_require_project(manifest),
                "target": targets[args.target],
                "binaries": manifest["release_binaries"],
            },
            separators=(",", ":"),
        )
    )
    return 0


def python_distribution_entries(manifest: dict) -> list[dict[str, object]]:
    packages = {package["package"]: package for package in manifest["python_packages"]}
    return [
        {
            "artifact": packages[distribution["name"]]["artifact"],
            "name": distribution["name"],
            "pyproject": packages[distribution["name"]]["manifest"],
            "source": distribution["source"],
            "build_system": distribution.get("build_system", "maturin"),
            "cargo_manifest": distribution.get("cargo_manifest"),
            "sdist": distribution["sdist"],
            "wheels": distribution["wheels"],
        }
        for distribution in manifest["python_distributions"]
    ]


def python_wheel_matrix(args: argparse.Namespace) -> int:
    manifest = load_publish_kit(Path(args.manifest))
    include = [
        {
            key: value
            for key, value in distribution.items()
            if key not in {"sdist", "wheels"} and value is not None
        }
        | {"os": os_name}
        for distribution in python_distribution_entries(manifest)
        for os_name in distribution["wheels"]
    ]
    if not include:
        raise SystemExit("manifest must define at least one Python wheel build")
    print(json.dumps({"include": include}, separators=(",", ":")))
    return 0


def python_sdist_matrix(args: argparse.Namespace) -> int:
    manifest = load_publish_kit(Path(args.manifest))
    include = [
        {
            key: value
            for key, value in distribution.items()
            if key not in {"sdist", "wheels"} and value is not None
        }
        for distribution in python_distribution_entries(manifest)
        if distribution["sdist"]
    ]
    print(json.dumps({"include": include}, separators=(",", ":")))
    return 0


def python_distribution_name_from_wheel(path: Path, expected: set[str]) -> str:
    with zipfile.ZipFile(path) as archive:
        metadata = [name for name in archive.namelist() if name.endswith(".dist-info/METADATA")]
        if len(metadata) != 1:
            raise SystemExit(f"{path}: expected exactly one wheel METADATA file")
        name = message_from_bytes(archive.read(metadata[0])).get("Name")
    if name not in expected:
        raise SystemExit(f"{path}: unexpected Python distribution {name!r}")
    return name


def python_distribution_name_from_sdist(path: Path, expected: set[str]) -> str | None:
    with tarfile.open(path, "r:gz") as archive:
        metadata = [member for member in archive.getmembers() if member.name.endswith("/PKG-INFO")]
        if not metadata:
            return None
        if len(metadata) != 1:
            raise SystemExit(f"{path}: expected exactly one sdist PKG-INFO file")
        contents = archive.extractfile(metadata[0])
        if contents is None:
            raise SystemExit(f"{path}: unable to read sdist PKG-INFO")
        name = message_from_bytes(contents.read()).get("Name")
    if name not in expected:
        raise SystemExit(f"{path}: unexpected Python distribution {name!r}")
    return name


def verify_python_release_assets(args: argparse.Namespace) -> int:
    manifest = load_publish_kit(Path(args.manifest))
    asset_dir = Path(args.asset_dir)
    if not asset_dir.is_dir():
        raise SystemExit(f"Python asset directory does not exist: {asset_dir}")
    expected = {
        entry["name"]: {"wheel": len(entry["wheels"]), "sdist": int(entry["sdist"])}
        for entry in python_distribution_entries(manifest)
    }
    found = {name: {"wheel": 0, "sdist": 0} for name in expected}
    destination = Path(args.copy_to) if args.copy_to else None
    if destination:
        destination.mkdir(parents=True, exist_ok=True)
    for asset in sorted(asset_dir.iterdir()):
        if not asset.is_file():
            continue
        if asset.suffix == ".whl":
            name = python_distribution_name_from_wheel(asset, set(expected))
            found[name]["wheel"] += 1
        elif asset.name.endswith(".tar.gz"):
            name = python_distribution_name_from_sdist(asset, set(expected))
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


def normalize_pypi_name(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def public_registry_checks(
    contracts: dict[str, dict], channel_name: str, name: str, version: str | None
) -> list[dict[str, str | None]]:
    try:
        contract = contracts[channel_name]
    except KeyError as error:
        raise SystemExit(f"channel contract missing for {channel_name}") from error
    if not contract.get("public_registry_checks", False):
        raise SystemExit(f"{channel_name} does not support a public registry inquiry")
    normalized_name = normalize_pypi_name(name) if channel_name == "pypi" else name
    registries = (
        [
            {
                "name": "crates.io",
                "project_lookup_url": contract["project_lookup_url"],
                "version_lookup_url": contract["version_lookup_url"],
                "version_policy": "must_be_absent",
            }
        ]
        if channel_name == "crates_io"
        else contract.get("registries", [])
    )
    return [
        {
            "channel": channel_name,
            "agent": contract["agent"],
            "registry": registry["name"],
            "name": name,
            "normalized_name": normalized_name,
            "expected_version": version,
            "project_lookup_url": registry["project_lookup_url"].format(
                name=normalized_name, version=version or ""
            ),
            "version_lookup_url": (
                registry["version_lookup_url"].format(name=normalized_name, version=version)
                if version
                else None
            ),
            "version_policy": registry["version_policy"],
        }
        for registry in registries
    ]


def public_registry_check_plan(args: argparse.Namespace) -> int:
    manifest = load_publish_kit(Path(args.manifest), with_channel_contracts=True)
    checks = [
        *(
            check
            for crate in manifest["crates"]
            if crate.get("publish", True)
            for check in public_registry_checks(
                manifest["channel_contracts"], "crates_io", crate["package"], args.version
            )
        ),
        *(
            check
            for distribution in python_distribution_entries(manifest)
            for check in public_registry_checks(
                manifest["channel_contracts"], "pypi", distribution["name"], args.version
            )
        ),
    ]
    print(json.dumps({"checks": checks}, separators=(",", ":")))
    return 0


def public_registry_inquiry_plan(args: argparse.Namespace) -> int:
    contracts = load_publish_channel_contracts(Path(args.contracts))
    print(
        json.dumps(
            {"checks": public_registry_checks(contracts, args.channel, args.name, args.version)},
            separators=(",", ":"),
        )
    )
    return 0


def verify_version(args: argparse.Namespace) -> int:
    workspace_toml = Path(args.workspace_toml)
    workspace = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    version = workspace["workspace"]["package"]["version"]
    if version != args.version:
        raise SystemExit(f"workspace version mismatch: expected {args.version}, got {version}")
    manifest = load_publish_kit(Path(args.manifest))
    for crate in manifest["crates"]:
        package = tomllib.loads(Path(crate["cargo_toml"]).read_text(encoding="utf-8"))["package"]
        declared = package["version"]
        actual = version if isinstance(declared, dict) and declared.get("workspace") else declared
        if actual != version:
            raise SystemExit(f"{crate['package']}: version mismatch: expected {version}, got {actual}")
    print("version verification passed")
    return 0


def verify_version_lockstep(args: argparse.Namespace) -> int:
    workspace_toml = Path(args.workspace_toml)
    version = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    manifest = load_publish_kit(Path(args.manifest))
    for crate in manifest["crates"]:
        package = tomllib.loads(Path(crate["cargo_toml"]).read_text(encoding="utf-8"))["package"]
        declared = package["version"]
        actual = version if isinstance(declared, dict) and declared.get("workspace") else declared
        if actual != version:
            raise SystemExit(f"{crate['cargo_toml']}: version mismatch: expected {version}, got {actual}")
    for distribution in python_distribution_entries(manifest):
        pyproject = tomllib.loads(Path(distribution["pyproject"]).read_text(encoding="utf-8"))
        project = pyproject["project"]
        if "version" in project and project["version"] != version:
            raise SystemExit(f"{distribution['pyproject']}: version mismatch: expected {version}")
        if "version" not in project and "version" not in project.get("dynamic", []):
            raise SystemExit(f"{distribution['pyproject']}: missing version metadata")
    print("version lockstep verification passed")
    return 0


def preflight_secret_plan(args: argparse.Namespace) -> int:
    manifest = load_publish_kit(Path(args.manifest), with_channel_contracts=True)
    root_channels = [
        {"name": name, **publish_channel_preflight(manifest, name)}
        for name in ("crates_io", "github_release")
    ]
    post_release_channels = [
        {"name": name, **publish_channel_preflight(manifest, name)}
        for name in publish_channel_names(manifest)
    ]
    all_channels = [*root_channels, *post_release_channels]
    print(
        json.dumps(
            {
                "repository_secrets": [
                    secret for channel in all_channels for secret in channel["repository_secrets"]
                ],
                "environment_secrets": [
                    secret for channel in all_channels for secret in channel["environment_secrets"]
                ],
                "liveness_checks": [
                    check for channel in all_channels for check in channel["liveness_checks"]
                ],
                "root_channels": root_channels,
                "post_release_channels": post_release_channels,
            },
            separators=(",", ":"),
        )
    )
    return 0


def channel_preflight_results(args: argparse.Namespace) -> int:
    try:
        outcomes = json.loads(args.outcomes)
    except json.JSONDecodeError as error:
        raise SystemExit(f"invalid preflight outcomes JSON: {error.msg}") from error
    if not isinstance(outcomes, dict) or not all(isinstance(value, str) for value in outcomes.values()):
        raise SystemExit("preflight outcomes must be a string-to-string object")

    def status(step: str) -> str:
        outcome = outcomes.get(step)
        if outcome == "success":
            return "passed"
        if outcome in {"failure", "cancelled"}:
            return "failed"
        return "blocked"

    manifest = load_publish_kit(Path(args.manifest), with_channel_contracts=True)
    channels = [
        {"name": name, **publish_channel_preflight(manifest, name)}
        for name in ("crates_io", "github_release", *publish_channel_names(manifest))
    ]
    results = []
    for channel in channels:
        checks = [
            {"kind": "release_authorization", "status": status("ownership")},
            {"kind": "release_metadata", "status": status("release_metadata")},
        ]
        for field, step in (
            ("repository_secrets", "repository_secrets"),
            ("environment_secrets", "environment_secrets"),
            ("liveness_checks", "credential_liveness"),
            ("github_actions_permissions", "github_release_permissions"),
            ("public_registry_checks", "registry_state"),
        ):
            requirement = channel[field]
            if requirement:
                checks.append({"kind": field, "status": status(step)})
        result_status = "failed" if any(check["status"] == "failed" for check in checks) else (
            "blocked" if any(check["status"] == "blocked" for check in checks) else "passed"
        )
        results.append(
            {
                "name": channel["name"],
                "tag": args.tag or None,
                "status": result_status,
                "checks": checks,
                "sanitized_diagnostic": "" if result_status == "passed" else "PREFLIGHT.CHECK_INCOMPLETE",
            }
        )
    print(json.dumps({"tag": args.tag or None, "channels": results}, separators=(",", ":")))
    return 0


def channel_dispatch_plan(args: argparse.Namespace) -> int:
    manifest = load_publish_kit(Path(args.manifest), with_channel_contracts=True)
    channels = []
    for name in publish_channel_names(manifest):
        channel = publish_channel_config(manifest, name)
        dispatch_inputs = channel["dispatch_inputs"]
        if "tag" in dispatch_inputs:
            raise SystemExit(f"[channels.{name}].dispatch_inputs must not override tag")
        rehearsal = channel.get("credential_rehearsal_inputs")
        if rehearsal is not None and "tag" in rehearsal:
            raise SystemExit(f"[channels.{name}].credential_rehearsal_inputs must not override tag")
        channels.append(
            {
                "name": name,
                "agent": publish_channel_preflight(manifest, name)["agent"],
                "workflow": channel["workflow"],
                "inputs": {"tag": args.tag, **dispatch_inputs},
                "credential_rehearsal": (
                    {"workflow": channel["workflow"], "inputs": {"tag": args.tag, **rehearsal}}
                    if rehearsal is not None
                    else None
                ),
                "preflight": publish_channel_preflight(manifest, name),
            }
        )
    print(json.dumps({"channels": channels}, separators=(",", ":")))
    return 0


def validate_manifest(args: argparse.Namespace) -> int:
    manifest_path = Path(args.manifest)
    raw_manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    # Keep the pre-kit schema readable for historical fixture validation while
    # enforcing the channel contract whenever a repository adopts the kit.
    if "project" in raw_manifest or "channels" in raw_manifest:
        validate_publish_kit_manifest(manifest_path, Path(args.workspace_toml))
    manifest = load_manifest(manifest_path)
    manifest_packages = {crate["package"] for crate in manifest["crates"]}
    published_manifest_packages = {
        crate["package"] for crate in manifest["crates"] if crate["publish"]
    }
    workspace_toml = Path(args.workspace_toml)
    workspace_root = workspace_toml.parent
    workspace_defaults = workspace_package_defaults(workspace_toml)
    workspace_packages = workspace_package_map(workspace_toml)
    missing = []
    for member in workspace_members(workspace_toml):
        crate_toml = workspace_root / member / "Cargo.toml"
        if not crate_toml.exists() or not crate_is_publishable(crate_toml):
            continue
        name = crate_name(crate_toml)
        if name and name not in manifest_packages:
            missing.append(name)
            print(f"MISSING: {name}")
    if missing:
        print(f"\n{len(missing)} publishable crate(s) missing from manifest.", file=sys.stderr)
        return 1

    metadata_errors = []
    for crate in manifest["crates"]:
        if not crate["publish"]:
            continue
        crate_toml = workspace_root / crate["cargo_toml"]
        missing_fields = missing_publish_metadata_fields(crate_toml, workspace_defaults)
        if missing_fields:
            metadata_errors.append(
                f"{crate['package']}: missing required publish metadata field(s): {', '.join(missing_fields)}"
            )
    if metadata_errors:
        print("publish metadata violation(s):")
        for error in metadata_errors:
            print(f"  - {error}")
        return 1

    dependency_errors = []
    for crate in manifest["crates"]:
        if not crate["publish"]:
            continue
        crate_toml = workspace_root / crate["cargo_toml"]
        for dependency in sorted(workspace_dependency_names(crate_toml, workspace_root)):
            dependency_toml = workspace_packages.get(dependency)
            if dependency_toml is None:
                continue
            if not crate_is_publishable(dependency_toml):
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
        print("publish dependency violation(s):")
        for error in dependency_errors:
            print(f"  - {error}")
        return 1

    print("ok: all publishable workspace crates are present in the manifest")
    print("ok: all publishable manifest crates define required publish metadata")
    print("ok: published crates have publishable runtime/build workspace dependencies")
    return 0


def has_workspace_path_deps(crate_toml: Path, workspace_root: Path) -> list[str]:
    data = tomllib.loads(crate_toml.read_text(encoding="utf-8"))
    ws_toml = workspace_root / "Cargo.toml"
    ws_data = tomllib.loads(ws_toml.read_text(encoding="utf-8")) if ws_toml.exists() else {}
    workspace_deps = ws_data.get("workspace", {}).get("dependencies", {})
    crate_dir = crate_toml.parent
    deps: list[str] = []

    def check_table(table: object) -> None:
        if not isinstance(table, dict):
            return
        for dep_name, dep_spec in table.items():
            if isinstance(dep_spec, dict):
                if dep_spec.get("workspace") is True:
                    ws_dep = workspace_deps.get(dep_name, {})
                    if isinstance(ws_dep, dict) and "path" in ws_dep:
                        deps.append(dep_name)
                elif "path" in dep_spec:
                    dep_path = (crate_dir / dep_spec["path"]).resolve()
                    if dep_path.is_relative_to(workspace_root.resolve()):
                        deps.append(dep_name)

    check_table(data.get("dependencies", {}))
    check_table(data.get("build-dependencies", {}))
    for target_data in data.get("target", {}).values():
        if isinstance(target_data, dict):
            check_table(target_data.get("dependencies", {}))
            check_table(target_data.get("build-dependencies", {}))
    return sorted(set(deps))


def validate_preflight_checks(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    workspace_root = Path(args.workspace_toml).parent
    errors = []
    for crate in manifest["crates"]:
        if crate["preflight_check"] != PREFLIGHT_FULL:
            continue
        crate_toml = workspace_root / crate["cargo_toml"]
        path_deps = has_workspace_path_deps(crate_toml, workspace_root)
        if path_deps:
            errors.append(
                f"{crate['artifact']} has workspace path deps ({', '.join(path_deps)}) but preflight_check='full'"
            )
    if errors:
        for error in errors:
            print(error)
        return 1
    print("ok: all preflight_check='full' crates are genuine leaf crates")
    return 0


def workspace_package_map(workspace_toml: Path) -> dict[str, Path]:
    root = workspace_toml.parent
    mapping = {}
    for member in workspace_members(workspace_toml):
        crate_toml = root / member / "Cargo.toml"
        if crate_toml.exists():
            name = crate_name(crate_toml)
            if name:
                mapping[name] = crate_toml
    return mapping


def workspace_dependency_names(crate_toml: Path, workspace_root: Path) -> set[str]:
    data = tomllib.loads(crate_toml.read_text(encoding="utf-8"))
    ws_toml = workspace_root / "Cargo.toml"
    ws_data = tomllib.loads(ws_toml.read_text(encoding="utf-8")) if ws_toml.exists() else {}
    workspace_deps = ws_data.get("workspace", {}).get("dependencies", {})
    workspace_packages = set(workspace_package_map(ws_toml).keys()) if ws_toml.exists() else set()
    crate_dir = crate_toml.parent
    deps: set[str] = set()

    def resolve(dep_name: str, dep_spec: object) -> str | None:
        if isinstance(dep_spec, str):
            return dep_name if dep_name in workspace_packages else None
        if not isinstance(dep_spec, dict):
            return None
        if dep_spec.get("workspace") is True:
            ws_dep = workspace_deps.get(dep_name, {})
            if isinstance(ws_dep, dict):
                package_name = ws_dep.get("package", dep_name)
                if "path" in ws_dep or package_name in workspace_packages:
                    return package_name
            return dep_name if dep_name in workspace_packages else None
        package_name = dep_spec.get("package", dep_name)
        if "path" in dep_spec:
            dep_path = (crate_dir / dep_spec["path"]).resolve()
            if dep_path.is_relative_to(workspace_root.resolve()):
                return package_name
        return package_name if package_name in workspace_packages else None

    def collect(table: object) -> None:
        if not isinstance(table, dict):
            return
        for dep_name, dep_spec in table.items():
            package_name = resolve(dep_name, dep_spec)
            if package_name:
                deps.add(package_name)

    collect(data.get("dependencies", {}))
    collect(data.get("build-dependencies", {}))
    for target_data in data.get("target", {}).values():
        if isinstance(target_data, dict):
            collect(target_data.get("dependencies", {}))
            collect(target_data.get("build-dependencies", {}))
    return deps


def validate_publish_order(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    workspace_root = Path(args.workspace_toml).parent
    publishable = [crate for crate in manifest["crates"] if crate["publish"]]
    order = {crate["package"]: crate["publish_order"] for crate in publishable}
    violations = []
    for crate in publishable:
        crate_toml = workspace_root / crate["cargo_toml"]
        for dep_package in sorted(workspace_dependency_names(crate_toml, workspace_root)):
            if dep_package in order and order[crate["package"]] <= order[dep_package]:
                violations.append(
                    f"{crate['package']} (publish_order={order[crate['package']]}) depends on "
                    f"{dep_package} (publish_order={order[dep_package]})"
                )
    if violations:
        print("publish_order violation(s):")
        for violation in violations:
            print(f"  - {violation}")
        return 1
    print("ok: publish_order matches the workspace dependency graph")
    return 0


def homebrew_archive_name(version: str, triple: str) -> str:
    return f"atm_{version}_{triple}.tar.gz"


def github_release_asset_url(tag: str, archive_name: str) -> str:
    return f"https://github.com/randlee/atm-core/releases/download/{tag}/{archive_name}"


def load_release_checksums(release_dir: Path) -> dict[str, str]:
    checksums_path = release_dir / "checksums.txt"
    if not checksums_path.exists():
        raise SystemExit(f"missing checksums file: {checksums_path}")
    checksums: dict[str, str] = {}
    for line_no, raw_line in enumerate(checksums_path.read_text(encoding="utf-8").splitlines(), start=1):
        line = raw_line.strip()
        if not line:
            continue
        parts = line.split(maxsplit=1)
        if len(parts) != 2:
            raise SystemExit(f"invalid checksums.txt line {line_no}: {raw_line!r}")
        sha256, filename = parts
        checksums[Path(filename).name] = sha256
    return checksums


def expected_homebrew_assets(version: str, tag: str, checksums: dict[str, str]) -> dict[tuple[str, str], tuple[str, str]]:
    assets: dict[tuple[str, str], tuple[str, str]] = {}
    for platform_key, triple in HOMEBREW_PLATFORM_TRIPLES.items():
        archive_name = homebrew_archive_name(version, triple)
        sha256 = checksums.get(archive_name)
        if sha256 is None:
            raise SystemExit(f"missing checksum for release archive {archive_name}")
        assets[platform_key] = (github_release_asset_url(tag, archive_name), sha256)
    return assets


def homebrew_context_key(block_stack: list[str]) -> tuple[str, str] | None:
    top_level = next((entry for entry in reversed(block_stack) if entry in {"on_macos", "on_linux"}), None)
    arch = next((entry for entry in reversed(block_stack) if entry in {"on_arm", "on_intel"}), None)
    if top_level is None or arch is None:
        return None
    candidate = (top_level, arch)
    if candidate in HOMEBREW_PLATFORM_TRIPLES:
        return candidate
    return None


def formula_block_push(stripped: str) -> str | None:
    if stripped.endswith(" do"):
        return stripped[:-3]
    if stripped.startswith("if "):
        return stripped
    if stripped.startswith("def "):
        return stripped
    return None


def rewrite_homebrew_formula(text: str, *, version: str, tag: str, checksums: dict[str, str]) -> str:
    assets = expected_homebrew_assets(version, tag, checksums)
    output: list[str] = []
    block_stack: list[str] = []
    for raw_line in text.splitlines(keepends=True):
        stripped = raw_line.strip()
        if stripped == "end":
            if block_stack:
                block_stack.pop()
            output.append(raw_line)
            continue

        push_value = formula_block_push(stripped)
        context_key = homebrew_context_key(block_stack)
        indent = raw_line[: len(raw_line) - len(raw_line.lstrip())]

        if stripped.startswith('version "'):
            raw_line = f'{indent}version "{version}"\n'
        elif context_key is not None and stripped.startswith('url "'):
            expected_url, _ = assets[context_key]
            raw_line = f'{indent}url "{expected_url}"\n'
        elif context_key is not None and stripped.startswith('sha256 "'):
            _, expected_sha = assets[context_key]
            raw_line = f'{indent}sha256 "{expected_sha}"\n'

        output.append(raw_line)
        if push_value is not None:
            block_stack.append(push_value)
    return "".join(output)


def validate_homebrew_formula_content(
    text: str,
    *,
    version: str,
    tag: str,
    checksums: dict[str, str],
    formula_label: str,
) -> list[str]:
    assets = expected_homebrew_assets(version, tag, checksums)
    errors: list[str] = []
    seen: dict[tuple[str, str], dict[str, int]] = {
        key: {"url": 0, "sha256": 0} for key in HOMEBREW_PLATFORM_TRIPLES
    }
    version_match = re.search(r'^\s*version "([^"]+)"', text, re.MULTILINE)
    if version_match is None:
        errors.append(f"{formula_label}: missing version declaration")
    elif version_match.group(1) != version:
        errors.append(
            f"{formula_label}: version mismatch: expected {version}, found {version_match.group(1)}"
        )

    block_stack: list[str] = []
    for line_no, raw_line in enumerate(text.splitlines(), start=1):
        stripped = raw_line.strip()
        if stripped == "end":
            if block_stack:
                block_stack.pop()
            continue

        context_key = homebrew_context_key(block_stack)
        if context_key is not None:
            expected_url, expected_sha = assets[context_key]
            if stripped.startswith('url "'):
                seen[context_key]["url"] += 1
                actual_url = stripped[len('url "') : -1]
                if actual_url != expected_url:
                    errors.append(
                        f"{formula_label}:{line_no}: {context_key[0]}/{context_key[1]} url mismatch: "
                        f"expected {expected_url}, found {actual_url}"
                    )
            elif stripped.startswith('sha256 "'):
                seen[context_key]["sha256"] += 1
                actual_sha = stripped[len('sha256 "') : -1]
                if actual_sha != expected_sha:
                    errors.append(
                        f"{formula_label}:{line_no}: {context_key[0]}/{context_key[1]} sha256 mismatch: "
                        f"expected {expected_sha}, found {actual_sha}"
                    )

        push_value = formula_block_push(stripped)
        if push_value is not None:
            block_stack.append(push_value)

    for context_key, counters in seen.items():
        for field, count in counters.items():
            if count != 1:
                errors.append(
                    f"{formula_label}: expected exactly one {field} in {context_key[0]}/{context_key[1]}, found {count}"
                )
    return errors


def update_homebrew_formulas(args: argparse.Namespace) -> int:
    checksums = load_release_checksums(Path(args.release_dir))
    updated = 0
    for formula_path_str in args.formula:
        formula_path = Path(formula_path_str)
        text = formula_path.read_text(encoding="utf-8")
        formula_path.write_text(
            rewrite_homebrew_formula(text, version=args.version, tag=args.tag, checksums=checksums),
            encoding="utf-8",
        )
        updated += 1
    print(f"ok: updated {updated} Homebrew formula(s)")
    return 0


def validate_homebrew_formulas(args: argparse.Namespace) -> int:
    checksums = load_release_checksums(Path(args.release_dir))
    errors: list[str] = []
    for formula_path_str in args.formula:
        formula_path = Path(formula_path_str)
        errors.extend(
            validate_homebrew_formula_content(
                formula_path.read_text(encoding="utf-8"),
                version=args.version,
                tag=args.tag,
                checksums=checksums,
                formula_label=str(formula_path),
            )
        )
    if errors:
        print("homebrew formula validation failed:")
        for error in errors:
            print(f"  - {error}")
        return 1
    print("ok: Homebrew formulas match expected platform assets and checksums")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Release artifact manifest utilities")
    subparsers = parser.add_subparsers(dest="command", required=True)

    emit = subparsers.add_parser("emit-inventory")
    emit.add_argument("--manifest", required=True)
    emit.add_argument("--version", required=True)
    emit.add_argument("--tag", required=True)
    emit.add_argument("--commit", required=True)
    emit.add_argument("--source-ref", required=True)
    emit.add_argument("--generated-at")
    emit.add_argument("--output", required=True)
    emit.set_defaults(func=emit_inventory)

    list_tomls = subparsers.add_parser("list-cargo-tomls")
    list_tomls.add_argument("--manifest", required=True)
    list_tomls.set_defaults(func=list_cargo_tomls)

    list_items = subparsers.add_parser("list-artifacts")
    list_items.add_argument("--manifest", required=True)
    list_items.add_argument("--publishable-only", action="store_true")
    list_items.set_defaults(func=list_artifacts)

    list_pre = subparsers.add_parser("list-preflight")
    list_pre.add_argument("--manifest", required=True)
    list_pre.add_argument("--mode", required=True, choices=[PREFLIGHT_FULL, PREFLIGHT_LOCKED])
    list_pre.set_defaults(func=list_preflight)

    list_plan = subparsers.add_parser("list-publish-plan")
    list_plan.add_argument("--manifest", required=True)
    list_plan.set_defaults(func=list_publish_plan)

    channel = subparsers.add_parser("channel-config")
    channel.add_argument("--manifest", required=True)
    channel.add_argument("--channel", required=True)
    channel.set_defaults(func=channel_config)

    release_targets = subparsers.add_parser("release-target-matrix")
    release_targets.add_argument("--manifest", required=True)
    release_targets.set_defaults(func=release_target_matrix)

    package_config = subparsers.add_parser("release-package-config")
    package_config.add_argument("--manifest", required=True)
    package_config.add_argument("--target", required=True)
    package_config.set_defaults(func=release_package_config)

    python_wheels = subparsers.add_parser("python-wheel-matrix")
    python_wheels.add_argument("--manifest", required=True)
    python_wheels.set_defaults(func=python_wheel_matrix)

    python_sdists = subparsers.add_parser("python-sdist-matrix")
    python_sdists.add_argument("--manifest", required=True)
    python_sdists.set_defaults(func=python_sdist_matrix)

    verify_python_assets = subparsers.add_parser("verify-python-release-assets")
    verify_python_assets.add_argument("--manifest", required=True)
    verify_python_assets.add_argument("--asset-dir", required=True)
    verify_python_assets.add_argument("--copy-to")
    verify_python_assets.set_defaults(func=verify_python_release_assets)

    version_check = subparsers.add_parser("verify-version")
    version_check.add_argument("--manifest", required=True)
    version_check.add_argument("--workspace-toml", required=True)
    version_check.add_argument("--version", required=True)
    version_check.set_defaults(func=verify_version)

    version_lockstep = subparsers.add_parser("verify-version-lockstep")
    version_lockstep.add_argument("--manifest", required=True)
    version_lockstep.add_argument("--workspace-toml", required=True)
    version_lockstep.set_defaults(func=verify_version_lockstep)

    registry_plan = subparsers.add_parser("public-registry-check-plan")
    registry_plan.add_argument("--manifest", required=True)
    registry_plan.add_argument("--version", required=True)
    registry_plan.set_defaults(func=public_registry_check_plan)

    registry_inquiry = subparsers.add_parser("public-registry-inquiry-plan")
    registry_inquiry.add_argument("--contracts", required=True)
    registry_inquiry.add_argument("--channel", choices=("crates_io", "pypi"), required=True)
    registry_inquiry.add_argument("--name", required=True)
    registry_inquiry.add_argument("--version")
    registry_inquiry.set_defaults(func=public_registry_inquiry_plan)

    dispatch = subparsers.add_parser("channel-dispatch-plan")
    dispatch.add_argument("--manifest", required=True)
    dispatch.add_argument("--tag", required=True)
    dispatch.set_defaults(func=channel_dispatch_plan)

    secret_plan = subparsers.add_parser("preflight-secret-plan")
    secret_plan.add_argument("--manifest", required=True)
    secret_plan.set_defaults(func=preflight_secret_plan)

    preflight_results = subparsers.add_parser("channel-preflight-results")
    preflight_results.add_argument("--manifest", required=True)
    preflight_results.add_argument("--outcomes", required=True)
    preflight_results.add_argument("--tag", required=True)
    preflight_results.set_defaults(func=channel_preflight_results)

    list_bins = subparsers.add_parser("list-release-binaries")
    list_bins.add_argument("--manifest", required=True)
    list_bins.set_defaults(func=list_release_binaries)

    list_docs = subparsers.add_parser("list-installed-doc-members")
    list_docs.add_argument("--manifest", required=True)
    list_docs.set_defaults(func=list_installed_doc_members)

    stage_docs = subparsers.add_parser("stage-install-docs")
    stage_docs.add_argument("--manifest", required=True)
    stage_docs.add_argument("--output-root", required=True)
    stage_docs.set_defaults(func=stage_install_docs)

    validate_bins = subparsers.add_parser("validate-release-binaries")
    validate_bins.add_argument("--manifest", required=True)
    validate_bins.add_argument("--required", action="append", default=[])
    validate_bins.set_defaults(func=validate_release_binaries)

    build_bins = subparsers.add_parser("cargo-build-bin-args")
    build_bins.add_argument("--manifest", required=True)
    build_bins.set_defaults(func=cargo_build_bin_args)

    unpublished = subparsers.add_parser("check-version-unpublished")
    unpublished.add_argument("--manifest", required=True)
    unpublished.add_argument("--version", required=True)
    unpublished.set_defaults(func=check_version_unpublished)

    validate_m = subparsers.add_parser("validate-manifest")
    validate_m.add_argument("--manifest", required=True)
    validate_m.add_argument("--workspace-toml", default="Cargo.toml")
    validate_m.set_defaults(func=validate_manifest)

    validate_p = subparsers.add_parser("validate-preflight-checks")
    validate_p.add_argument("--manifest", required=True)
    validate_p.add_argument("--workspace-toml", default="Cargo.toml")
    validate_p.set_defaults(func=validate_preflight_checks)

    validate_o = subparsers.add_parser("validate-publish-order")
    validate_o.add_argument("--manifest", required=True)
    validate_o.add_argument("--workspace-toml", default="Cargo.toml")
    validate_o.set_defaults(func=validate_publish_order)

    update_homebrew = subparsers.add_parser("update-homebrew-formulas")
    update_homebrew.add_argument("--release-dir", required=True)
    update_homebrew.add_argument("--version", required=True)
    update_homebrew.add_argument("--tag", required=True)
    update_homebrew.add_argument("--formula", action="append", default=[])
    update_homebrew.set_defaults(func=update_homebrew_formulas)

    validate_homebrew = subparsers.add_parser("validate-homebrew-formulas")
    validate_homebrew.add_argument("--release-dir", required=True)
    validate_homebrew.add_argument("--version", required=True)
    validate_homebrew.add_argument("--tag", required=True)
    validate_homebrew.add_argument("--formula", action="append", default=[])
    validate_homebrew.set_defaults(func=validate_homebrew_formulas)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
