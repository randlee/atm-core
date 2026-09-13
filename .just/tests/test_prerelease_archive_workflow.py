"""ATM-specific prerelease manifest and tag-helper contract tests."""

from __future__ import annotations

import importlib.util
import io
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


def load_script(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def workflow_text(name: str) -> str:
    return (discover_repo_root() / ".github" / "workflows" / name).read_text(encoding="utf-8")


def prerelease_script():
    return load_script(
        "atm_prerelease_wait_helper",
        discover_repo_root() / ".claude" / "skills" / "prerelease" / "scripts" / "prerelease.py",
    )


def packaging_script(workflow: str, step_name: str) -> str:
    """Extract the Python heredoc used to package one release archive."""
    step = workflow.split(f"      - name: {step_name}\n", 1)[1].split("\n      - name:", 1)[0]
    script = step.split("          python3 - <<'PY'\n", 1)[1].split("          PY\n", 1)[0]
    lines = script.splitlines()
    if not all(not line or line.startswith("          ") for line in lines):
        raise AssertionError("workflow Python block has unexpected indentation")
    return "\n".join(line[10:] if line else "" for line in lines)


class PrereleaseArchiveWorkflowTests(unittest.TestCase):
    def test_wait_for_archive_returns_after_a_successful_run(self) -> None:
        prerelease = prerelease_script()
        clock = [0.0]
        sleeps: list[float] = []

        def monotonic() -> float:
            return clock[0]

        def sleep(seconds: float) -> None:
            sleeps.append(seconds)
            clock[0] += seconds

        with (
            mock.patch.object(prerelease, "gh_json", side_effect=[[], [{
                "headSha": "source-sha",
                "status": "completed",
                "conclusion": "success",
            }]]) as gh_json,
            mock.patch.object(prerelease.time, "monotonic", side_effect=monotonic),
            mock.patch.object(prerelease.time, "sleep", side_effect=sleep),
            mock.patch("sys.stdout", new_callable=io.StringIO) as stdout,
        ):
            prerelease.wait_for_archive("prerelease/v1.5.17", "source-sha")

        self.assertEqual(gh_json.call_count, 2)
        self.assertEqual(sleeps, [60])
        self.assertEqual(
            stdout.getvalue().splitlines(),
            [
                "waiting for prerelease-archive.yml (0.0 min elapsed)",
                "waiting for prerelease-archive.yml (1.0 min elapsed)",
            ],
        )

    def test_wait_for_archive_raises_for_a_failed_run(self) -> None:
        prerelease = prerelease_script()

        with (
            mock.patch.object(prerelease, "gh_json", return_value=[{
                "headSha": "source-sha",
                "status": "completed",
                "conclusion": "failure",
            }]),
            mock.patch.object(prerelease.time, "monotonic", return_value=0.0),
            mock.patch.object(prerelease.time, "sleep") as sleep,
        ):
            with self.assertRaisesRegex(SystemExit, "prerelease-archive.yml failed for prerelease/v1.5.17"):
                prerelease.wait_for_archive("prerelease/v1.5.17", "source-sha")

        sleep.assert_not_called()

    def test_wait_for_archive_raises_after_twenty_minutes_without_real_sleep(self) -> None:
        prerelease = prerelease_script()
        clock = [0.0]
        sleeps: list[float] = []

        def monotonic() -> float:
            return clock[0]

        def sleep(seconds: float) -> None:
            sleeps.append(seconds)
            clock[0] += seconds

        with (
            mock.patch.object(prerelease, "gh_json", return_value=[]),
            mock.patch.object(prerelease.time, "monotonic", side_effect=monotonic),
            mock.patch.object(prerelease.time, "sleep", side_effect=sleep),
        ):
            with self.assertRaisesRegex(
                SystemExit,
                "timed out waiting for prerelease-archive.yml for prerelease/v1.5.17 after 20.0 minutes",
            ):
                prerelease.wait_for_archive("prerelease/v1.5.17", "source-sha")

        self.assertEqual(clock, [20 * 60])
        self.assertEqual(sleeps, [60] * 20)

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

    @unittest.skipUnless(os.name == "posix", "selector composition uses POSIX symlinks")
    def test_prerelease_install_activates_only_through_daemon_switch(self) -> None:
        root = discover_repo_root()
        prerelease = load_script(
            "atm_prerelease_skill",
            root / ".claude" / "skills" / "prerelease" / "scripts" / "prerelease.py",
        )
        daemon_switch = load_script(
            "atm_daemon_switch_composition",
            root / ".claude" / "skills" / "daemon-switch" / "scripts" / "daemon-switch.py",
        )
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory)
            old_bin = fixture / "old" / "bin"
            stage = fixture / "builds" / "v1.5.11"
            candidate_bin = stage / "bin"
            path_bin = fixture / "path"
            private_bin = fixture / "private-selectors"
            for folder in (old_bin, candidate_bin, path_bin):
                folder.mkdir(parents=True)
            for folder, version in ((old_bin, "1.5.10"), (candidate_bin, "1.5.11")):
                for name in ("atm", "atm-daemon"):
                    binary = folder / name
                    binary.write_text(f"#!/bin/sh\necho '{name} {version}'\n", encoding="utf-8")
                    binary.chmod(0o755)
            active_cli = path_bin / "atm"
            active_daemon = path_bin / "atm-daemon"
            active_cli.symlink_to(old_bin / "atm")
            active_daemon.symlink_to(old_bin / "atm-daemon")
            config = dict(
                tomllib.loads(
                    (root / "release" / "publish-artifacts.toml").read_text(encoding="utf-8")
                )["prerelease"]
            )
            config.update({
                "install_root": str(fixture / "builds"),
                "selector_dir": {
                    "darwin": str(private_bin),
                    "linux": str(private_bin),
                    "windows": str(private_bin),
                },
            })
            calls: list[str] = []

            def run_manifest_command(command_text: str, *, capture: bool = False):
                if capture:
                    calls.append(command_text)
                    return subprocess.run(
                        command_text,
                        shell=True,
                        check=True,
                        text=True,
                        capture_output=True,
                    )
                self.assertEqual(
                    command_text,
                    "python3 .claude/skills/daemon-switch/scripts/daemon-switch.py "
                    "switch --prerelease 1.5.11 --yes",
                )
                self.assertEqual(active_cli.resolve(), (old_bin / "atm").resolve())
                self.assertEqual(active_daemon.resolve(), (old_bin / "atm-daemon").resolve())
                self.assertEqual((private_bin / "atm").resolve(), (candidate_bin / "atm").resolve())
                self.assertEqual(
                    (private_bin / "atm-daemon").resolve(),
                    (candidate_bin / "atm-daemon").resolve(),
                )
                with mock.patch.object(
                    daemon_switch.sys,
                    "argv",
                    [
                        "daemon-switch.py",
                        "switch",
                        "--prerelease",
                        "1.5.11",
                        "--yes",
                        "--service",
                        "fixture",
                    ],
                ):
                    self.assertEqual(daemon_switch.main(), 0)
                return subprocess.CompletedProcess([], 0, "", "")

            with (
                mock.patch.object(prerelease, "select_release", return_value=("1.5.11", {})),
                mock.patch.object(prerelease.platform, "system", return_value="Linux"),
                mock.patch.object(prerelease, "shell", side_effect=run_manifest_command),
                mock.patch.dict(os.environ, {"PATH": f"{path_bin}{os.pathsep}{os.environ['PATH']}"}),
                mock.patch.object(
                    daemon_switch,
                    "resolve_prerelease_pair",
                    return_value=(candidate_bin / "atm", candidate_bin / "atm-daemon", "1.5.11"),
                ) as resolve,
                mock.patch.object(daemon_switch, "sign_prerelease_pair") as sign,
                mock.patch.object(daemon_switch, "require_no_active_temporary_launch_session"),
                mock.patch.object(daemon_switch, "save_default_pair"),
                mock.patch.object(daemon_switch, "run_service") as service,
                mock.patch.object(daemon_switch, "require_stopped_daemon") as stopped,
                mock.patch.object(daemon_switch, "require_macos_development_signatures"),
                mock.patch.object(
                    daemon_switch, "wait_for_live_pair", return_value=(True, "matched")
                ) as live_proof,
                mock.patch("sys.stdout", new_callable=io.StringIO) as stdout,
            ):
                version, installed = prerelease.install({"prerelease": config}, "1.5.11")

            self.assertEqual((version, installed), ("1.5.11", stage))
            self.assertEqual((private_bin / "atm").resolve(), (candidate_bin / "atm").resolve())
            self.assertEqual(
                (private_bin / "atm-daemon").resolve(), (candidate_bin / "atm-daemon").resolve()
            )
            self.assertEqual(active_cli.resolve(), (candidate_bin / "atm").resolve())
            self.assertEqual(active_daemon.resolve(), (candidate_bin / "atm-daemon").resolve())
            resolve.assert_called_once_with("1.5.11")
            sign.assert_called_once_with(candidate_bin / "atm", candidate_bin / "atm-daemon")
            self.assertEqual([call.args[1] for call in service.call_args_list], ["stop", "start"])
            stopped.assert_called_once()
            self.assertEqual(stopped.call_args.args[1], (old_bin / "atm").resolve())
            live_proof.assert_called_once_with(
                (candidate_bin / "atm").resolve(), (candidate_bin / "atm-daemon").resolve()
            )
            self.assertEqual(calls, ["atm --version"])
            self.assertNotIn("already selected; service left running", stdout.getvalue())

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

    def test_packaging_matches_release_workflow_byte_for_byte(self) -> None:
        release_script = packaging_script(
            workflow_text("release.yml"), "Package manifest-declared release archive"
        )
        prerelease_script = packaging_script(
            workflow_text("prerelease-archive.yml"),
            "Package manifest-declared prerelease archive",
        )
        release_version = 'version = "${{ needs.gate-and-tag.outputs.release_version }}"'
        prerelease_version = 'version = "${{ needs.plan.outputs.version }}"'
        self.assertIn(release_version, release_script)
        self.assertIn(prerelease_version, prerelease_script)
        self.assertEqual(
            release_script.replace(release_version, 'version = "VERSION"'),
            prerelease_script.replace(prerelease_version, 'version = "VERSION"'),
        )

    def test_checksums_are_an_explicit_github_release_asset(self) -> None:
        workflow = workflow_text("prerelease-archive.yml")
        release_step = workflow.split(
            "      - name: Generate checksums and publish GitHub prerelease assets\n", 1
        )[1]
        release_job = workflow.split("  release:\n", 1)[1]
        self.assertIn("- uses: actions/checkout@v4", release_job)
        self.assertIn('shasum -a 256 "${archives[@]}" > checksums.txt', release_step)
        self.assertIn(
            'gh release create "$tag" --prerelease --title "$tag" --generate-notes '
            '"${archives[@]}" checksums.txt',
            release_step,
        )
        self.assertNotIn("provenance.json", workflow)

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

    def test_prerelease_tag_dry_run_rejects_remote_tag_collision(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            subprocess.run(["git", "init", "-q", "-b", "fixture"], cwd=repo, check=True)
            with (
                mock.patch.object(prerelease_tag, "current_branch", return_value="fixture"),
                mock.patch.object(prerelease_tag, "require_clean_tree"),
                mock.patch.object(prerelease_tag, "workspace_version", return_value="1.5.14"),
                mock.patch.object(prerelease_tag, "remote_tag_exists", return_value=True),
                mock.patch.object(prerelease_tag, "verify_lockstep") as verify_lockstep,
            ):
                with self.assertRaisesRegex(SystemExit, "tag already exists on origin"):
                    prerelease_tag.execute(repo, dry_run=True)
            verify_lockstep.assert_not_called()

    def test_publish_and_dry_run_share_tag_availability_preflight(self) -> None:
        root = discover_repo_root()
        for dry_run in (True, False):
            with (
                self.subTest(dry_run=dry_run),
                mock.patch.object(prerelease_tag, "current_branch", return_value="fixture"),
                mock.patch.object(prerelease_tag, "require_clean_tree"),
                mock.patch.object(prerelease_tag, "workspace_version", return_value="1.5.14"),
                mock.patch.object(
                    prerelease_tag,
                    "require_available_tag",
                    side_effect=SystemExit("collision"),
                ) as require_available_tag,
                mock.patch.object(prerelease_tag, "verify_lockstep") as verify_lockstep,
                mock.patch.object(prerelease_tag, "candidate_changes") as candidate_changes,
            ):
                with self.assertRaisesRegex(SystemExit, "collision"):
                    prerelease_tag.execute(root, dry_run=dry_run)
            require_available_tag.assert_called_once_with(root, "prerelease/v1.5.15")
            verify_lockstep.assert_not_called()
            candidate_changes.assert_not_called()

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
