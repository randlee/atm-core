"""Python-distribution and version commands for the release manifest CLI."""

from __future__ import annotations

import argparse
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
    _python_distribution_entries,
    _python_distribution_expectations,
    _python_project_version,
    load_manifest,
    workspace_version,
)


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
        data = tomllib.loads(Path(crate["cargo_toml"]).read_text(encoding="utf-8"))
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
        _assert_workspace_inherited_version(workspace_toml, cargo_toml)
        checked_cargo_manifests.add(cargo_toml)
    for distribution in _python_distribution_entries(manifest):
        if distribution["build_system"] != "maturin":
            continue
        cargo_toml = distribution["cargo_manifest"]
        if cargo_toml not in checked_cargo_manifests:
            _assert_workspace_inherited_version(workspace_toml, cargo_toml)
            checked_cargo_manifests.add(cargo_toml)
    for package in manifest["python_packages"]:
        _assert_python_package_version(workspace_toml, package["manifest"], version)
    print("version lockstep verification passed")
    return 0


def cmd_verify_python_version(args: argparse.Namespace) -> int:
    version = workspace_version(Path(args.workspace_toml))
    if version != args.version:
        raise SystemExit(f"workspace version mismatch: expected {args.version}, got {version}")
    actual = _python_project_version(Path(args.pyproject))
    if actual is None:
        print("python package declares a dynamic version")
        return 0
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


def _readme_version_checks(version: str, dependency_crate: str) -> tuple[tuple[str, str, str], ...]:
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
