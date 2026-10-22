#!/usr/bin/env python3
"""Publish or install manifest-declared GitHub prerelease binary assets."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
from urllib.request import urlopen
import zipfile
from typing import Any, Sequence


STABLE_VERSION_PARTS = 3
ARCHIVE_WORKFLOW = "prerelease-archive.yml"
ARCHIVE_WAIT_SECONDS = 60 * 60
ARCHIVE_POLL_SECONDS = 60


def command(args: Sequence[str], *, capture: bool = False) -> subprocess.CompletedProcess[str]:
    """Run one external command with the current repository as its working directory."""
    return subprocess.run(args, check=True, text=True, capture_output=capture)


def shell(command_text: str, *, capture: bool = False) -> subprocess.CompletedProcess[str]:
    """Run a manifest command after the manifest was validated by the installer."""
    return subprocess.run(command_text, shell=True, check=True, text=True, capture_output=capture)


def parse_version(value: str) -> str:
    if len(value.split(".")) != STABLE_VERSION_PARTS or not all(part.isdigit() for part in value.split(".")):
        raise SystemExit("version must be X.Y.Z")
    return value


def read_manifest(path: Path) -> dict[str, Any]:
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise SystemExit(f"cannot read manifest {path}: {error}") from error
    if not isinstance(data, dict):
        raise SystemExit("publish-artifacts.toml must be a TOML table")
    return data


def prerelease_manifest(path: Path) -> dict[str, Any]:
    """Read the opt-in prerelease table from a publish artifacts manifest."""
    manifest = read_manifest(path)
    reader = Path(".github/scripts/release_artifacts.py")
    if reader.is_file():
        manifest = manifest_with_release_artifacts(manifest, path, reader)
    value = manifest.get("prerelease")
    if not isinstance(value, dict):
        raise SystemExit("[prerelease] is not enabled in release/publish-artifacts.toml")
    value["release_targets"] = manifest.get("release_targets", [])
    value["python_distributions"] = manifest.get("python_distributions", [])
    return value


def manifest_with_release_artifacts(
    manifest: dict[str, Any], manifest_path: Path, reader: Path
) -> dict[str, Any]:
    """Get release target and Python distribution matrices from the shared reader."""
    def read_matrix(command_name: str) -> dict[str, Any]:
        result = command(
            [sys.executable, str(reader), command_name, "--manifest", str(manifest_path)],
            capture=True,
        )
        try:
            value = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise SystemExit(f"release_artifacts.py returned invalid {command_name} JSON: {error}") from error
        if not isinstance(value, dict) or not isinstance(value.get("include"), list):
            raise SystemExit(f"release_artifacts.py returned invalid {command_name} matrix")
        return value

    targets = read_matrix("release-target-matrix")
    wheels = read_matrix("python-wheel-matrix")
    sdists = read_matrix("python-sdist-matrix")
    wheel_entries: dict[str, list[str]] = {}
    for wheel in wheels["include"]:
        if not isinstance(wheel, dict) or not isinstance(wheel.get("name"), str):
            raise SystemExit("release_artifacts.py returned an invalid wheel entry")
        runner = wheel.get("id") or wheel.get("os")
        if not isinstance(runner, str):
            raise SystemExit("release_artifacts.py returned a wheel without a runner")
        wheel_entries.setdefault(wheel["name"], []).append(runner)
    sdist_names = {
        entry["name"] for entry in sdists["include"]
        if isinstance(entry, dict) and isinstance(entry.get("name"), str)
    }
    manifest["release_targets"] = targets["include"]
    manifest["python_distributions"] = [
        {"name": name, "wheels": runners, "sdist": name in sdist_names}
        for name, runners in wheel_entries.items()
    ]
    return manifest


def required_string(config: dict[str, Any], name: str) -> str:
    value = config.get(name)
    if not isinstance(value, str) or not value:
        raise SystemExit(f"[prerelease].{name} must be a non-empty string")
    return value


def required_string_list(config: dict[str, Any], name: str) -> list[str]:
    value = config.get(name)
    if (
        not isinstance(value, list)
        or not value
        or not all(isinstance(item, str) and item for item in value)
    ):
        raise SystemExit(f"[prerelease].{name} must be a non-empty string array")
    return value


def release_targets(manifest: dict[str, Any]) -> list[dict[str, str]]:
    values = manifest.get("release_targets")
    if not isinstance(values, list):
        raise SystemExit("release_targets must be an array")
    targets: list[dict[str, str]] = []
    for value in values:
        if not isinstance(value, dict):
            raise SystemExit("release_targets entries must be tables")
        target = value.get("target")
        archive = value.get("archive")
        if not isinstance(target, str) or not isinstance(archive, str):
            raise SystemExit("release_targets entries require target and archive")
        targets.append({"target": target, "archive": archive})
    return targets


def archive_name(manifest: dict[str, Any], version: str, target: dict[str, str]) -> str:
    project = manifest.get("project")
    if not isinstance(project, dict) or not isinstance(project.get("archive_prefix"), str):
        raise SystemExit("[project].archive_prefix is required")
    return f"{project['archive_prefix']}_{version}_{target['target']}.{target['archive']}"


def expected_assets(manifest: dict[str, Any], version: str) -> set[str]:
    assets = {"checksums.txt", *(archive_name(manifest, version, target) for target in release_targets(manifest))}
    distributions = manifest.get("python_distributions", [])
    if not isinstance(distributions, list):
        raise SystemExit("python_distributions must be an array")
    # Wheel filenames include ABI/platform tags, so their exact names are
    # discovered from the prerelease asset list.  The manifest still requires
    # at least one wheel for every declared distribution.
    return assets


def required_wheel_prefixes(manifest: dict[str, Any]) -> set[str]:
    distributions = manifest.get("python_distributions", [])
    if not isinstance(distributions, list):
        raise SystemExit("python_distributions must be an array")
    prefixes: set[str] = set()
    for distribution in distributions:
        if not isinstance(distribution, dict) or not isinstance(distribution.get("name"), str):
            raise SystemExit("python_distributions entries require name")
        prefixes.add(distribution["name"].replace("-", "_") + "-")
    return prefixes


def host_target(manifest: dict[str, Any]) -> dict[str, str]:
    system, machine = platform.system(), platform.machine().lower()
    arch = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}.get(machine)
    suffix = {"Darwin": "apple-darwin", "Linux": "unknown-linux-gnu", "Windows": "pc-windows-msvc"}.get(system)
    if arch is None or suffix is None:
        raise SystemExit(f"unsupported prerelease host: {system} {machine}")
    triple = f"{arch}-{suffix}"
    for target in release_targets(manifest):
        if target["target"] == triple:
            return target
    raise SystemExit(f"manifest does not declare a prerelease archive for {triple}")


def checksums(text: str) -> dict[str, str]:
    """Parse the sha256sum format emitted by prerelease-archive.yml."""
    values: dict[str, str] = {}
    for line in text.splitlines():
        digest, separator, filename = line.partition("  ")
        if not separator:
            continue
        filename = filename.removeprefix("*")
        if len(digest) == 64 and all(character in "0123456789abcdef" for character in digest.lower()):
            values[filename] = digest.lower()
    return values


def verify_checksum(path: Path, expected: str) -> None:
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != expected:
        raise SystemExit(f"sha256 mismatch for {path.name}")


def host_wheel_names(
    digests: dict[str, str], manifest: dict[str, Any], *, system: str | None = None,
    machine: str | None = None,
) -> list[str]:
    """Select one universal or host-platform wheel per declared distribution."""
    system = platform.system() if system is None else system
    machine = (platform.machine() if machine is None else machine).lower().replace("amd64", "x86_64")
    if system == "Windows" and machine == "aarch64":
        machine = "arm64"
    hints = {"Linux": ("manylinux", machine), "Darwin": ("macosx", machine), "Windows": ("win", "arm64" if machine == "arm64" else "amd64")}
    if system not in hints:
        raise SystemExit(f"unsupported prerelease wheel host: {system} {machine}")
    os_hint, arch_hint = hints[system]
    selected: list[str] = []
    for prefix in sorted(required_wheel_prefixes(manifest)):
        candidates = sorted(name for name in digests if name.startswith(prefix) and name.endswith(".whl"))
        universal = [name for name in candidates if name.endswith("-py3-none-any.whl")]
        compatible = [name for name in candidates if os_hint in name and arch_hint in name]
        if len(universal) == 1:
            selected.append(universal[0])
        elif len(compatible) == 1:
            selected.append(compatible[0])
        else:
            raise SystemExit(f"GitHub prerelease has no unique host wheel for {prefix.rstrip('-')}")
    return selected


def gh_json(args: Sequence[str]) -> Any:
    result = command(["gh", *args], capture=True)
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise SystemExit(f"gh returned invalid JSON: {error}") from error


def release_for_tag(tag: str) -> dict[str, Any]:
    release = gh_json(["release", "view", tag, "--json", "url,isPrerelease,isDraft,assets"])
    if not isinstance(release, dict) or release.get("isDraft") or not release.get("isPrerelease"):
        raise SystemExit(f"{tag} is not a published GitHub prerelease")
    return release


def select_release(config: dict[str, Any], requested: str) -> tuple[str, dict[str, Any]]:
    prefix = required_string(config, "tag_prefix")
    if requested != "latest":
        version = parse_version(requested)
        return version, release_for_tag(f"{prefix}{version}")
    releases = gh_json(["release", "list", "--limit", "100", "--json", "tagName,isPrerelease,isDraft"])
    if not isinstance(releases, list):
        raise SystemExit("gh release list returned an invalid response")
    for release in releases:
        if not isinstance(release, dict):
            continue
        tag = release.get("tagName")
        if release.get("isPrerelease") and not release.get("isDraft") and isinstance(tag, str) and tag.startswith(prefix):
            return parse_version(tag.removeprefix(prefix)), release_for_tag(tag)
    raise SystemExit("no published GitHub prerelease matches the manifest tag prefix")


def release_asset_names(release: dict[str, Any]) -> set[str]:
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise SystemExit("GitHub prerelease response has no assets")
    return {asset["name"] for asset in assets if isinstance(asset, dict) and isinstance(asset.get("name"), str)}


def require_release_assets(release: dict[str, Any], manifest: dict[str, Any], version: str) -> None:
    names = release_asset_names(release)
    missing = expected_assets(manifest, version) - names
    if missing:
        raise SystemExit(f"GitHub prerelease is missing required assets: {', '.join(sorted(missing))}")
    distributions = manifest.get("python_distributions", [])
    missing_wheels = []
    for distribution in distributions:
        prefix = distribution["name"].replace("-", "_") + "-"
        expected_count = len(distribution.get("wheels", []))
        actual_count = sum(name.startswith(prefix) and name.endswith(".whl") for name in names)
        if actual_count != expected_count:
            missing_wheels.append(f"{prefix.rstrip('-')} ({actual_count}/{expected_count})")
    if missing_wheels:
        raise SystemExit(f"GitHub prerelease has the wrong manifest-declared wheel set: {', '.join(sorted(missing_wheels))}")
    missing_sdists = []
    for distribution in distributions:
        if not distribution.get("sdist"):
            continue
        prefix = distribution["name"].replace("-", "_") + "-"
        actual_count = sum(name.startswith(prefix) and name.endswith(".tar.gz") for name in names)
        if actual_count != 1:
            missing_sdists.append(f"{prefix.rstrip('-')} ({actual_count}/1)")
    if missing_sdists:
        raise SystemExit(f"GitHub prerelease is missing required source distributions: {', '.join(sorted(missing_sdists))}")


def download_asset(tag: str, name: str, destination: Path, *, public_url: str | None = None) -> Path:
    destination.mkdir(parents=True, exist_ok=True)
    path = destination / name
    if public_url is not None:
        with urlopen(f"{public_url}/{name}", timeout=30) as response:  # noqa: S310 - manifest-declared GitHub release URL
            path.write_bytes(response.read())
    else:
        command(["gh", "release", "download", tag, "--pattern", name, "--dir", str(destination)])
    if not path.is_file():
        raise SystemExit(f"GitHub prerelease did not download {name}")
    return path


def download_checked_archive(tag: str, version: str, manifest: dict[str, Any], destination: Path, *, public_url: str | None = None) -> Path:
    if public_url is None:
        release = release_for_tag(tag)
        require_release_assets(release, manifest, version)
    checksum_file = download_asset(tag, "checksums.txt", destination, public_url=public_url)
    target = host_target(manifest)
    name = archive_name(manifest, version, target)
    expected = checksums(checksum_file.read_text(encoding="utf-8")).get(name)
    if expected is None:
        raise SystemExit(f"checksums.txt has no sha256 for {name}")
    archive = download_asset(tag, name, destination, public_url=public_url)
    verify_checksum(archive, expected)
    return archive


def safe_extract(archive: Path, destination: Path) -> None:
    root = destination.resolve()
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as bundle:
            names = bundle.namelist()
            if any(not (destination / name).resolve().is_relative_to(root) for name in names):
                raise SystemExit("prerelease archive contains an unsafe path")
            bundle.extractall(destination)
        return
    with tarfile.open(archive, mode="r:gz") as bundle:
        members = bundle.getmembers()
        if any(not (destination / member.name).resolve().is_relative_to(root) for member in members):
            raise SystemExit("prerelease archive contains an unsafe path")
        bundle.extractall(destination, members=members, filter="data")


def binary_name(binary: str) -> str:
    return f"{binary}.exe" if platform.system() == "Windows" else binary


def staged_pair(stage: Path, config: dict[str, Any]) -> list[Path]:
    values = config.get("binaries")
    if not isinstance(values, list) or not values or not all(isinstance(value, str) and value for value in values):
        raise SystemExit("[prerelease].binaries must be a non-empty string array")
    paths = [stage / "bin" / binary_name(value) for value in values]
    if not all(path.is_file() for path in paths):
        raise SystemExit(f"staged prerelease pair is incomplete under {stage}")
    return paths


def stage_archive(archive: Path, stage: Path, config: dict[str, Any]) -> list[Path]:
    if stage.is_dir():
        return staged_pair(stage, config)
    staging = stage.with_name(f".{stage.name}.download")
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(mode=0o700, parents=True)
    try:
        safe_extract(archive, staging)
        roots = [path for path in staging.iterdir() if path.is_dir()]
        if len(roots) != 1:
            raise SystemExit("prerelease archive has an unexpected layout")
        staged_pair(roots[0], config)
        stage.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        shutil.rmtree(stage, ignore_errors=True)
        os.replace(roots[0], stage)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    return staged_pair(stage, config)


def stage_wheels(tag: str, manifest: dict[str, Any], stage: Path, destination: Path, *, public_url: str | None) -> None:
    if not required_wheel_prefixes(manifest):
        return
    checksum_file = download_asset(tag, "checksums.txt", destination, public_url=public_url)
    digests = checksums(checksum_file.read_text(encoding="utf-8"))
    wheel_dir = stage / "wheels"
    wheel_dir.mkdir(parents=True, exist_ok=True)
    for name in host_wheel_names(digests, manifest):
        wheel = download_asset(tag, name, destination, public_url=public_url)
        verify_checksum(wheel, digests[name])
        shutil.copy2(wheel, wheel_dir / name)


def selector_directory(config: dict[str, Any]) -> Path:
    selectors = config.get("selector_dir")
    if not isinstance(selectors, dict):
        raise SystemExit("[prerelease].selector_dir must be a table")
    keys = {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}
    try:
        raw = selectors[keys[platform.system()]]
    except KeyError as error:
        raise SystemExit(f"[prerelease].selector_dir lacks {platform.system()} support") from error
    if not isinstance(raw, str) or not raw:
        raise SystemExit("prerelease selector directory must be a non-empty string")
    return Path(os.path.expandvars(raw)).expanduser()


def repoint_selector(paths: Sequence[Path], config: dict[str, Any]) -> None:
    selector = selector_directory(config)
    selector.mkdir(parents=True, exist_ok=True)
    for source in paths:
        target = selector / source.name
        if platform.system() == "Windows":
            shutil.copy2(source, target)
            continue
        if target.is_symlink() or target.exists():
            target.unlink()
        target.symlink_to(source)


def replace_command(template: str, version: str, stage: Path) -> str:
    return template.replace("{version}", version).replace("{stage_dir}", str(stage))


def verify_install(config: dict[str, Any], version: str, stage: Path) -> None:
    shell(replace_command(required_string(config, "post_install"), version, stage))
    verified = shell(required_string(config, "verify"), capture=True)
    if version not in f"{verified.stdout}\n{verified.stderr}":
        raise SystemExit(f"verify command did not report {version}")


def install(manifest: dict[str, Any], requested: str) -> tuple[str, Path]:
    config = manifest["prerelease"]
    assert isinstance(config, dict)
    if requested == "latest":
        version, _release = select_release(config, requested)
        public_url = None
    else:
        version = parse_version(requested)
        project = manifest.get("project")
        homepage = project.get("homepage") if isinstance(project, dict) else None
        if not isinstance(homepage, str) or not homepage.startswith("https://github.com/"):
            raise SystemExit("[project].homepage must be a GitHub repository URL for unauthenticated prerelease install")
        public_url = f"{homepage.rstrip('/')}/releases/download/{required_string(config, 'tag_prefix')}{version}"
    root = Path(os.path.expandvars(required_string(config, "install_root"))).expanduser()
    stage = root / f"v{version}"
    if stage.is_dir():
        paths = staged_pair(stage, config)
    else:
        tag = f"{required_string(config, 'tag_prefix')}{version}"
        with tempfile.TemporaryDirectory(prefix="prerelease-") as directory:
            archive = download_checked_archive(tag, version, manifest, Path(directory), public_url=public_url)
            paths = stage_archive(archive, stage, config)
            stage_wheels(tag, manifest, stage, Path(directory), public_url=public_url)
    repoint_selector(paths, config)
    verify_install(config, version, stage)
    return version, stage


def require_publish_preconditions(config: dict[str, Any]) -> None:
    branch = command(["git", "branch", "--show-current"], capture=True).stdout.strip()
    if not branch or branch in required_string_list(config, "protected_branches"):
        raise SystemExit("prerelease publishing refuses a protected or detached branch")
    command(["git", "diff", "--quiet"])
    command(["gh", "auth", "status"])


def wait_for_archive(tag: str, source_sha: str) -> None:
    started_at = time.monotonic()
    deadline = started_at + ARCHIVE_WAIT_SECONDS
    while time.monotonic() < deadline:
        elapsed_minutes = (time.monotonic() - started_at) / 60
        print(
            f"waiting for {ARCHIVE_WORKFLOW} ({elapsed_minutes:.1f} min elapsed)",
            flush=True,
        )
        runs = gh_json(["run", "list", "--workflow", ARCHIVE_WORKFLOW, "--branch", tag, "--limit", "20", "--json", "status,conclusion,headSha"])
        if isinstance(runs, list):
            run = next(
                (
                    candidate
                    for candidate in runs
                    if isinstance(candidate, dict)
                    and candidate.get("headSha") == source_sha
                ),
                None,
            )
            if run is not None and run.get("status") == "completed":
                if run.get("conclusion") == "success":
                    return
                raise SystemExit(f"{ARCHIVE_WORKFLOW} failed for {tag}")
        remaining = deadline - time.monotonic()
        if remaining > 0:
            time.sleep(min(ARCHIVE_POLL_SECONDS, remaining))
    elapsed_minutes = (time.monotonic() - started_at) / 60
    raise SystemExit(
        f"timed out waiting for {ARCHIVE_WORKFLOW} for {tag} after {elapsed_minutes:.1f} minutes"
    )


def publish(manifest: dict[str, Any]) -> tuple[str, str]:
    config = manifest["prerelease"]
    assert isinstance(config, dict)
    require_publish_preconditions(config)
    result = command([sys.executable, required_string(config, "tag_script")], capture=True)
    tag = next((line.split()[3] for line in result.stdout.splitlines() if line.startswith("created and pushed ")), "")
    prefix = required_string(config, "tag_prefix")
    if not tag.startswith(prefix):
        raise SystemExit("prerelease tag script did not report a prerelease tag")
    version = parse_version(tag.removeprefix(prefix))
    source_sha = command(["git", "rev-list", "-n", "1", tag], capture=True).stdout.strip()
    if not source_sha:
        raise SystemExit(f"cannot resolve the commit for {tag}")
    wait_for_archive(tag, source_sha)
    release = release_for_tag(tag)
    require_release_assets(release, manifest, version)
    with tempfile.TemporaryDirectory(prefix="prerelease-verify-") as directory:
        checksums_file = download_asset(tag, "checksums.txt", Path(directory))
        digest_by_name = checksums(checksums_file.read_text(encoding="utf-8"))
        for asset in sorted(expected_assets(manifest, version) - {"checksums.txt"}):
            expected = digest_by_name.get(asset)
            if expected is None:
                raise SystemExit(f"checksums.txt has no sha256 for {asset}")
            verify_checksum(download_asset(tag, asset, Path(directory)), expected)
    url = release.get("url")
    if not isinstance(url, str):
        raise SystemExit("GitHub prerelease response has no URL")
    return tag, url


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument(
        "--publish",
        "--create",
        dest="publish",
        action="store_true",
        help="publish a new manifest-selected prerelease (--create is a compatibility alias)",
    )
    mode.add_argument(
        "--install",
        nargs="?",
        const="latest",
        metavar="X.Y.Z",
        help="install X.Y.Z, or the latest matching GitHub prerelease when omitted",
    )
    parser.add_argument("--manifest", type=Path, default=Path("release/publish-artifacts.toml"))
    parser.add_argument("--dry-run", action="store_true", help="print the plan without network calls")
    parser.add_argument("--authorized", action="store_true", help="confirm written operator authorization to publish")
    args = parser.parse_args(argv)
    manifest = read_manifest(args.manifest)
    config = prerelease_manifest(args.manifest)
    manifest["release_targets"] = config["release_targets"]
    manifest["python_distributions"] = config["python_distributions"]
    if args.publish:
        if args.dry_run:
            plan = command(
                [sys.executable, required_string(config, "tag_script"), "--dry-run"],
                capture=True,
            )
            print(plan.stdout, end="" if plan.stdout.endswith("\n") else "\n")
            print(f"would wait for {ARCHIVE_WORKFLOW}, then verify Release assets and checksums")
            return 0
        if not args.authorized:
            raise SystemExit("--publish requires written operator authorization")
        _tag, url = publish(manifest)
        print(f"Release URL: {url}")
        return 0
    requested = args.install or "latest"
    if requested != "latest":
        parse_version(requested)
    if args.dry_run:
        print(f"would install GitHub prerelease {requested} without network access")
        return 0
    version, stage = install(manifest, requested)
    print(f"installed prerelease {version} at {stage}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
