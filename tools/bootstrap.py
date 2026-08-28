#!/usr/bin/env python3
"""Install and verify the repository's exact CI/bootstrap tool contract."""

from __future__ import annotations

from dataclasses import dataclass
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from typing import Sequence
import urllib.error
import urllib.request
import zipfile


ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = ROOT / "tools" / "bootstrap.toml"
REQUIREMENTS_PATH = ROOT / "tools" / "bootstrap-requirements.txt"
VENV_PATH = ROOT / ".bootstrap-venv"


class BootstrapError(RuntimeError):
    """The pinned bootstrap contract cannot be satisfied safely."""


@dataclass(frozen=True)
class BootstrapManifest:
    rust: str
    python: str
    just: str
    cargo_tools: tuple[tuple[str, str], ...]
    cargo_allowed_strategies: tuple[tuple[str, tuple[str, ...]], ...]
    sc_compose: str
    sc_compose_checksums: tuple[tuple[str, str], ...]
    wyvern: str
    wyvern_checksums: tuple[tuple[str, str], ...]
    python_packages: tuple[tuple[str, str], ...]


def load_manifest(path: Path = MANIFEST_PATH) -> BootstrapManifest:
    """Load the exact tool versions from the one checked-in contract."""
    raw = tomllib.loads(path.read_text(encoding="utf-8"))
    toolchain = raw["toolchain"]
    cargo = raw["cargo"]
    cargo_allowed = raw["cargo-allowed-strategies"]
    sc_compose = raw["sc-compose"]
    wyvern = raw["wyvern"]
    python = raw["python"]
    cargo_tools = (
        ("cargo-deny", cargo["cargo-deny"]),
        ("cargo-audit", cargo["cargo-audit"]),
        ("cargo-shear", cargo["cargo-shear"]),
        ("cargo-modules", cargo["cargo-modules"]),
    )
    if set(cargo_allowed) != {name for name, _version in cargo_tools}:
        raise BootstrapError("cargo-allowed-strategies must name every cargo tool exactly once.")
    if any(
        not isinstance(strategies, list)
        or any(strategy != "quick-install" for strategy in strategies)
        for strategies in cargo_allowed.values()
    ):
        raise BootstrapError("cargo-allowed-strategies may contain only quick-install.")
    return BootstrapManifest(
        rust=toolchain["rust"],
        python=toolchain["python"],
        just=toolchain["just"],
        cargo_tools=cargo_tools,
        cargo_allowed_strategies=tuple(
            sorted((name, tuple(strategies)) for name, strategies in cargo_allowed.items())
        ),
        sc_compose=sc_compose["version"],
        sc_compose_checksums=tuple(sorted(sc_compose["checksums"].items())),
        wyvern=wyvern["version"],
        wyvern_checksums=tuple(sorted(wyvern["checksums"].items())),
        python_packages=tuple(sorted(python.items())),
    )


def command_output(command: Sequence[str]) -> str:
    """Run one inspection command and return its combined output."""
    result = subprocess.run(command, capture_output=True, check=False, text=True, cwd=ROOT)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise BootstrapError(f"{' '.join(command)} failed: {detail}")
    return f"{result.stdout}\n{result.stderr}".strip()


def require_version(label: str, actual: str, expected: str) -> None:
    """Reject a prefix, range, or unrelated version rather than guessing."""
    if re.search(rf"(?<![0-9.]){re.escape(expected)}(?![0-9.])", actual) is None:
        raise BootstrapError(f"{label} must be exactly {expected}; found {actual or 'no version output'}.")


def homebrew_python_formula(manifest: BootstrapManifest) -> str:
    """Return the Homebrew formula for the manifest's Python major/minor line."""
    major, minor, _patch = manifest.python.split(".")
    return f"python@{major}.{minor}"


def homebrew_seed_commands(manifest: BootstrapManifest, brew: Path) -> tuple[tuple[str, ...], ...]:
    """Return the only mutable local-macOS seed synchronization commands."""
    formula = homebrew_python_formula(manifest)
    return (
        (str(brew), "install", formula, "just"),
        (str(brew), "upgrade", formula, "just"),
    )


def seed_tool_matches(label: str, actual: str, expected: str) -> bool:
    """Return whether a seed tool has the exact manifest version."""
    try:
        require_version(label, actual, expected)
    except BootstrapError:
        return False
    return True


def synchronize_macos_seed_tools(manifest: BootstrapManifest, *, dry_run: bool) -> None:
    """Repair local Homebrew seed drift before enforcing the exact contract.

    GitHub Actions supplies its own pinned seeds and must never mutate a runner's
    package manager. Local macOS development instead uses Homebrew's current
    stable bottle for the manifest-derived major/minor Python formula and just.
    The exact patch still remains an explicit postcondition below.
    """
    if sys.platform != "darwin" or os.environ.get("CI"):
        return

    python_matches = seed_tool_matches("Python", platform.python_version(), manifest.python)
    try:
        just_matches = seed_tool_matches("just", command_output(["just", "--version"]), manifest.just)
    except BootstrapError:
        just_matches = False
    if python_matches and just_matches:
        return

    brew = next((candidate for candidate in (Path("/opt/homebrew/bin/brew"), Path("/usr/local/bin/brew")) if candidate.is_file()), None)
    if brew is None:
        raise BootstrapError(
            "macOS seed tools drifted and Homebrew was not found at /opt/homebrew/bin/brew or /usr/local/bin/brew. "
            "Install Homebrew, then rerun just bootstrap."
        )
    for command in homebrew_seed_commands(manifest, brew):
        run(command, dry_run=dry_run)
    if dry_run:
        return

    formula = homebrew_python_formula(manifest)
    prefix = Path(command_output([str(brew), "--prefix", formula]))
    major, minor, _patch = manifest.python.split(".")
    python = prefix / "bin" / f"python{major}.{minor}"
    require_version("Homebrew Python", command_output([str(python), "--version"]), manifest.python)
    os.execv(str(python), [str(python), str(Path(__file__).resolve()), *sys.argv[1:]])


def verify_seed_tools(manifest: BootstrapManifest) -> None:
    """Verify the minimal tools that must exist before a Just recipe can run."""
    require_version("Python", platform.python_version(), manifest.python)
    require_version("Rust", command_output(["rustc", "--version"]), manifest.rust)
    require_version("just", command_output(["just", "--version"]), manifest.just)


def cargo_install_command(name: str, version: str, *, force: bool) -> list[str]:
    """Return the deterministic cargo-install command for one registry tool."""
    command = ["cargo", "install", "--locked"]
    if force:
        command.append("--force")
    return [*command, "--version", version, name]


SC_COMPOSE_RELEASE_REPOSITORY = "randlee/sc-compose"


def sc_compose_target() -> str:
    """Map the runner to a release target with a published prebuilt asset."""
    system = platform.system().lower()
    machine = platform.machine().lower()
    if system == "darwin":
        if machine in {"arm64", "aarch64"}:
            return "aarch64-apple-darwin"
        if machine in {"x86_64", "amd64"}:
            return "x86_64-apple-darwin"
    elif system == "linux" and machine in {"x86_64", "amd64"}:
        return "x86_64-unknown-linux-gnu"
    elif system == "windows" and machine in {"x86_64", "amd64"}:
        return "x86_64-pc-windows-msvc"
    raise BootstrapError(f"sc-compose has no prebuilt release asset for {platform.system()} {platform.machine()}.")


def sc_compose_asset_name(version: str, target: str) -> str:
    """Return the exact release asset filename for a target triple."""
    suffix = ".zip" if "windows" in target else ".tar.gz"
    return f"sc-compose_{version}_{target}{suffix}"


def sc_compose_release_url(version: str, asset: str) -> str:
    """Return the immutable GitHub release URL for one pinned asset."""
    return f"https://github.com/{SC_COMPOSE_RELEASE_REPOSITORY}/releases/download/v{version}/{asset}"


def sc_compose_install_command(version: str, target: str) -> tuple[str, str]:
    """Describe the pinned prebuilt release install for dry-run/tests."""
    asset = sc_compose_asset_name(version, target)
    return (asset, sc_compose_release_url(version, asset))


def cargo_binstall_command(
    name: str,
    version: str,
    *,
    force: bool,
    allowed_strategies: Sequence[str] = (),
) -> list[str]:
    """Return a non-compiling cargo-binstall command for one registry tool.

    Disable Binstall's compile strategy so an unavailable artifact cannot
    silently rebuild from source.  The quick-install strategy is disabled for
    tools with first-party release assets; cargo-modules explicitly enables it
    because its upstream project publishes tags but no GitHub releases.
    """
    disabled_strategies = "compile" if "quick-install" in allowed_strategies else "quick-install,compile"
    command = [
        "cargo",
        "binstall",
        "--no-confirm",
        "--disable-telemetry",
        "--disable-strategies",
        disabled_strategies,
    ]
    if force:
        command.append("--force")
    return [*command, f"{name}@{version}"]


def venv_python_path() -> Path:
    """Return the repository-local Python executable for the exact bootstrap venv."""
    return VENV_PATH / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")


def ensure_bootstrap_venv(manifest: BootstrapManifest, *, dry_run: bool) -> Path:
    """Create or repair the isolated Python environment without mutating the OS Python."""
    python = venv_python_path()
    if python.is_file():
        try:
            require_version("bootstrap venv Python", command_output([str(python), "--version"]), manifest.python)
            return python
        except BootstrapError:
            pass
    run([sys.executable, "-m", "venv", "--clear", str(VENV_PATH)], dry_run=dry_run)
    if not dry_run:
        require_version("bootstrap venv Python", command_output([str(python), "--version"]), manifest.python)
    return python


def pip_install_command(python: Path) -> list[str]:
    """Install only the exact, checked-in Python dependency closure."""
    return [
        str(python),
        "-m",
        "pip",
        "install",
        "--disable-pip-version-check",
        "--no-deps",
        "--requirement",
        str(REQUIREMENTS_PATH),
    ]


def cargo_bin_path(name: str) -> Path:
    """Locate the binary installed by Cargo without trusting PATH precedence."""
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    suffix = ".exe" if sys.platform == "win32" else ""
    return cargo_home / "bin" / f"{name}{suffix}"


def _release_checksum(manifest: BootstrapManifest, target: str) -> str:
    checksums = dict(manifest.sc_compose_checksums)
    try:
        return checksums[target]
    except KeyError as error:
        raise BootstrapError(f"sc-compose manifest has no checksum for release target {target}.") from error


def _download_release(url: str, *, label: str = "release asset") -> bytes:
    """Download one release asset without invoking a shell or package manager."""
    try:
        with urllib.request.urlopen(url, timeout=120) as response:
            return response.read()
    except (OSError, urllib.error.URLError) as error:
        raise BootstrapError(f"unable to download pinned {label} {url}: {error}") from error


def _safe_member_name(name: str) -> str:
    path = Path(name)
    if path.is_absolute() or ".." in path.parts:
        raise BootstrapError(f"sc-compose release archive contains unsafe path {name!r}.")
    return path.as_posix()


def _extract_sc_compose(archive: bytes, asset: str, destination: Path) -> None:
    """Extract only the expected executable from a verified release archive."""
    executable_name = "sc-compose.exe" if asset.endswith(".zip") else "sc-compose"
    with tempfile.TemporaryDirectory(prefix="atm-sc-compose-") as temp_dir:
        archive_path = Path(temp_dir) / asset
        archive_path.write_bytes(archive)
        if asset.endswith(".zip"):
            with zipfile.ZipFile(archive_path) as package:
                members = { _safe_member_name(name): name for name in package.namelist() }
                member = next((original for safe, original in members.items() if Path(safe).name == executable_name), None)
                if member is None:
                    raise BootstrapError(f"verified sc-compose archive does not contain {executable_name}.")
                binary = package.read(member)
        else:
            with tarfile.open(archive_path, mode="r:gz") as package:
                member = next(
                    (item for item in package.getmembers() if Path(_safe_member_name(item.name)).name == executable_name),
                    None,
                )
                if member is None or not member.isfile():
                    raise BootstrapError(f"verified sc-compose archive does not contain {executable_name}.")
                extracted = package.extractfile(member)
                if extracted is None:
                    raise BootstrapError("verified sc-compose executable could not be read from the archive.")
                binary = extracted.read()
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.name}.tmp-{os.getpid()}")
    temporary.write_bytes(binary)
    if sys.platform != "win32":
        temporary.chmod(0o755)
    os.replace(temporary, destination)


def install_sc_compose_release(manifest: BootstrapManifest, *, dry_run: bool) -> None:
    """Install the pinned GitHub release asset; never compile or fall back."""
    target = sc_compose_target()
    asset, url = sc_compose_install_command(manifest.sc_compose, target)
    expected = _release_checksum(manifest, target)
    destination = cargo_bin_path("sc-compose")
    print(f"+ download {url} -> {destination}")
    if dry_run:
        return
    archive = _download_release(url, label="sc-compose release asset")
    actual = hashlib.sha256(archive).hexdigest()
    if actual != expected:
        raise BootstrapError(
            f"sc-compose release checksum mismatch for {asset}: expected {expected}, found {actual}."
        )
    checksums_url = sc_compose_release_url(manifest.sc_compose, "checksums.txt")
    checksums = _download_release(checksums_url, label="sc-compose checksums").decode("utf-8", errors="strict")
    listed = next((parts[0] for line in checksums.splitlines() if (parts := line.split()) and len(parts) >= 2 and parts[-1] == asset), None)
    if listed != expected:
        raise BootstrapError(f"checksums.txt does not confirm the pinned checksum for {asset}.")
    _extract_sc_compose(archive, asset, destination)


def cargo_receipts() -> dict[str, object]:
    """Load Cargo's installation receipt for exact registry verification."""
    receipt = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / ".crates2.json"
    try:
        raw = json.loads(receipt.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError) as error:
        raise BootstrapError(f"Cargo installation receipt is unavailable: {receipt}") from error
    installs = raw.get("installs")
    if not isinstance(installs, dict):
        raise BootstrapError(f"Cargo installation receipt has no installs map: {receipt}")
    return installs


def binstall_receipts() -> list[dict[str, object]]:
    """Load Binstall's concatenated JSON installation receipt records."""
    path = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / "binstall" / "crates-v1.json"
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return []
    decoder = json.JSONDecoder()
    records: list[dict[str, object]] = []
    offset = 0
    while offset < len(text):
        while offset < len(text) and text[offset].isspace():
            offset += 1
        if offset == len(text):
            break
        try:
            record, offset = decoder.raw_decode(text, offset)
        except json.JSONDecodeError:
            return []
        if isinstance(record, dict):
            records.append(record)
    return records


def _version_text(value: object) -> str:
    """Normalize Binstall's string or structured SemVer receipt value."""
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        base = ".".join(str(value.get(part, 0)) for part in ("major", "minor", "patch"))
        pre = value.get("pre")
        if isinstance(pre, str) and pre:
            base += f"-{pre}"
        return base
    return ""


def binstall_tool_matches(name: str, version: str) -> bool:
    """Prove a prebuilt tool came from Binstall at the exact requested version."""
    for record in binstall_receipts():
        # `crates-v1.json` serializes `CrateInfo` with flattened fields; accept
        # the nested shape too so older locally retained receipts remain usable.
        nested = record.get("crate_info")
        info = nested if isinstance(nested, dict) else record
        if info.get("name") == name and _version_text(info.get("current_version")) == version:
            return True
    return False


def receipt_matches(key_prefix: str, *, required_fragments: Sequence[str]) -> bool:
    """Return whether one Cargo receipt proves this exact installation contract."""
    try:
        receipts = cargo_receipts()
    except BootstrapError:
        return False
    for key, value in receipts.items():
        if not key.startswith(key_prefix) or not isinstance(value, dict):
            continue
        rustc = value.get("rustc")
        if not isinstance(rustc, str):
            continue
        if all(fragment in key or fragment in rustc for fragment in required_fragments):
            return True
    return False


def registry_tool_matches(name: str, version: str, rust: str) -> bool:
    """Prove the registry tool is both the required version and Rust build."""
    return receipt_matches(
        f"{name} {version} (registry+",
        required_fragments=(f"release: {rust}",),
    )


def sc_compose_matches(manifest: BootstrapManifest) -> bool:
    """Prove the installed release binary reports the exact pinned version."""
    try:
        require_version("sc-compose", command_output([str(cargo_bin_path("sc-compose")), "--version"]), manifest.sc_compose)
    except (BootstrapError, OSError):
        return False
    return True


WYVERN_RELEASE_REPOSITORY = "randlee/wyvern"


def wyvern_asset_name(target: str) -> str:
    """Return the pinned Wyvern release asset for one supported target."""
    if target == "aarch64-apple-darwin":
        return "wyvern-macos-aarch64.tar.gz"
    if target == "x86_64-apple-darwin":
        return "wyvern-macos-x86_64.tar.gz"
    if target == "x86_64-pc-windows-msvc":
        return "wyvern-windows.zip"
    if target == "x86_64-unknown-linux-gnu":
        return "wyvern-linux.tar.gz"
    raise BootstrapError(f"Wyvern has no prebuilt release asset for target {target}.")


def wyvern_release_url(version: str, asset: str) -> str:
    """Return the immutable GitHub URL for one pinned Wyvern asset."""
    return f"https://github.com/{WYVERN_RELEASE_REPOSITORY}/releases/download/v{version}/{asset}"


def wyvern_install_command(version: str, target: str) -> tuple[str, str]:
    """Describe the pinned Wyvern release install for dry-run/tests."""
    asset = wyvern_asset_name(target)
    return asset, wyvern_release_url(version, asset)


def _wyvern_release_checksum(manifest: BootstrapManifest, target: str) -> str:
    """Return the manifest checksum for one pinned Wyvern release asset."""
    asset = wyvern_asset_name(target)
    checksums = dict(manifest.wyvern_checksums)
    try:
        return checksums[asset]
    except KeyError as error:
        raise BootstrapError(f"Wyvern manifest has no checksum for release asset {asset}.") from error


def _extract_wyvern(archive: bytes, asset: str, destination: Path) -> None:
    """Extract only the Wyvern CLI from a verified release archive."""
    executable_name = "wyvern.exe" if asset.endswith(".zip") else "wyvern"
    with tempfile.TemporaryDirectory(prefix="atm-wyvern-") as temp_dir:
        archive_path = Path(temp_dir) / asset
        archive_path.write_bytes(archive)
        if asset.endswith(".zip"):
            with zipfile.ZipFile(archive_path) as package:
                members = {_safe_member_name(name): name for name in package.namelist()}
                member = next(
                    (original for safe, original in members.items() if Path(safe).name == executable_name),
                    None,
                )
                if member is None:
                    raise BootstrapError(f"verified Wyvern archive does not contain {executable_name}.")
                binary = package.read(member)
        else:
            with tarfile.open(archive_path, mode="r:gz") as package:
                member = next(
                    (item for item in package.getmembers() if Path(_safe_member_name(item.name)).name == executable_name),
                    None,
                )
                if member is None or not member.isfile():
                    raise BootstrapError(f"verified Wyvern archive does not contain {executable_name}.")
                extracted = package.extractfile(member)
                if extracted is None:
                    raise BootstrapError("verified Wyvern executable could not be read from the archive.")
                binary = extracted.read()
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.name}.tmp-{os.getpid()}")
    temporary.write_bytes(binary)
    if sys.platform != "win32":
        temporary.chmod(0o755)
    os.replace(temporary, destination)


def install_wyvern_release(manifest: BootstrapManifest, *, dry_run: bool) -> None:
    """Install the pinned Wyvern CLI release without compiling or guessing."""
    target = sc_compose_target()
    asset, url = wyvern_install_command(manifest.wyvern, target)
    expected = _wyvern_release_checksum(manifest, target)
    destination = cargo_bin_path("wyvern")
    print(f"+ download {url} -> {destination}")
    if dry_run:
        return
    archive = _download_release(url, label="Wyvern release asset")
    actual = hashlib.sha256(archive).hexdigest()
    if actual != expected:
        raise BootstrapError(
            f"Wyvern release checksum mismatch for {asset}: expected {expected}, found {actual}."
        )
    checksums_url = wyvern_release_url(manifest.wyvern, "checksums.txt")
    checksums = _download_release(checksums_url, label="Wyvern checksums").decode("utf-8", errors="strict")
    listed = next(
        (parts[0] for line in checksums.splitlines() if (parts := line.split()) and len(parts) >= 2 and parts[-1] == asset),
        None,
    )
    if listed != expected:
        raise BootstrapError(f"checksums.txt does not confirm the pinned Wyvern checksum for {asset}.")
    _extract_wyvern(archive, asset, destination)


def wyvern_matches(manifest: BootstrapManifest) -> bool:
    """Prove the installed Wyvern binary reports the exact pinned version."""
    try:
        require_version("Wyvern", command_output([str(cargo_bin_path("wyvern")), "--version"]), manifest.wyvern)
    except (BootstrapError, OSError):
        return False
    return True


def cargo_binstall_available() -> bool:
    """Return whether the CI/local environment has the prebuilt installer."""
    return shutil.which("cargo-binstall") is not None


def running_in_ci() -> bool:
    """Return whether bootstrap is running in a CI environment."""
    return os.environ.get("CI", "").strip().lower() in {"1", "true", "yes"}


def run(command: Sequence[str], *, dry_run: bool, allow_failure: bool = False) -> bool:
    """Execute one deterministic installation step or render it for review."""
    print("+", " ".join(command))
    if dry_run:
        return True
    result = subprocess.run(command, check=False, cwd=ROOT)
    if result.returncode != 0:
        if allow_failure:
            print(f"bootstrap: {' '.join(command)} unavailable; using registry fallback", file=sys.stderr)
            return False
        raise BootstrapError(f"bootstrap install command failed with exit {result.returncode}: {' '.join(command)}")
    return True


def python_package_version(python: Path, package: str) -> str:
    """Read one distribution version from the isolated bootstrap environment."""
    program = f"from importlib.metadata import version; print(version({package!r}))"
    return command_output([str(python), "-c", program]).strip()


def verify_installed_tools(manifest: BootstrapManifest, python: Path) -> None:
    """Prove every installed external tool reports the exact manifest version."""
    for name, version in manifest.cargo_tools:
        binary = cargo_bin_path(name)
        command_output([str(binary), "--version"])
        if not (registry_tool_matches(name, version, manifest.rust) or binstall_tool_matches(name, version)):
            raise BootstrapError(f"{name} is not the exact pinned registry/prebuilt version {version}.")
    sc_compose = cargo_bin_path("sc-compose")
    command_output([str(sc_compose), "--version"])
    if not sc_compose_matches(manifest):
        raise BootstrapError(f"sc-compose is not the exact pinned prebuilt release version {manifest.sc_compose}.")
    wyvern = cargo_bin_path("wyvern")
    command_output([str(wyvern), "--version"])
    if not wyvern_matches(manifest):
        raise BootstrapError(f"Wyvern is not the exact pinned prebuilt release version {manifest.wyvern}.")
    for package, version in manifest.python_packages:
        try:
            actual = python_package_version(python, package)
        except BootstrapError as error:
            raise BootstrapError(f"Python package {package} is not installed.") from error
        if actual != version:
            raise BootstrapError(f"Python package {package} must be exactly {version}; found {actual}.")


def bootstrap(manifest: BootstrapManifest, *, dry_run: bool) -> None:
    """Install the complete contract, then verify it rather than trusting installs."""
    synchronize_macos_seed_tools(manifest, dry_run=dry_run)
    if not dry_run:
        verify_seed_tools(manifest)
    python = ensure_bootstrap_venv(manifest, dry_run=dry_run)
    ci = running_in_ci()
    allowed_strategies = dict(manifest.cargo_allowed_strategies)
    for name, version in manifest.cargo_tools:
        if registry_tool_matches(name, version, manifest.rust) or binstall_tool_matches(name, version):
            continue
        installed = False
        if cargo_binstall_available():
            installed = run(
                cargo_binstall_command(
                    name,
                    version,
                    force=True,
                    allowed_strategies=allowed_strategies[name],
                ),
                dry_run=dry_run,
                allow_failure=not ci,
            )
        if not installed:
            if ci:
                raise BootstrapError(f"cargo-binstall could not install the exact prebuilt {name} {version} in CI.")
            run(cargo_install_command(name, version, force=True), dry_run=dry_run)
    if not sc_compose_matches(manifest):
        install_sc_compose_release(manifest, dry_run=dry_run)
    if not wyvern_matches(manifest):
        install_wyvern_release(manifest, dry_run=dry_run)
    run(pip_install_command(python), dry_run=dry_run)
    if not dry_run:
        verify_installed_tools(manifest, python)


def main(argv: Sequence[str]) -> int:
    """Run a real bootstrap or a reviewable command-only dry run."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dry-run", action="store_true", help="Print exact install commands without changing state.")
    args = parser.parse_args(argv[1:])
    try:
        bootstrap(load_manifest(), dry_run=args.dry_run)
    except (BootstrapError, KeyError, OSError, tomllib.TOMLDecodeError) as error:
        print(f"bootstrap refused: {error}", file=sys.stderr)
        return 1
    print("bootstrap complete: exact pinned tool contract verified.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
