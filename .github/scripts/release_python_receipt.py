#!/usr/bin/env python3
"""Emit a source-bound SHA-256 receipt for manifest-selected Python assets."""

from __future__ import annotations

import argparse
import hashlib
import json
import tarfile
import zipfile
from email import message_from_bytes
from pathlib import Path

from release_manifest import load_manifest


def distribution_metadata(path: Path) -> tuple[str, str]:
    """Read the normalized distribution name and version from one artifact."""
    if path.suffix == ".whl":
        with zipfile.ZipFile(path) as archive:
            members = [name for name in archive.namelist() if name.endswith(".dist-info/METADATA")]
            if len(members) != 1:
                raise SystemExit(f"{path}: expected exactly one wheel METADATA file")
            metadata = message_from_bytes(archive.read(members[0]))
    elif path.name.endswith(".tar.gz"):
        with tarfile.open(path, "r:gz") as archive:
            members = [member for member in archive.getmembers() if member.name.endswith("/PKG-INFO")]
            if len(members) != 1:
                raise SystemExit(f"{path}: expected exactly one sdist PKG-INFO file")
            extracted = archive.extractfile(members[0])
            if extracted is None:
                raise SystemExit(f"{path}: unable to read sdist PKG-INFO")
            metadata = message_from_bytes(extracted.read())
    else:
        raise SystemExit(f"{path}: unsupported Python artifact")
    name, version = metadata.get("Name"), metadata.get("Version")
    if not name or not version:
        raise SystemExit(f"{path}: package metadata must provide Name and Version")
    return name, version


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def create_receipt(manifest_path: Path, asset_dir: Path, version: str, source_commit: str) -> dict[str, object]:
    manifest = load_manifest(manifest_path)
    expected = {
        entry["name"]: {"wheel": len(entry["wheels"]), "sdist": int(entry["sdist"])}
        for entry in manifest["python_distributions"]
    }
    if not expected:
        raise SystemExit("manifest must define [[python_distributions]]")
    found = {name: {"wheel": 0, "sdist": 0} for name in expected}
    artifacts: list[dict[str, str]] = []
    for path in sorted(asset_dir.iterdir()):
        if not path.is_file() or (path.suffix != ".whl" and not path.name.endswith(".tar.gz")):
            continue
        name, actual_version = distribution_metadata(path)
        if name not in expected:
            raise SystemExit(f"{path}: unexpected Python distribution {name!r}")
        if actual_version != version:
            raise SystemExit(f"{path}: expected version {version}, found {actual_version}")
        found[name]["wheel" if path.suffix == ".whl" else "sdist"] += 1
        artifacts.append({"filename": path.name, "package": name, "sha256": sha256(path)})
    if not artifacts:
        raise SystemExit(f"no Python artifacts found in {asset_dir}")
    if found != expected:
        raise SystemExit(f"Python artifact set mismatch: expected {expected}, found {found}")
    return {
        "source_commit": source_commit,
        "manifest_sha256": sha256(manifest_path),
        "version": version,
        "artifacts": artifacts,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--asset-dir", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    receipt = create_receipt(args.manifest, args.asset_dir, args.version, args.source_commit)
    args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
