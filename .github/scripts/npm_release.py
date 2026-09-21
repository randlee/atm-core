#!/usr/bin/env python3
"""Build npm archives once; verify and publish immutable release bytes by tag."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.error
import urllib.parse
import urllib.request

from release_manifest import load_manifest
from worker_result import redact_diagnostic

REGISTRY = "https://registry.npmjs.org"
def _safe_diagnostic(value: bytes | str | None) -> str:
    text = value.decode("utf-8", "replace") if isinstance(value, bytes) else (value or "")
    return redact_diagnostic(text) or "<no diagnostic output>"


def packages(manifest):
    entries = manifest.get("npm_packages", [])
    names = set()
    assets = set()
    scopes = set()
    for entry in entries:
        name, source = entry["name"], Path(entry["source"])
        if not re.fullmatch(r"(?:@[a-z0-9][a-z0-9._-]*/)?[a-z0-9][a-z0-9._-]*", name):
            raise ValueError("invalid npm package name")
        asset = filename(name, "")
        if name in names or asset in assets or source.is_absolute() or ".." in source.parts:
            raise ValueError("duplicate npm package or unsafe source path")
        names.add(name)
        assets.add(asset)
        if name.startswith("@"):
            scopes.add(name.split("/", 1)[0])
    if len(scopes) > 1:
        raise ValueError("npm packages must use one consistent scope")
    if bool(entries) != ("npm" in manifest.get("channels", {})):
        raise ValueError("npm_packages and channels.npm must be declared together")
    return entries


def version(tag):
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", tag):
        raise ValueError("expected vX.Y.Z with optional prerelease")
    return tag[1:]


def filename(name, release_version):
    return name.replace("@", "").replace("/", "-") + "-" + release_version + ".tgz"


def metadata(path):
    with tarfile.open(path, "r:gz") as archive:
        contents = archive.getmembers()
        for member in contents:
            parts = Path(member.name).parts
            if not parts or parts[0] != "package" or ".." in parts or not (member.isfile() or member.isdir()):
                raise ValueError("unsafe npm archive member")
        members = [m for m in contents if m.name == "package/package.json"]
        if len(members) != 1 or not members[0].isfile() or members[0].size > 1024 * 1024:
            raise ValueError("archive must contain exactly one regular package/package.json")
        return json.load(archive.extractfile(members[0]))


def check_package_metadata(data, name, release_version, label):
    if not isinstance(data, dict) or data.get("name") != name or data.get("version") != release_version or data.get("private", False) is not False:
        raise ValueError(f"{label}: identity/version/private mismatch")
    config = data.get("publishConfig", {})
    if not isinstance(config, dict) or config.get("access", "public") != "public" or "tag" in config:
        raise ValueError(f"{label}: publishConfig conflicts with shared public/tag policy")
    registry = config.get("registry", REGISTRY)
    if not isinstance(registry, str) or registry.rstrip("/") != REGISTRY:
        raise ValueError(f"{label}: redirects publication to another registry")


def check_metadata(path, name, release_version):
    check_package_metadata(metadata(path), name, release_version, "npm archive")


def validate_sources(manifest, release_version, source_ref=None):
    """Fail before release writes when any declared npm source is unsuitable."""
    for entry in packages(manifest):
        package_path = Path(entry["source"]) / "package.json"
        if source_ref:
            data = json.loads(subprocess.check_output(["git", "show", f"{source_ref}:{package_path.as_posix()}"], text=True))
            lock_path = package_path.parent / "package-lock.json"
        else:
            source = package_path.resolve()
            if not source.is_relative_to(Path.cwd().resolve()):
                raise ValueError("npm source escapes checkout")
            data = json.loads(source.read_text())
            lock_path = package_path.parent / "package-lock.json"
        check_package_metadata(data, entry["name"], release_version, f"source npm package {package_path}")
        if source_ref:
            lock_data = json.loads(subprocess.check_output(["git", "show", f"{source_ref}:{lock_path.as_posix()}"], text=True))
        else:
            lock_data = json.loads(lock_path.read_text())
        if lock_data.get("name") != entry["name"] or lock_data.get("version") != release_version:
            raise ValueError(f"source npm lockfile {lock_path}: top-level identity/version mismatch")
        root = lock_data.get("packages", {}).get("", {})
        if root.get("name") != entry["name"] or root.get("version") != release_version:
            raise ValueError(f"source npm lockfile {lock_path}: packages root identity/version mismatch")


def check_release_source(manifest_path, tag, source_ref=None):
    if source_ref:
        if not re.fullmatch(r"[0-9a-f]{40}", source_ref):
            raise ValueError("npm source ref must be an exact commit SHA")
        path = Path(manifest_path)
        if path.is_absolute() or ".." in path.parts:
            raise ValueError("npm source manifest must be repository-relative")
        manifest = tomllib.loads(subprocess.check_output(["git", "show", f"{source_ref}:{path.as_posix()}"], text=True))
    else:
        manifest = load_manifest(Path(manifest_path))
    if packages(manifest):
        validate_sources(manifest, version(tag), source_ref)


def build(manifest, tag, asset_dir):
    entries = packages(manifest)
    if not entries:
        return
    release_version = version(tag)
    validate_sources(manifest, release_version)
    asset_dir.mkdir(parents=True, exist_ok=True)
    for entry in entries:
        source = Path(entry["source"]).resolve()
        subprocess.run(["npm", "ci"], cwd=source, check=True)
        subprocess.run(["npm", "run", "build", "--if-present"], cwd=source, check=True)
        subprocess.run(["npm", "pack", "--ignore-scripts", "--pack-destination", str(asset_dir.resolve())], cwd=source, check=True)
        check_metadata(asset_dir / filename(entry["name"], release_version), entry["name"], release_version)


def verify(manifest, tag, asset_dir):
    release_version = version(tag)
    checksums = {}
    for line in (asset_dir / "checksums.txt").read_text().splitlines():
        digest, name = line.split(maxsplit=1)
        name = name.lstrip("*")
        if name in checksums or not re.fullmatch("[0-9a-f]{64}", digest):
            raise ValueError("invalid or duplicate release checksum")
        checksums[name] = digest
    verified = []
    for entry in packages(manifest):
        path = asset_dir / filename(entry["name"], release_version)
        if path.is_symlink() or not path.is_file():
            raise ValueError("missing regular npm asset")
        if checksums.get(path.name) != hashlib.sha256(path.read_bytes()).hexdigest():
            raise ValueError("npm asset checksum mismatch")
        check_metadata(path, entry["name"], release_version)
        verified.append((entry["name"], path))
    return verified


def registry_version(name, release_version):
    url = f"{REGISTRY}/{urllib.parse.quote(name, safe='')}/{urllib.parse.quote(release_version, safe='')}"
    try:
        with urllib.request.urlopen(url, timeout=30) as response:
            data = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise RuntimeError("npm registry lookup indeterminate") from None
    except (OSError, ValueError):
        raise RuntimeError("npm registry lookup indeterminate") from None
    if data.get("name") != name or data.get("version") != release_version:
        raise ValueError("npm registry identity mismatch")
    return data


def identical(data, path):
    expected = "sha512-" + base64.b64encode(hashlib.sha512(path.read_bytes()).digest()).decode()
    if data.get("dist", {}).get("integrity") != expected:
        raise ValueError("published npm version differs from release bytes")


def publish(manifest, tag, asset_dir, dry_run=True):
    verified = verify(manifest, tag, asset_dir)  # validate every archive before any write
    release_version = version(tag)
    for name, path in verified:
        existing = registry_version(name, release_version)
        if existing is not None:
            identical(existing, path)
            print(f"already published: {name}@{release_version}")
            continue
        if dry_run:
            print(f"ready: {name}@{release_version}")
            continue
        # No lifecycle scripts, project npmrc, or rebuild in the credentialed leg.
        with tempfile.TemporaryDirectory() as temporary:
            command = ["npm", "publish", str(path.resolve()), "--ignore-scripts", "--access", "public", "--registry", REGISTRY, "--tag", "next" if "-" in release_version else "latest"]
            result = subprocess.run(command, cwd=temporary, capture_output=True, text=True)
        if result.returncode:
            # A race or a response lost after acceptance is recoverable only
            # when the registry confirms the exact bytes. Never print npm output.
            existing = registry_version(name, release_version)
            if existing is None:
                diagnostic = {
                    "channel": "npm", "status": "failed", "tag": tag,
                    "commit": "unavailable", "command": command,
                    "exit_status": result.returncode,
                    "error": {"code": "NPM.PUBLISH_FAILED", "message": _safe_diagnostic(
                        "stdout: " + (result.stdout or "") + "\nstderr: " + (result.stderr or "")
                    )},
                    "attempts": 1, "workflow_url": "unavailable", "job_url": "unavailable",
                    "evidence": "npm subprocess output",
                    "registry_outcome": "version absent after failed publication",
                    "verification": ["registry version lookup returned absent"],
                    "sanitized_diagnostic": _safe_diagnostic(
                        "stdout: " + (result.stdout or "") + "\nstderr: " + (result.stderr or "")
                    ),
                }
                raise RuntimeError("npm publication failed; retry this channel by tag: " + json.dumps(diagnostic, sort_keys=True))
            identical(existing, path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("build", "verify", "preflight", "publish", "check-source"))
    parser.add_argument("--manifest", default="release/publish-artifacts.toml")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--source-ref", help="Exact commit to check before creating a release tag")
    parser.add_argument("--asset-dir", type=Path, default=Path("npm-dist"))
    args = parser.parse_args()
    if args.command == "check-source":
        check_release_source(args.manifest, args.tag, args.source_ref)
        return
    manifest = load_manifest(Path(args.manifest))
    if args.command == "build":
        build(manifest, args.tag, args.asset_dir)
    elif args.command == "verify":
        verify(manifest, args.tag, args.asset_dir)
    else:
        if not packages(manifest):
            raise ValueError("npm channel is not enabled")
        publish(manifest, args.tag, args.asset_dir, dry_run=args.command == "preflight")


if __name__ == "__main__":
    main()
