#!/usr/bin/env python3
"""Install and verify the repository's exact CI/bootstrap tool contract."""

from __future__ import annotations

from dataclasses import dataclass
import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tomllib
from typing import Sequence


ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = ROOT / "tools" / "bootstrap.toml"
REQUIREMENTS_PATH = ROOT / "tools" / "bootstrap-requirements.txt"
VENV_PATH = ROOT / ".bootstrap-venv"
SC_COMPOSE_REPOSITORY = "https://github.com/randlee/sc-compose.git"


class BootstrapError(RuntimeError):
    """The pinned bootstrap contract cannot be satisfied safely."""


@dataclass(frozen=True)
class BootstrapManifest:
    rust: str
    python: str
    just: str
    cargo_tools: tuple[tuple[str, str], ...]
    sc_compose_rev: str
    python_packages: tuple[tuple[str, str], ...]


def load_manifest(path: Path = MANIFEST_PATH) -> BootstrapManifest:
    """Load the exact tool versions from the one checked-in contract."""
    raw = tomllib.loads(path.read_text(encoding="utf-8"))
    toolchain = raw["toolchain"]
    cargo = raw["cargo"]
    python = raw["python"]
    return BootstrapManifest(
        rust=toolchain["rust"],
        python=toolchain["python"],
        just=toolchain["just"],
        cargo_tools=(
            ("cargo-deny", cargo["cargo-deny"]),
            ("cargo-audit", cargo["cargo-audit"]),
            ("cargo-shear", cargo["cargo-shear"]),
            ("cargo-modules", cargo["cargo-modules"]),
        ),
        sc_compose_rev=cargo["sc-compose-rev"],
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


def sc_compose_install_command(revision: str, *, force: bool) -> list[str]:
    """Return the authoritative sc-compose source-revision install command."""
    command = [
        "cargo",
        "install",
        "--git",
        SC_COMPOSE_REPOSITORY,
        "--rev",
        revision,
        "--locked",
    ]
    if force:
        command.append("--force")
    return [*command, "--bin", "sc-compose"]


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


def cargo_receipts() -> dict[str, object]:
    """Load Cargo's installation receipt for exact Git-source verification."""
    receipt = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / ".crates2.json"
    try:
        raw = json.loads(receipt.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError) as error:
        raise BootstrapError(f"Cargo installation receipt is unavailable: {receipt}") from error
    installs = raw.get("installs")
    if not isinstance(installs, dict):
        raise BootstrapError(f"Cargo installation receipt has no installs map: {receipt}")
    return installs


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


def verify_sc_compose_receipt(revision: str) -> None:
    """Prove Cargo installed sc-compose from the manifest's exact Git revision."""
    if not receipt_matches(
        "sc-compose ",
        required_fragments=(f"git+{SC_COMPOSE_REPOSITORY}", revision),
    ):
        raise BootstrapError(f"sc-compose was not installed from the exact source revision {revision}.")


def sc_compose_matches(manifest: BootstrapManifest) -> bool:
    """Prove the Git source and compiler match before omitting a rebuild."""
    return receipt_matches(
        "sc-compose ",
        required_fragments=(
            f"git+{SC_COMPOSE_REPOSITORY}",
            manifest.sc_compose_rev,
            f"release: {manifest.rust}",
        ),
    )


def run(command: Sequence[str], *, dry_run: bool) -> None:
    """Execute one deterministic installation step or render it for review."""
    print("+", " ".join(command))
    if dry_run:
        return
    result = subprocess.run(command, check=False, cwd=ROOT)
    if result.returncode != 0:
        raise BootstrapError(f"bootstrap install command failed with exit {result.returncode}: {' '.join(command)}")


def python_package_version(python: Path, package: str) -> str:
    """Read one distribution version from the isolated bootstrap environment."""
    program = f"from importlib.metadata import version; print(version({package!r}))"
    return command_output([str(python), "-c", program]).strip()


def verify_installed_tools(manifest: BootstrapManifest, python: Path) -> None:
    """Prove every installed external tool reports the exact manifest version."""
    for name, version in manifest.cargo_tools:
        binary = cargo_bin_path(name)
        command_output([str(binary), "--version"])
        if not registry_tool_matches(name, version, manifest.rust):
            raise BootstrapError(f"{name} is not the exact pinned version built by Rust {manifest.rust}.")
    sc_compose = cargo_bin_path("sc-compose")
    command_output([str(sc_compose), "--version"])
    verify_sc_compose_receipt(manifest.sc_compose_rev)
    if not sc_compose_matches(manifest):
        raise BootstrapError(f"sc-compose was not built by the pinned Rust {manifest.rust} toolchain.")
    for package, version in manifest.python_packages:
        try:
            actual = python_package_version(python, package)
        except BootstrapError as error:
            raise BootstrapError(f"Python package {package} is not installed.") from error
        if actual != version:
            raise BootstrapError(f"Python package {package} must be exactly {version}; found {actual}.")


def bootstrap(manifest: BootstrapManifest, *, dry_run: bool) -> None:
    """Install the complete contract, then verify it rather than trusting installs."""
    verify_seed_tools(manifest)
    python = ensure_bootstrap_venv(manifest, dry_run=dry_run)
    for name, version in manifest.cargo_tools:
        run(cargo_install_command(name, version, force=not registry_tool_matches(name, version, manifest.rust)), dry_run=dry_run)
    run(sc_compose_install_command(manifest.sc_compose_rev, force=not sc_compose_matches(manifest)), dry_run=dry_run)
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
