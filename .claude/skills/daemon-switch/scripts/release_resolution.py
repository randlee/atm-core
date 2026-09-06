"""Published-release and tagged-worktree resolution for daemon-switch.

This module deliberately contains no selector or service-manager mutation.
Callers receive a verified pair and perform the lifecycle transition separately.
"""

from __future__ import annotations

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
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from macos_development_signing import (
    CLI_IDENTIFIER,
    DAEMON_IDENTIFIER,
    SigningIdentity,
    SigningIdentityError,
    resolve_apple_development_identity,
    verify_signing_identity,
)


STABLE_VERSION = re.compile(r"^\d+\.\d+\.\d+$")
PRERELEASE_TAG_PREFIX = "prerelease/v"
GITHUB_RELEASES_API = "https://api.github.com/repos/randlee/atm-core/releases"


class SwitchError(RuntimeError):
    """A precondition that protects the singleton daemon was not met."""


def executable_name(name: str) -> str:
    return f"{name}.exe" if os.name == "nt" else name


def run(
    args: Sequence[str], *, timeout: float = 10.0, cwd: Path | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, text=True, capture_output=True, timeout=timeout, check=False, cwd=cwd)


def version(path: Path) -> str | None:
    try:
        result = run([str(path), "--version"], timeout=5.0)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return result.stdout.strip() or result.stderr.strip() or None


def selected_release_version(path: Path) -> str | None:
    """Read only the CLI version used for Homebrew provenance."""
    return binary_release_version(path, "selected ATM CLI")


def command_path(name: str, override: str | None, option: str) -> Path:
    raw = override or shutil.which(executable_name(name))
    if raw is None:
        raise SwitchError(f"cannot find {executable_name(name)} on PATH; pass {option}")
    return Path(raw).expanduser().absolute()


def require_executable(path: Path, label: str) -> Path:
    resolved = path.expanduser().resolve()
    if not resolved.is_file():
        raise SwitchError(f"{label} does not exist: {path}")
    if not os.access(resolved, os.X_OK):
        raise SwitchError(f"{label} is not executable: {resolved}")
    return resolved


def macos_development_signing_identity_available() -> bool:
    if platform.system() != "Darwin":
        return False
    try:
        resolve_apple_development_identity()
    except (OSError, subprocess.SubprocessError, SigningIdentityError):
        return False
    return True


def macos_binary_has_development_signature(binary: Path, identifier: str, identity: SigningIdentity) -> bool:
    try:
        return verify_signing_identity(str(binary), identifier, identity)
    except (OSError, subprocess.SubprocessError):
        return False


def require_macos_development_signatures(cli: Path, daemon: Path) -> None:
    system = platform.system()
    if system == "Windows":
        print("warning: Windows signing not yet implemented; skipping ATM signature gate.", file=sys.stderr)
        return
    if system != "Darwin":
        return
    try:
        identity = resolve_apple_development_identity()
    except (OSError, subprocess.SubprocessError, SigningIdentityError) as error:
        raise SwitchError(f"Apple Development signing preflight failed: {error}") from error
    for label, binary, identifier in (("CLI", cli, CLI_IDENTIFIER), ("daemon", daemon, DAEMON_IDENTIFIER)):
        if not macos_binary_has_development_signature(binary, identifier, identity):
            raise SwitchError(
                f"{label} target is not strictly signed by the required signing identity: {binary}. "
                "Build with `just build` or run `python3 .just/sign_daemon_dev.py` before daemon-switch."
            )


def homebrew_release_metadata() -> dict[str, object]:
    brew = shutil.which("brew")
    if brew is None:
        raise SwitchError("cannot verify Homebrew release provenance: brew is not installed")
    try:
        result = run([brew, "info", "--json=v2", "--installed", "atm"], timeout=10.0)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SwitchError(f"cannot verify Homebrew release provenance: {error}") from error
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SwitchError(f"cannot verify Homebrew release provenance: {detail or 'brew info failed'}")
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise SwitchError("cannot verify Homebrew release provenance: brew info returned invalid JSON") from error
    formulae = payload.get("formulae") if isinstance(payload, dict) else None
    if not isinstance(formulae, list):
        raise SwitchError("cannot verify Homebrew release provenance: ATM formula metadata is missing")
    matches = [
        formula for formula in formulae
        if isinstance(formula, dict) and (
            formula.get("name") == "atm"
            or (isinstance(formula.get("full_name"), str) and formula["full_name"].rsplit("/", 1)[-1] == "atm")
        )
    ]
    if len(matches) != 1:
        raise SwitchError("cannot verify Homebrew release provenance: ATM formula metadata is missing")
    return matches[0]


def require_homebrew_release_provenance(cli: Path, daemon: Path) -> None:
    if platform.system() != "Darwin":
        return
    cli = require_executable(cli, "Homebrew atm CLI")
    daemon = require_executable(daemon, "Homebrew atm daemon")
    brew = shutil.which("brew")
    if brew is None:
        raise SwitchError("cannot verify Homebrew release provenance: brew is not installed")
    try:
        prefix_result = run([brew, "--prefix", "atm"], timeout=10.0)
        prefix = Path(prefix_result.stdout.strip()).expanduser().resolve()
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        raise SwitchError(f"cannot verify Homebrew release provenance: invalid formula prefix: {error}") from error
    if prefix_result.returncode != 0:
        raise SwitchError("cannot verify Homebrew release provenance: brew prefix failed")
    expected_bin_dir = (prefix / "bin").resolve()
    for label, binary in (("CLI", cli), ("daemon", daemon)):
        try:
            binary.relative_to(expected_bin_dir)
        except ValueError as error:
            raise SwitchError(f"{label} target is not the installed Homebrew ATM binary under {expected_bin_dir}: {binary}") from error
    formula = homebrew_release_metadata()
    if formula.get("homepage") != "https://github.com/randlee/atm-core":
        raise SwitchError("Homebrew ATM formula has an unexpected project homepage")
    cli_version = selected_release_version(cli)
    versions, installed, urls = formula.get("versions"), formula.get("installed"), formula.get("urls")
    if not isinstance(versions, dict) or versions.get("stable") != cli_version:
        raise SwitchError("Homebrew ATM formula stable version does not match the selected binaries")
    if not isinstance(installed, list) or len(installed) != 1 or not isinstance(installed[0], dict) or installed[0].get("version") != cli_version:
        raise SwitchError("Homebrew ATM installed version does not match the selected binaries")
    stable = urls.get("stable") if isinstance(urls, dict) else None
    release_url = stable.get("url") if isinstance(stable, dict) else None
    checksum = stable.get("checksum") if isinstance(stable, dict) else None
    expected_prefix = f"https://github.com/randlee/atm-core/releases/download/v{cli_version}/"
    if not isinstance(release_url, str) or not release_url.startswith(expected_prefix):
        raise SwitchError("Homebrew ATM formula does not point at the matching GitHub Release asset")
    if not isinstance(checksum, str) or re.fullmatch(r"[0-9a-fA-F]{64}", checksum) is None:
        raise SwitchError("Homebrew ATM formula has no valid SHA-256 release asset checksum")


def homebrew_pair() -> tuple[Path, Path] | None:
    brew = shutil.which("brew")
    if brew is None:
        return None
    result = run([brew, "--prefix", "atm"], timeout=10.0)
    if result.returncode != 0:
        return None
    prefix = Path(result.stdout.strip())
    cli, daemon = prefix / "bin" / executable_name("atm"), prefix / "bin" / executable_name("atm-daemon")
    return (cli.resolve(), daemon.resolve()) if cli.is_file() and daemon.is_file() else None


def require_macos_restore_provenance(cli: Path, daemon: Path) -> None:
    if platform.system() != "Darwin":
        return
    brew_pair = homebrew_pair()
    try:
        selected = (cli.expanduser().resolve(), daemon.expanduser().resolve())
    except (OSError, RuntimeError) as error:
        raise SwitchError(f"cannot resolve restore targets: {error}") from error
    if brew_pair is not None and selected == brew_pair:
        require_homebrew_release_provenance(*brew_pair)
    else:
        require_macos_development_signatures(cli, daemon)


def binary_release_version(binary: Path, label: str) -> str:
    reported = version(binary)
    if not reported:
        raise SwitchError(f"cannot determine {label} version: {binary}")
    candidate = reported.rsplit(maxsplit=1)[-1]
    if STABLE_VERSION.fullmatch(candidate) is None:
        raise SwitchError(f"cannot determine {label} version from {reported!r}: {binary}")
    return candidate


def require_pair_version(cli: Path, daemon: Path, expected: str) -> None:
    cli_version, daemon_version = binary_release_version(cli, "ATM CLI"), binary_release_version(daemon, "ATM daemon")
    if cli_version != expected or daemon_version != expected:
        raise SwitchError(f"target CLI/daemon versions must both equal {expected}; found cli={cli_version}, daemon={daemon_version}")


def github_json(path: str) -> object:
    request = Request(f"{GITHUB_RELEASES_API}{path}", headers={"Accept": "application/vnd.github+json", "User-Agent": "atm-daemon-switch"})
    try:
        with urlopen(request, timeout=10) as response:  # noqa: S310 - fixed GitHub API origin.
            return json.loads(response.read().decode("utf-8"))
    except HTTPError as error:
        if error.code == 404:
            return None
        raise SwitchError(f"cannot resolve GitHub release: HTTP {error.code}") from error
    except (OSError, URLError, TimeoutError, json.JSONDecodeError) as error:
        raise SwitchError("cannot resolve GitHub release while offline or unavailable; connect to the network and retry") from error


def latest_published_release_version() -> str:
    payload = github_json("")
    if not isinstance(payload, list):
        raise SwitchError("cannot resolve latest published release: GitHub returned an invalid release list")
    for release in payload:
        tag = release.get("tag_name") if isinstance(release, dict) and not release.get("draft") and not release.get("prerelease") else None
        if isinstance(tag, str) and tag.startswith("v") and STABLE_VERSION.fullmatch(tag[1:]):
            return tag[1:]
    raise SwitchError("cannot resolve latest published release: no stable GitHub release exists")


def release_is_published(version_value: str) -> bool:
    payload = github_json(f"/tags/v{version_value}")
    return isinstance(payload, dict) and not payload.get("draft") and not payload.get("prerelease")


def release_archive_triple() -> tuple[str, str]:
    system, machine = platform.system(), platform.machine().lower()
    triples = {
        ("Linux", "x86_64"): ("x86_64-unknown-linux-gnu", "tar.gz"),
        ("Linux", "amd64"): ("x86_64-unknown-linux-gnu", "tar.gz"),
        ("Linux", "aarch64"): ("aarch64-unknown-linux-gnu", "tar.gz"),
        ("Linux", "arm64"): ("aarch64-unknown-linux-gnu", "tar.gz"),
        ("Darwin", "x86_64"): ("x86_64-apple-darwin", "tar.gz"),
        ("Darwin", "amd64"): ("x86_64-apple-darwin", "tar.gz"),
        ("Darwin", "aarch64"): ("aarch64-apple-darwin", "tar.gz"),
        ("Darwin", "arm64"): ("aarch64-apple-darwin", "tar.gz"),
        ("Windows", "x86_64"): ("x86_64-pc-windows-msvc", "zip"),
        ("Windows", "amd64"): ("x86_64-pc-windows-msvc", "zip"),
    }
    try:
        return triples[(system, machine)]
    except KeyError as error:
        raise SwitchError(f"no published ATM archive is available for {system} {machine}") from error


def release_install_roots() -> list[Path]:
    system = platform.system()
    if system == "Darwin":
        pair = homebrew_pair()
        return [pair[0].parent] if pair is not None else []
    if system == "Windows":
        local = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local"))
        profile = Path(os.environ.get("USERPROFILE", Path.home()))
        roots = [local / "Programs" / "ATM", local / "Programs" / "atm", profile / "scoop" / "apps" / "atm" / "current"]
        packages = local / "Microsoft" / "WinGet" / "Packages"
        if packages.is_dir():
            roots.extend(path for path in packages.glob("*ATM*") if path.is_dir())
        return roots
    return [Path("/usr/bin"), Path("/usr/local/bin"), Path.home() / ".local" / "bin"]


def pair_from_root(root: Path) -> tuple[Path, Path] | None:
    for directory in (root, root / "bin"):
        cli, daemon = directory / executable_name("atm"), directory / executable_name("atm-daemon")
        if cli.is_file() and daemon.is_file():
            return cli.resolve(), daemon.resolve()
    return None


def state_path() -> Path:
    root = Path(os.environ.get("APPDATA", Path.home() / "AppData" / "Roaming")) if os.name == "nt" else Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    return root / "atm" / "daemon-switch.json"


def load_state() -> dict[str, str]:
    path = state_path()
    if not path.is_file():
        return {}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    return data if isinstance(data, dict) else {}


def save_default_pair(cli: Path, daemon: Path) -> None:
    path, data = state_path(), load_state()
    data.setdefault("default_cli", str(cli))
    data.setdefault("default_daemon", str(daemon))
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def extract_linux_release_archive(version_value: str) -> tuple[Path, Path]:
    triple, extension = release_archive_triple()
    archive_name = f"atm_{version_value}_{triple}.{extension}"
    destination = state_path().with_name("released-pairs") / version_value / triple
    cli, daemon = destination / "bin" / executable_name("atm"), destination / "bin" / executable_name("atm-daemon")
    if cli.is_file() and daemon.is_file():
        return cli, daemon
    request = Request(f"https://github.com/randlee/atm-core/releases/download/v{version_value}/{archive_name}", headers={"User-Agent": "atm-daemon-switch"})
    try:
        with urlopen(request, timeout=30) as response:  # noqa: S310 - fixed release origin.
            archive = response.read()
    except (OSError, URLError, TimeoutError) as error:
        raise SwitchError(f"cannot download ATM {version_value} for Linux; install the package or reconnect and retry") from error
    if extension != "tar.gz":
        raise SwitchError(f"unexpected Linux archive format: {extension}")
    destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    staging = destination.with_name(f".{destination.name}.download")
    import io
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
        members = bundle.getmembers()
        if any(not (staging / member.name).resolve().is_relative_to(staging.resolve()) for member in members):
            raise SwitchError("published release archive contains an unsafe path")
        bundle.extractall(staging, members=members, filter="data")
    roots = [path for path in staging.iterdir() if path.is_dir()]
    if len(roots) != 1:
        raise SwitchError("published release archive has an unexpected layout")
    os.replace(roots[0], destination)
    shutil.rmtree(staging, ignore_errors=True)
    return require_executable(cli, "downloaded ATM CLI"), require_executable(daemon, "downloaded ATM daemon")


def resolve_release_pair(requested: str) -> tuple[Path, Path, str]:
    expected = latest_published_release_version() if requested == "latest" else requested
    if STABLE_VERSION.fullmatch(expected) is None:
        raise SwitchError("--release must be a stable X.Y.Z version or 'latest'")
    for root in release_install_roots():
        pair = pair_from_root(root)
        if pair is None:
            continue
        try:
            require_pair_version(*pair, expected)
        except SwitchError:
            continue
        return *pair, expected
    if platform.system() == "Linux":
        cli, daemon = extract_linux_release_archive(expected)
        require_pair_version(cli, daemon, expected)
        return cli, daemon, expected
    raise SwitchError(f"cannot find installed ATM {expected} on this platform; install the published release and retry")


def workspace_version(worktree: Path) -> str:
    try:
        manifest = tomllib.loads((worktree / "Cargo.toml").read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise SwitchError(f"cannot read worktree Cargo.toml: {error}") from error
    value = manifest.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(value, str) or STABLE_VERSION.fullmatch(value) is None:
        raise SwitchError("worktree workspace.package.version must be stable X.Y.Z")
    return value


def exact_prerelease_tag(worktree: Path) -> str:
    result = run(["git", "-C", str(worktree), "tag", "--points-at", "HEAD"], timeout=10)
    if result.returncode != 0:
        raise SwitchError(f"cannot inspect worktree HEAD tags: {(result.stderr or result.stdout).strip()}")
    expected = f"{PRERELEASE_TAG_PREFIX}{workspace_version(worktree)}"
    if expected not in result.stdout.splitlines():
        raise SwitchError(f"worktree HEAD requires exact tag {expected}; run `python3 .just/prerelease_tag.py` on that branch, then rebuild")
    return expected[len(PRERELEASE_TAG_PREFIX):]


def prepare_worktree_pair(worktree: Path, bump: bool) -> tuple[Path, Path, str]:
    root = worktree.expanduser().resolve()
    if not root.is_dir() or not (root / ".git").exists():
        raise SwitchError(f"--worktree must name a git worktree: {worktree}")
    if bump:
        result = run([sys.executable, str(root / ".just" / "prerelease_tag.py")], cwd=root, timeout=180)
        if result.returncode != 0:
            raise SwitchError(f"prerelease tagging failed: {(result.stderr or result.stdout).strip()}")
        result = run(["cargo", "build", "--release", "-p", "agent-team-mail", "-p", "atm-daemon"], cwd=root, timeout=600)
        if result.returncode != 0:
            raise SwitchError(f"release build after prerelease tag failed: {(result.stderr or result.stdout).strip()}")
    expected = exact_prerelease_tag(root)
    cli = require_executable(root / "target" / "release" / executable_name("atm"), "worktree ATM CLI")
    daemon = require_executable(root / "target" / "release" / executable_name("atm-daemon"), "worktree ATM daemon")
    require_pair_version(cli, daemon, expected)
    return cli, daemon, expected
