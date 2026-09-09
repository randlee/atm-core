"""ATM-specific prerelease manifest and tag-helper contract tests."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest import mock

JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_common import discover_repo_root
from lint_common import workspace_manifest_paths
import prerelease_tag
from prerelease_tag import patch_bump
from prerelease_tag import workspace_version


class PrereleaseArchiveWorkflowTests(unittest.TestCase):
    def test_consumer_input_is_the_prerelease_manifest_source_of_truth(self) -> None:
        root = discover_repo_root()
        source = json.loads(
            (root / "release" / "sc-publish-consumer-input.json").read_text(encoding="utf-8")
        )["prerelease"]
        rendered = tomllib.loads(
            (root / "release" / "publish-artifacts.toml").read_text(encoding="utf-8")
        )["prerelease"]
        self.assertEqual(source, rendered)

    def test_prerelease_install_stages_before_the_daemon_switch_extension(self) -> None:
        root = discover_repo_root()
        prerelease = tomllib.loads(
            (root / "release" / "publish-artifacts.toml").read_text(encoding="utf-8")
        )["prerelease"]
        self.assertEqual(prerelease["binaries"], ["atm", "atm-daemon"])
        self.assertIn("daemon-switch.py switch --prerelease {version} --yes", prerelease["post_install"])
        self.assertTrue(prerelease["selector_dir"]["darwin"].startswith(prerelease["install_root"]))
        self.assertTrue(prerelease["selector_dir"]["linux"].startswith(prerelease["install_root"]))
        self.assertNotIn("Programs\\\\ATM", prerelease["selector_dir"]["windows"])

    def test_generic_workflow_preserves_manifest_build_and_plain_artifact_contracts(self) -> None:
        root = discover_repo_root()
        workflow = (root / ".github" / "workflows" / "prerelease-archive.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("toolchain: ${{ needs.plan.outputs.rust_toolchain }}", workflow)
        self.assertIn("uses: ./.github/actions/install-linux-native-deps", workflow)
        self.assertIn('for bundled_path in binary.get("bundled_paths", []):', workflow)
        self.assertIn("uses: actions/upload-artifact@v4", workflow)
        self.assertIn("name: ${{ matrix.target }}", workflow)
        self.assertIn('shasum -a 256 "${archives[@]}" > checksums.txt', workflow)
        self.assertIn('"${archives[@]}" checksums.txt', workflow)
        self.assertIn('gh release create "$tag" --prerelease', workflow)
        self.assertIn('cmp checksums.txt existing-release/checksums.txt', workflow)
        self.assertIn("concurrent run converged", workflow)
        self.assertNotIn('gh release upload "$tag" --clobber', workflow)
        self.assertNotIn("randlee/atm-core", workflow)

    def test_prerelease_tag_recipe_and_helper_have_protected_branch_and_dry_run_guards(self) -> None:
        root = discover_repo_root()
        justfile = (root / "Justfile").read_text(encoding="utf-8")
        helper = (root / ".just" / "prerelease_tag.py").read_text(encoding="utf-8")
        self.assertIn("prerelease-tag", justfile)
        self.assertIn(".just/prerelease_tag.py", justfile)
        self.assertIn("--dry-run", justfile)
        self.assertIn('branch in {"develop", "main"}', helper)
        self.assertIn("requires a clean working tree", helper)
        self.assertIn('"tag", "-a"', helper)
        self.assertIn('"push", "origin", branch', helper)

    def test_prerelease_tag_helper_patch_bumps_the_workspace_version(self) -> None:
        # Fixture-local values: this unit test exercises patch arithmetic, not the real workspace.
        self.assertEqual(patch_bump("1.4.5"), "1.4.6")
        self.assertEqual(patch_bump("9.99.0"), "9.99.1")

    def test_candidate_bump_updates_actual_lockfile_collision_safely(self) -> None:
        root = discover_repo_root()
        old_version = workspace_version(root)
        new_version = patch_bump(old_version)
        changes = prerelease_tag.candidate_changes(root, old_version, new_version)
        lock = tomllib.loads(changes[root / "Cargo.lock"])
        directives = [
            package for package in lock["package"] if package["name"] == "sc-lint-directives"
        ]
        self.assertEqual(
            [(package["version"], "source" in package) for package in directives],
            [("0.5.0", True), (new_version, False)],
        )
        self.assertEqual(
            tomllib.loads(changes[root / "crates" / "atm-query-python" / "pyproject.toml"])[
                "project"
            ]["version"],
            new_version,
        )
        expected = {
            root / "Cargo.toml",
            *(
                path
                for path in prerelease_tag.python_project_paths(root)
                if "version =" in path.read_text(encoding="utf-8")
            ),
            root / ".winget" / "randlee.agent-team-mail.yaml",
            root / "Cargo.lock",
        }
        self.assertEqual(set(changes), expected)
        self.assertNotIn(root / "crates" / "atm-graft-python" / "pyproject.toml", changes)
        for path in prerelease_tag.python_project_paths(root):
            if path not in changes:
                continue
            project = tomllib.loads(changes[path])["project"]
            self.assertEqual(project["version"], new_version)
        winget = changes[root / ".winget" / "randlee.agent-team-mail.yaml"]
        self.assertIn(f"PackageVersion: {new_version}", winget)
        self.assertIn(f"ManifestVersion: {new_version}", winget)
        self.assertIn(
            f"releases/download/v{new_version}/atm_{new_version}_x86_64-pc-windows-msvc.zip",
            winget,
        )

    def test_workspace_versions_and_internal_dependencies_are_centralized(self) -> None:
        root = discover_repo_root()
        root_manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
        workspace_dependencies = root_manifest["workspace"]["dependencies"]
        member_paths = workspace_manifest_paths(root)
        member_dirs = {path.parent.resolve() for path in member_paths}
        internal_dependency_names = {
            name
            for name, dependency in workspace_dependencies.items()
            if isinstance(dependency, dict)
            and isinstance(dependency.get("path"), str)
            and (root / dependency["path"]).resolve() in member_dirs
        }

        for manifest_path in member_paths:
            manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(
                manifest["package"]["version"],
                {"workspace": True},
                manifest_path.relative_to(root),
            )
            for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
                for dependency_name, dependency in manifest.get(section_name, {}).items():
                    if dependency_name not in internal_dependency_names:
                        continue
                    self.assertIsInstance(dependency, dict)
                    self.assertTrue(
                        dependency.get("workspace") is True,
                        f"{manifest_path.relative_to(root)} [{section_name}.{dependency_name}] "
                        "must inherit its workspace dependency",
                    )
                    self.assertNotIn("version", dependency)
                    self.assertNotIn("path", dependency)

    def test_write_and_commit_restores_files_when_commit_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=repo, check=True)
            subprocess.run(["git", "config", "user.name", "AS1.1 test"], cwd=repo, check=True)
            manifest = repo / "Cargo.toml"
            # Fixture-local versions: this test exercises rollback after a failed commit.
            original = "[workspace.package]\nversion = \"1.4.5\"\n"
            manifest.write_text(original, encoding="utf-8")
            subprocess.run(["git", "add", "Cargo.toml"], cwd=repo, check=True)
            subprocess.run(["git", "commit", "-qm", "fixture"], cwd=repo, check=True)

            real_git = prerelease_tag.git

            def fail_commit(repo_root: Path, *args: str, check: bool = True):
                if args and args[0] == "commit":
                    raise SystemExit("simulated commit failure")
                return real_git(repo_root, *args, check=check)

            with mock.patch.object(prerelease_tag, "git", side_effect=fail_commit):
                with self.assertRaises(SystemExit):
                    prerelease_tag.write_and_commit(
                        repo,
                        {manifest: original.replace("1.4.5", "1.4.6")},
                        "fixture bump",
                    )
            self.assertEqual(manifest.read_text(encoding="utf-8"), original)
            self.assertEqual(
                subprocess.run(
                    ["git", "status", "--porcelain"],
                    cwd=repo,
                    text=True,
                    capture_output=True,
                    check=True,
                ).stdout,
                "",
            )

    def test_just_prerelease_tag_dry_run_is_end_to_end_and_clean(self) -> None:
        root = discover_repo_root()
        old_version = workspace_version(root)
        new_version = patch_bump(old_version)
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory) / "atm-core"

            prerelease_tag.copy_tracked_files(root, repo)
            subprocess.run(["git", "init", "-q", "-b", "fixture"], cwd=repo, check=True)
            subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=repo, check=True)
            subprocess.run(["git", "config", "user.name", "AS1.1 test"], cwd=repo, check=True)
            subprocess.run(["git", "add", "."], cwd=repo, check=True)
            subprocess.run(["git", "commit", "-qm", "fixture"], cwd=repo, check=True)
            python_command = "python" if os.name == "nt" else "python3"
            result = subprocess.run(
                ["just", "--set", "python_cmd", python_command, "prerelease-tag", "--dry-run"],
                cwd=repo,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(f"workspace version: {old_version} -> {new_version}", result.stdout)
            self.assertIn(f"would create tag: prerelease/v{new_version}", result.stdout)
            self.assertEqual(
                subprocess.run(
                    ["git", "status", "--porcelain"],
                    cwd=repo,
                    text=True,
                    capture_output=True,
                    check=True,
                ).stdout,
                "",
            )


if __name__ == "__main__":
    unittest.main()
