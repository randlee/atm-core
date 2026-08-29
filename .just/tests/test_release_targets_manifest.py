"""atm-core-owned regression coverage for the release target manifest.

Not a kit file: `.github/scripts/release_artifacts.py` and the rest of
`.github/scripts/tests/` are installed byte-for-byte from the pinned
sc-publish kit (see README.sc-publish.md) and must never carry local
edits. This file lives in `.just/tests/`, outside that installed set, so it
is untouched by a kit re-sync, and exercises the real, checked-in
`release/publish-artifacts.toml` (rendered from the consumer-owned
`release/sc-publish-consumer-input.json`) against the kit script's own
CLI commands.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tomllib
import unittest
from pathlib import Path


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
        timeout=60,
    )


class ReleaseTargetsManifestTests(unittest.TestCase):
    def test_release_manifest_declares_native_aarch64_linux_target(self) -> None:
        """Regression-test the native aarch64 Linux release target (issue #1057).

        Every release target must be built natively (no cross-linker), and the
        manifest-declared target set is what release.yml's build matrix and the
        GitHub Release asset-pattern check both derive from.
        """
        manifest = release_manifest()
        targets = {entry["target"]: entry for entry in manifest["release_targets"]}
        self.assertIn("aarch64-unknown-linux-gnu", targets)
        aarch64_linux = targets["aarch64-unknown-linux-gnu"]
        self.assertEqual(aarch64_linux["os"], "ubuntu-24.04-arm")
        self.assertEqual(aarch64_linux["archive"], "tar.gz")
        # x86_64 Linux, aarch64 Linux, x86_64 macOS, aarch64 macOS, x86_64 Windows.
        self.assertEqual(len(targets), 5)

    def test_release_manifest_validates_with_the_native_aarch64_linux_target(self) -> None:
        """The real manifest (with the new target) must pass validate-manifest."""
        result = run_release_artifacts(
            "validate-manifest",
            "--manifest",
            "release/publish-artifacts.toml",
            "--workspace-toml",
            "Cargo.toml",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("manifest validation passed", result.stdout)

    def test_release_target_matrix_includes_the_native_aarch64_linux_entry(self) -> None:
        """release.yml's build matrix must carry the new target through unchanged."""
        result = run_release_artifacts(
            "release-target-matrix",
            "--manifest",
            "release/publish-artifacts.toml",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        matrix = json.loads(result.stdout)
        self.assertIn(
            {
                "target": "aarch64-unknown-linux-gnu",
                "os": "ubuntu-24.04-arm",
                "archive": "tar.gz",
            },
            matrix["include"],
        )

    def test_homebrew_and_scoop_channels_do_not_reference_the_new_linux_target(self) -> None:
        """arm-linux Homebrew is a deliberate follow-up (#1057), not this change."""
        manifest = release_manifest()
        homebrew_assets = manifest["channels"]["homebrew"]["assets"]
        asset_targets = {asset["target"] for asset in homebrew_assets}
        self.assertNotIn("aarch64-unknown-linux-gnu", asset_targets)
        self.assertEqual(manifest["channels"]["homebrew"]["renderer_target"], "x86_64-unknown-linux-gnu")
        self.assertEqual(manifest["channels"]["scoop"]["renderer_target"], "x86_64-unknown-linux-gnu")


if __name__ == "__main__":
    unittest.main()
