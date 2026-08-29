"""atm-core-owned regression coverage for the release target manifest.

Not a kit file: `.github/scripts/release_artifacts.py` and the rest of
`.github/scripts/tests/` are installed byte-for-byte from the pinned
sc-publish kit (see README.sc-publish.md) and must never carry local
edits. This file lives outside that installed set so it is untouched by a
kit re-sync, and exercises the real, checked-in
`release/publish-artifacts.toml` (rendered from the consumer-owned
`release/sc-publish-consumer-input.json`) against the kit script's own
CLI commands.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest


def repo_root() -> Path:
    return next(
        path
        for path in Path(__file__).resolve().parents
        if (path / "install.py").is_file()
    )


def scripts_root() -> Path:
    return repo_root() / ".github" / "scripts"


def release_manifest() -> dict:
    manifest_path = repo_root() / "release" / "publish-artifacts.toml"
    return tomllib.loads(manifest_path.read_text(encoding="utf-8"))


def run_release_artifacts(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(scripts_root() / "release_artifacts.py"), *args],
        cwd=repo_root(),
        text=True,
        capture_output=True,
        check=False,
    )


def test_release_manifest_declares_native_aarch64_linux_target() -> None:
    """Regression-test the native aarch64 Linux release target (issue #1057).

    Every release target must be built natively (no cross-linker), and the
    manifest-declared target set is what release.yml's build matrix and the
    GitHub Release asset-pattern check both derive from.
    """
    manifest = release_manifest()
    targets = {entry["target"]: entry for entry in manifest["release_targets"]}
    assert "aarch64-unknown-linux-gnu" in targets
    aarch64_linux = targets["aarch64-unknown-linux-gnu"]
    assert aarch64_linux["os"] == "ubuntu-24.04-arm"
    assert aarch64_linux["archive"] == "tar.gz"
    # x86_64 Linux, aarch64 Linux, x86_64 macOS, aarch64 macOS, x86_64 Windows.
    assert len(targets) == 5


def test_release_manifest_validates_with_the_native_aarch64_linux_target() -> None:
    """The real manifest (with the new target) must pass validate-manifest."""
    result = run_release_artifacts(
        "validate-manifest",
        "--manifest",
        "release/publish-artifacts.toml",
        "--workspace-toml",
        "Cargo.toml",
    )
    assert result.returncode == 0, result.stderr
    assert "manifest validation passed" in result.stdout


def test_release_target_matrix_includes_the_native_aarch64_linux_entry() -> None:
    """release.yml's build matrix must carry the new target through unchanged."""
    result = run_release_artifacts(
        "release-target-matrix",
        "--manifest",
        "release/publish-artifacts.toml",
    )
    assert result.returncode == 0, result.stderr
    matrix = json.loads(result.stdout)
    assert {
        "target": "aarch64-unknown-linux-gnu",
        "os": "ubuntu-24.04-arm",
        "archive": "tar.gz",
    } in matrix["include"]


def test_homebrew_and_scoop_channels_do_not_reference_the_new_linux_target() -> None:
    """arm-linux Homebrew is a deliberate follow-up (#1057), not this change."""
    manifest = release_manifest()
    homebrew_assets = manifest["channels"]["homebrew"]["assets"]
    asset_targets = {asset["target"] for asset in homebrew_assets}
    assert "aarch64-unknown-linux-gnu" not in asset_targets
    assert manifest["channels"]["homebrew"]["renderer_target"] == "x86_64-unknown-linux-gnu"
    assert manifest["channels"]["scoop"]["renderer_target"] == "x86_64-unknown-linux-gnu"


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__]))
