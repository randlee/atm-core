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
import shutil
import subprocess
import sys
import tomllib
from typing import Sequence


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
    sc_compose: str
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
        sc_compose=cargo["sc-compose"],
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


def sc_compose_install_command(version: str, *, force: bool) -> list[str]:
    """Return the registry install command for the released sc-compose CLI."""
    command = ["cargo", "install", "--locked"]
    if force:
        command.append("--force")
    return [*command, "--version", version, "sc-compose"]


def cargo_binstall_command(name: str, version: str, *, force: bool) -> list[str]:
    """Return a non-compiling cargo-binstall command for one registry tool.

    Disable Binstall's compile strategy so an unavailable artifact takes the
    explicit Cargo registry fallback below instead of silently rebuilding from
    source.  The quick-install strategy is also disabled: CI must consume the
    tool's own release artifact or use the verified registry fallback.
    """
    command = [
        "cargo",
        "binstall",
        "--no-confirm",
        "--disable-telemetry",
        "--disable-strategies",
        "quick-install,compile",
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
        info = record.get("crate_info")
        if not isinstance(info, dict):
            continue
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
    """Prove the released sc-compose CLI is installed from the registry."""
    return registry_tool_matches("sc-compose", manifest.sc_compose, manifest.rust)


def cargo_binstall_available() -> bool:
    """Return whether the CI/local environment has the prebuilt installer."""
    return shutil.which("cargo-binstall") is not None


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
        raise BootstrapError(f"sc-compose is not the exact released registry version {manifest.sc_compose}.")
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
    for name, version in manifest.cargo_tools:
        if registry_tool_matches(name, version, manifest.rust) or binstall_tool_matches(name, version):
            continue
        installed = False
        if cargo_binstall_available():
            installed = run(
                cargo_binstall_command(name, version, force=True),
                dry_run=dry_run,
                allow_failure=True,
            )
        if not installed:
            run(cargo_install_command(name, version, force=True), dry_run=dry_run)
    if not sc_compose_matches(manifest):
        run(sc_compose_install_command(manifest.sc_compose, force=True), dry_run=dry_run)
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
