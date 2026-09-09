from __future__ import annotations

import hashlib
import importlib.util
import io
import re
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[3] / ".claude" / "skills" / "prerelease" / "scripts" / "prerelease.py"
SPEC = importlib.util.spec_from_file_location("sc_publish_prerelease", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
PRERELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PRERELEASE)


def manifest(root: Path) -> dict[str, object]:
    return {
        "project": {"archive_prefix": "fixture"},
        "release_targets": [
            {"target": "x86_64-unknown-linux-gnu", "archive": "tar.gz"},
        ],
        "prerelease": {
            "tag_prefix": "prerelease/v",
            "tag_script": ".just/prerelease_tag.py",
            "install_root": str(root / "builds"),
            "binaries": ["fixture"],
            "selector_dir": {"darwin": str(root / "darwin"), "linux": str(root / "selector"), "windows": str(root / "windows")},
            "post_install": "true",
            "verify": "echo 1.5.11",
        },
    }


def write_manifest(root: Path) -> None:
    (root / "release").mkdir()
    (root / "release" / "publish-artifacts.toml").write_text(
        "[prerelease]\n"
        'tag_prefix = "prerelease/v"\n'
        'tag_script = ".just/prerelease_tag.py"\n'
        'install_root = "~/.fixture-builds"\n'
        'binaries = ["fixture"]\n'
        'selector_dir = { darwin = "/tmp", linux = "/tmp", windows = "C:\\\\tmp" }\n'
        'post_install = "true"\n'
        'verify = "echo 1.5.11"\n',
        encoding="utf-8",
    )


def fixture_archive() -> bytes:
    contents = io.BytesIO()
    with zipfile.ZipFile(contents, "w") as archive:
        archive.writestr("fixture_1.5.11_x86_64-unknown-linux-gnu/bin/fixture", "fixture")
    return contents.getvalue()


class PrereleaseTests(unittest.TestCase):
    def test_publish_dry_run_never_needs_network_or_authorization(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_manifest(root)
            result = subprocess.run([sys.executable, str(SCRIPT), "--manifest", "release/publish-artifacts.toml", "--create", "--dry-run"], cwd=root, text=True, capture_output=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("would tag", result.stdout)

    def test_publish_refuses_without_written_authorization(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_manifest(root)
            result = subprocess.run([sys.executable, str(SCRIPT), "--manifest", "release/publish-artifacts.toml", "--create"], cwd=root, text=True, capture_output=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("authorization", result.stderr)

    def test_publish_tags_waits_and_verifies_release_assets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            values = manifest(root)
            archive = fixture_archive()
            name = "fixture_1.5.11_x86_64-unknown-linux-gnu.tar.gz"
            digest = hashlib.sha256(archive).hexdigest()
            release = {"url": "https://example.test/release", "assets": [{"name": "checksums.txt"}, {"name": name}]}

            def download(_tag: str, asset: str, destination: Path) -> Path:
                path = destination / asset
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(f"{digest}  {name}\n".encode() if asset == "checksums.txt" else archive)
                return path

            tag_result = subprocess.CompletedProcess(
                [], 0, stdout="created and pushed prerelease/v1.5.11 from fixture\n"
            )
            with (
                mock.patch.object(PRERELEASE, "require_publish_preconditions"),
                mock.patch.object(PRERELEASE, "command", return_value=tag_result),
                mock.patch.object(PRERELEASE, "wait_for_archive") as wait,
                mock.patch.object(PRERELEASE, "release_for_tag", return_value=release),
                mock.patch.object(PRERELEASE, "download_asset", side_effect=download),
            ):
                tag, url = PRERELEASE.publish(values)
        self.assertEqual((tag, url), ("prerelease/v1.5.11", "https://example.test/release"))
        wait.assert_called_once_with("prerelease/v1.5.11")

    def test_create_rejects_an_operator_supplied_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_manifest(root)
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--create", "1.5.11"],
                cwd=root,
                text=True,
                capture_output=True,
                check=False,
            )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unrecognized arguments: 1.5.11", result.stderr)

    def test_install_stages_repoints_and_reuses_a_local_pair(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            values = manifest(root)
            archive = root / "fixture.zip"
            archive.write_bytes(fixture_archive())
            config = values["prerelease"]
            assert isinstance(config, dict)
            stage = root / "builds" / "v1.5.11"
            with (
                mock.patch.object(PRERELEASE, "select_release", return_value=("1.5.11", {})),
                mock.patch.object(PRERELEASE, "download_checked_archive", return_value=archive) as download,
                mock.patch.object(PRERELEASE.platform, "system", return_value="Linux"),
            ):
                version, installed = PRERELEASE.install(values, "1.5.11")
                PRERELEASE.install(values, "1.5.11")
                self.assertEqual((version, installed), ("1.5.11", stage))
                self.assertTrue((stage / "bin" / "fixture").is_file())
                self.assertTrue((root / "selector" / "fixture").is_symlink())
                download.assert_called_once()

    def test_checksum_verification_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "fixture.zip"
            archive.write_bytes(b"wrong archive")
            with self.assertRaisesRegex(SystemExit, "sha256 mismatch"):
                PRERELEASE.verify_checksum(archive, "0" * 64)

    def test_safe_extract_rejects_zip_path_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "unsafe.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("../escaped", "unsafe")
            with self.assertRaisesRegex(SystemExit, "unsafe path"):
                PRERELEASE.safe_extract(archive, root / "extract")
            self.assertFalse((root / "escaped").exists())

    def test_prerelease_workflow_converges_without_clobbering_a_release(self) -> None:
        workflow = (SCRIPT.parents[4] / ".github" / "workflows" / "prerelease-archive.yml").read_text(encoding="utf-8")
        self.assertIn('gh release create "$tag" --prerelease', workflow)
        self.assertIn('gh release view "$tag" --json isDraft,isPrerelease,assets', workflow)
        self.assertIn('shasum -a 256 "${archives[@]}" > checksums.txt', workflow)
        self.assertIn('gh release create "$tag" --prerelease --title "$tag" --generate-notes "${archives[@]}" checksums.txt', workflow)
        self.assertIn('cmp checksums.txt existing-release/checksums.txt', workflow)
        self.assertIn("concurrent run converged", workflow)
        self.assertLess(
            workflow.index('gh release create "$tag" --prerelease'),
            workflow.index('gh release view "$tag" --json isDraft,isPrerelease,assets'),
        )
        self.assertNotIn("gh release upload \"$tag\" --clobber", workflow)

    def test_prerelease_workflow_uses_manifest_build_contract(self) -> None:
        workflow = (SCRIPT.parents[4] / ".github" / "workflows" / "prerelease-archive.yml").read_text(encoding="utf-8")
        self.assertIn("toolchain: ${{ needs.plan.outputs.rust_toolchain }}", workflow)
        self.assertIn("uses: ./.github/actions/install-linux-native-deps", workflow)
        self.assertIn("ref: ${{ needs.plan.outputs.source_sha }}", workflow)
        self.assertIn('for bundled_path in binary.get("bundled_paths", []):', workflow)
        self.assertIn("path: ${{ env.ARCHIVE }}", workflow)
        self.assertIn("permissions:\n  contents: read", workflow)
        self.assertIn("    permissions:\n      contents: write", workflow)
        self.assertIn("timeout-minutes: 45", workflow)
        self.assertNotRegex(workflow, r":\s*\{[^\n]*\$\{\{")


if __name__ == "__main__":
    unittest.main()
