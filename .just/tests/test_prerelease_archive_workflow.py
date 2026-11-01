"""ATM-specific prerelease manifest and tag-helper contract tests."""

from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
import shlex
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import unittest
from contextlib import contextmanager, redirect_stderr
from pathlib import Path
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
    def fixture_env(self, home: Path) -> dict[str, str]:
        env = os.environ.copy()
        env.update({
            "HOME": str(home),
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_SYSTEM": os.devnull,
            "GIT_CONFIG_NOSYSTEM": "1",
            # A fixture commit of the tracked tree exceeds gc.auto's loose-object
            # threshold; a detached `gc --auto` would keep writing .git while
            # TemporaryDirectory removes it.
            "GIT_CONFIG_COUNT": "2",
            "GIT_CONFIG_KEY_0": "gc.auto",
            "GIT_CONFIG_VALUE_0": "0",
            "GIT_CONFIG_KEY_1": "maintenance.auto",
            "GIT_CONFIG_VALUE_1": "false",
        })
        return env

    def run_fixture(self, command: list[str], *, cwd: Path, env: dict[str, str], **kwargs):
        return subprocess.run(command, cwd=cwd, env=env, **kwargs)

    def configure_fixture_git(self, repo: Path, env: dict[str, str]) -> None:
        for command in (
            ["git", "config", "gc.auto", "0"],
            ["git", "config", "gc.autoDetach", "false"],
            ["git", "config", "maintenance.auto", "false"],
            ["git", "config", "maintenance.autoDetach", "false"],
        ):
            self.run_fixture(command, cwd=repo, env=env, check=True)

    @contextmanager
    def fixture_repo(self, *, branch: str | None = None):
        """Create an isolated Git fixture without background maintenance."""
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            home = repo / ".home"
            home.mkdir()
            env = self.fixture_env(home)
            init = ["git", "init", "-q"]
            if branch is not None:
                init.extend(["-b", branch])
            for command in (
                init,
                ["git", "config", "commit.gpgsign", "false"],
                ["git", "config", "core.hooksPath", os.devnull],
                ["git", "config", "user.email", "test@example.invalid"],
                ["git", "config", "user.name", "AS1.1 test"],
            ):
                self.run_fixture(command, cwd=repo, env=env, check=True)
            self.configure_fixture_git(repo, env)
            yield repo, env

    def fixture_public_release_assets(
        self, prerelease, manifest: dict, version: str
    ) -> dict[str, bytes]:
        """Build an in-memory public GitHub-release response map."""
        target = {
            "target": "x86_64-unknown-linux-gnu",
            "archive": "tar.gz",
        }
        archive_name = prerelease.archive_name(manifest, version, target)
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode="w:gz") as bundle:
            for name in ("atm", "atm-daemon"):
                content = f"#!/bin/sh\necho '{name} {version}'\n".encode()
                member = tarfile.TarInfo(f"atm-{version}/bin/{name}")
                member.mode = 0o755
                member.size = len(content)
                bundle.addfile(member, io.BytesIO(content))
        assets = {archive_name: archive.getvalue()}
        for prefix in prerelease.required_wheel_prefixes(manifest):
            name = f"{prefix}{version}-py3-none-any.whl"
            assets[name] = f"fixture wheel {name}\n".encode()
        checksums = "".join(
            f"{hashlib.sha256(content).hexdigest()}  {name}\n"
            for name, content in sorted(assets.items())
        )
        assets["checksums.txt"] = checksums.encode()
        return assets

    def fixture_public_urlopen(self, assets: dict[str, bytes], public_url: str):
        """Return a deterministic `urlopen` fake for versioned public assets."""
        requested: list[str] = []

        def urlopen_fixture(url: str, *, timeout: int):
            self.assertEqual(timeout, 30)
            self.assertTrue(url.startswith(f"{public_url}/"))
            name = url.removeprefix(f"{public_url}/")
            requested.append(name)
            return io.BytesIO(assets[name])

        return urlopen_fixture, requested

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

    def test_wait_for_archive_raises_after_sixty_minutes_without_real_sleep(self) -> None:
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
                "timed out waiting for prerelease-archive.yml for prerelease/v1.5.17 after 60.0 minutes",
            ):
                prerelease.wait_for_archive("prerelease/v1.5.17", "source-sha")

        self.assertEqual(clock, [60 * 60])
        self.assertEqual(sleeps, [60] * 60)

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
        self.assertIn(
            "{python} .claude/skills/daemon-switch/scripts/daemon-switch.py switch "
            "--prerelease {version} --yes --cli-link {cli_link} --daemon-link {daemon_link} "
            "--discover-managed-service --allow-missing-managed-service",
            prerelease["post_install"],
        )
        self.assertTrue(prerelease["selector_dir"]["darwin"].startswith(prerelease["install_root"]))
        self.assertTrue(prerelease["selector_dir"]["linux"].startswith(prerelease["install_root"]))
        self.assertNotIn("Programs\\\\ATM", prerelease["selector_dir"]["windows"])

    def test_prerelease_requires_python_311_with_typed_remediation(self) -> None:
        prerelease = prerelease_script()
        with mock.patch.object(prerelease.sys, "version_info", (3, 9, 18)):
            with self.assertRaisesRegex(
                SystemExit,
                r"PRERELEASE\.PYTHON_VERSION_UNSUPPORTED: Python 3\.11 or newer.*"
                r"fix: install Python 3\.11\+ and run prerelease\.py with that interpreter",
            ):
                prerelease.require_supported_python()

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
            for folder in (old_bin, path_bin):
                folder.mkdir(parents=True)
            for folder, version in ((old_bin, "1.5.10"),):
                for name in ("atm", "atm-daemon"):
                    binary = folder / name
                    binary.write_text(f"#!/bin/sh\necho '{name} {version}'\n", encoding="utf-8")
                    binary.chmod(0o755)
            active_cli = path_bin / "atm"
            active_daemon = path_bin / "atm-daemon"
            active_cli.symlink_to(old_bin / "atm")
            active_daemon.symlink_to(old_bin / "atm-daemon")
            manifest = tomllib.loads(
                (root / "release" / "publish-artifacts.toml").read_text(encoding="utf-8")
            )
            config = dict(manifest["prerelease"])
            manifest["prerelease"] = config
            config.update({
                "install_root": str(fixture / "builds"),
                "selector_dir": {
                    "darwin": str(private_bin),
                    "linux": str(private_bin),
                    "windows": str(private_bin),
                },
            })
            public_url = "https://github.com/randlee/atm-core/releases/download/prerelease/v1.5.11"
            assets = self.fixture_public_release_assets(prerelease, manifest, "1.5.11")
            urlopen_fixture, requested_assets = self.fixture_public_urlopen(assets, public_url)
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
                    f"{shlex.quote(prerelease.sys.executable)} "
                    ".claude/skills/daemon-switch/scripts/daemon-switch.py "
                    f"switch --prerelease 1.5.11 --yes --cli-link {shlex.quote(str(private_bin / 'atm'))} "
                    f"--daemon-link {shlex.quote(str(private_bin / 'atm-daemon'))} "
                    "--discover-managed-service --allow-missing-managed-service",
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
                        "--cli-link",
                        str(private_bin / "atm"),
                        "--daemon-link",
                        str(private_bin / "atm-daemon"),
                        "--discover-managed-service",
                        "--allow-missing-managed-service",
                    ],
                ):
                    self.assertEqual(daemon_switch.main(), 0)
                return subprocess.CompletedProcess([], 0, "", "")

            with (
                mock.patch.object(prerelease.platform, "system", return_value="Linux"),
                mock.patch.object(prerelease.platform, "machine", return_value="x86_64"),
                mock.patch.object(prerelease, "urlopen", side_effect=urlopen_fixture),
                mock.patch.object(prerelease, "shell", side_effect=run_manifest_command),
                mock.patch.dict(os.environ, {"PATH": f"{path_bin}{os.pathsep}{os.environ['PATH']}"}),
                mock.patch.object(
                    daemon_switch,
                    "resolve_prerelease_pair",
                    return_value=(candidate_bin / "atm", candidate_bin / "atm-daemon", "1.5.11"),
                ) as resolve,
                mock.patch.object(daemon_switch, "sign_prerelease_pair") as sign,
                mock.patch.object(daemon_switch, "require_no_active_temporary_launch_session"),
                mock.patch.object(
                    daemon_switch._service_control_module,
                    "_linux_candidates",
                    return_value=[],
                ),
                mock.patch.object(daemon_switch, "save_default_pair"),
                mock.patch.object(daemon_switch, "run_service") as service,
                mock.patch.object(daemon_switch, "require_stopped_daemon") as stopped,
                mock.patch.object(daemon_switch, "require_macos_development_signatures"),
                mock.patch.object(
                    daemon_switch, "wait_for_live_pair", return_value=(True, "matched")
                ) as live_proof,
                mock.patch("sys.stdout", new_callable=io.StringIO) as stdout,
            ):
                version, installed = prerelease.install(manifest, "1.5.11")

            self.assertEqual((version, installed), ("1.5.11", stage))
            self.assertEqual((private_bin / "atm").resolve(), (candidate_bin / "atm").resolve())
            self.assertEqual(
                (private_bin / "atm-daemon").resolve(), (candidate_bin / "atm-daemon").resolve()
            )
            self.assertEqual(active_cli.resolve(), (old_bin / "atm").resolve())
            self.assertEqual(active_daemon.resolve(), (old_bin / "atm-daemon").resolve())
            resolve.assert_called_once_with("1.5.11")
            sign.assert_called_once_with(candidate_bin / "atm", candidate_bin / "atm-daemon")
            service.assert_not_called()
            stopped.assert_not_called()
            live_proof.assert_not_called()
            self.assertIn("no managed service was found or restarted", stdout.getvalue())
            self.assertIn(f"run: {private_bin / 'atm-daemon'}", stdout.getvalue())
            self.assertEqual(calls, [f"{shlex.quote(str(private_bin / 'atm'))} --version"])
            self.assertEqual(set(requested_assets), set(assets))
            self.assertNotIn("already selected; service left running", stdout.getvalue())

    @unittest.skipUnless(os.name == "posix", "selector composition uses POSIX symlinks")
    def test_failed_activation_keeps_non_transactional_selector_repoint(self) -> None:
        """Consumer contract for sc-publish#112: failed activation keeps repoints."""
        prerelease = prerelease_script()
        manifest_root = discover_repo_root()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stage = root / "builds" / "v1.5.11"
            selector = root / "selectors"
            manifest = tomllib.loads(
                (manifest_root / "release" / "publish-artifacts.toml").read_text(encoding="utf-8")
            )
            config = dict(manifest["prerelease"])
            manifest["prerelease"] = config
            config.update({
                "install_root": str(root / "builds"),
                "selector_dir": {"darwin": str(selector), "linux": str(selector), "windows": str(selector)},
                "post_install": "activate {version}",
                "verify": "verify {version}",
            })
            public_url = "https://github.com/randlee/atm-core/releases/download/prerelease/v1.5.11"
            assets = self.fixture_public_release_assets(prerelease, manifest, "1.5.11")
            urlopen_fixture, requested_assets = self.fixture_public_urlopen(assets, public_url)
            with (
                mock.patch.object(prerelease.platform, "system", return_value="Linux"),
                mock.patch.object(prerelease.platform, "machine", return_value="x86_64"),
                mock.patch.object(prerelease, "urlopen", side_effect=urlopen_fixture),
                mock.patch.object(
                    prerelease, "shell", side_effect=subprocess.CalledProcessError(1, "activate")
                ),
            ):
                with self.assertRaises(subprocess.CalledProcessError):
                    prerelease.install(manifest, "1.5.11")
            self.assertTrue((selector / "atm").is_symlink())
            self.assertTrue(os.path.samefile(selector / "atm", stage / "bin" / "atm"))
            self.assertEqual(set(requested_assets), set(assets))

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
        self.assertIn('shasum -a 256 "${assets_to_publish[@]}" > checksums.txt', workflow)
        self.assertIn('"${assets_to_publish[@]}" checksums.txt', workflow)
        self.assertIn("uses: ./.github/workflows/ci.yml", workflow)
        self.assertIn("build_release_artifacts: true", workflow)
        self.assertIn('gh release create "$tag" --prerelease', workflow)
        self.assertIn('cmp checksums.txt existing-release/checksums.txt', workflow)
        self.assertIn("concurrent run converged", workflow)
        self.assertNotIn('gh release upload "$tag" --clobber', workflow)
        self.assertNotIn("randlee/atm-core", workflow)
        release_step = workflow.split(
            "      - name: Generate checksums and publish GitHub prerelease assets\n", 1
        )[1]
        self.assertRegex(release_step, r"find assets/hermes-atm-wheels-\*.*atm_graft\*\.whl")
        self.assertRegex(release_step, r"find assets/hermes-atm-wheels-\*.*atm_query\*\.whl")
        self.assertRegex(release_step, r"find assets/hermes-atm-wheels-linux-x86_64.*hermes_atm\*\.whl")
        self.assertNotRegex(release_step, r"assets/hermes-atm-wheels-linux-x86_64.*atm_query")

    def test_checksums_are_an_explicit_github_release_asset(self) -> None:
        workflow = workflow_text("prerelease-archive.yml")
        release_step = workflow.split(
            "      - name: Generate checksums and publish GitHub prerelease assets\n", 1
        )[1]
        release_job = workflow.split("  release:\n", 1)[1]
        self.assertIn("- uses: actions/checkout@v4", release_job)
        self.assertIn('shasum -a 256 "${assets_to_publish[@]}" > checksums.txt', release_step)
        self.assertIn(
            'gh release create "$tag" --prerelease --title "$tag" --generate-notes '
            '"${assets_to_publish[@]}" checksums.txt',
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
        with self.fixture_repo(branch="fixture") as (repo, _env):
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
        original = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
        self.assertEqual(
            [package for package in lock["package"] if "source" in package],
            [package for package in original["package"] if "source" in package],
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
            root / "crates/atm/tests/cli_surface_baseline.json",
            root / "docs/user-documents/cli-reference.md",
            root / "site/cli/index.html",
            root / "site/cli/cli-reference.css",
            root / "site/cli/cli-reference.js",
        }
        self.assertEqual(set(changes), expected)
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
        with self.fixture_repo() as (repo, env):
            manifest = repo / "Cargo.toml"
            # Fixture-local versions: this test exercises rollback after a failed commit.
            original = "[workspace.package]\nversion = \"1.4.5\"\n"
            manifest.write_text(original, encoding="utf-8")
            self.run_fixture(["git", "add", "Cargo.toml"], cwd=repo, env=env, check=True)
            self.run_fixture(["git", "commit", "-qm", "fixture"], cwd=repo, env=env, check=True)

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
                self.run_fixture(
                    ["git", "status", "--porcelain"],
                    cwd=repo,
                    env=env,
                    text=True,
                    capture_output=True,
                    check=True,
                ).stdout,
                "",
            )

    def test_fixture_env_disables_background_git_gc(self) -> None:
        with self.fixture_repo() as (repo, env):
            for key, expected in (("gc.auto", "0"), ("maintenance.auto", "false")):
                value = self.run_fixture(
                    ["git", "config", "--get", key],
                    cwd=repo,
                    env=env,
                    text=True,
                    capture_output=True,
                    check=True,
                ).stdout.strip()
                self.assertEqual(value, expected, key)

    def test_fixture_command_has_no_host_load_deadline(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            env = self.fixture_env(repo)
            with mock.patch.object(subprocess, "run") as run:
                self.run_fixture(["git", "init"], cwd=repo, env=env, check=True)
            self.assertNotIn("timeout", run.call_args.kwargs)
            self.assertTrue(run.call_args.kwargs["check"])

    def test_just_prerelease_tag_dry_run_is_end_to_end_and_clean(self) -> None:
        root = discover_repo_root()
        old_version = workspace_version(root)
        new_version = patch_bump(old_version)
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory) / "atm-core"
            home = Path(directory) / ".home"
            home.mkdir()
            env = self.fixture_env(home)

            prerelease_tag.copy_tracked_files(root, repo)
            managed_justfile = root / ".sc-lint" / "justfile"
            self.assertTrue(
                managed_justfile.is_file(),
                "just setup must generate .sc-lint/justfile before the prerelease fixture is copied",
            )
            fixture_managed_justfile = repo / ".sc-lint" / "justfile"
            fixture_managed_justfile.parent.mkdir(parents=True, exist_ok=True)
            fixture_managed_justfile.write_bytes(managed_justfile.read_bytes())
            for command in (
                ["git", "init", "-q", "-b", "fixture"],
                ["git", "config", "commit.gpgsign", "false"],
                ["git", "config", "core.hooksPath", os.devnull],
                ["git", "config", "user.email", "test@example.invalid"],
                ["git", "config", "user.name", "AS1.1 test"],
            ):
                self.run_fixture(command, cwd=repo, env=env, check=True)
            self.configure_fixture_git(repo, env)
            for command in (["git", "add", "."], ["git", "commit", "-qm", "fixture"]):
                self.run_fixture(command, cwd=repo, env=env, check=True)
            python_command = "python" if os.name == "nt" else "python3"
            result = self.run_fixture(
                ["just", "--set", "python_cmd", python_command, "prerelease-tag", "--dry-run"],
                cwd=repo,
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(f"workspace version: {old_version} -> {new_version}", result.stdout)
            self.assertIn(f"would create tag: prerelease/v{new_version}", result.stdout)
            self.assertEqual(
                self.run_fixture(
                    ["git", "status", "--porcelain"],
                    cwd=repo,
                    env=env,
                    text=True,
                    capture_output=True,
                    check=True,
                ).stdout,
                "",
            )


if __name__ == "__main__":
    unittest.main()
