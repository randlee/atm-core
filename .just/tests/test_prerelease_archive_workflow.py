"""Tests for the atm-core-owned prerelease archive workflow (AR1.1)."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
import zipfile
from unittest import mock

JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_common import discover_repo_root
import prerelease_tag
from prerelease_tag import patch_bump


def scripts_root() -> Path:
    return discover_repo_root() / ".github" / "scripts"


def workflow_text(name: str) -> str:
    return (discover_repo_root() / ".github" / "workflows" / name).read_text(encoding="utf-8")


def extract_python_step(workflow: str, step_name: str, next_step_name: str) -> str:
    """Extract the ``python3 - <<'PY' ... PY`` body of one workflow step."""
    step = workflow.split(f"      - name: {step_name}\n", 1)[1].split(
        f"      - name: {next_step_name}\n", 1
    )[0]
    script = step.split("          python3 - <<'PY'\n", 1)[1].split("          PY\n", 1)[0]
    lines = script.splitlines()
    if not all(not line or line.startswith("          ") for line in lines):
        raise AssertionError("workflow Python block has unexpected indentation")
    return "\n".join(line[10:] if line else "" for line in lines)


def extract_shell_step(workflow: str, step_name: str, next_step_name: str) -> str:
    """Extract the shell body of one workflow step."""
    step = workflow.split(f"      - name: {step_name}\n", 1)[1].split(
        f"      - name: {next_step_name}\n", 1
    )[0]
    script = step.split("        run: |\n", 1)[1]
    lines = script.splitlines()
    if not all(not line or line.startswith("          ") for line in lines):
        raise AssertionError("workflow shell block has unexpected indentation")
    return "\n".join(line[10:] if line else "" for line in lines)


def release_archive_packager_python() -> str:
    return extract_python_step(
        workflow_text("release.yml"),
        "Package manifest-declared release archive",
        "Upload artifact",
    )


def prerelease_archive_packager_python() -> str:
    return extract_python_step(
        workflow_text("prerelease-archive.yml"),
        "Package manifest-declared pre-release archive",
        "Upload artifact",
    )


def run_prerelease_archive_packager(
    tmp_path: Path, *, target_name: str, expected_filename: str
) -> subprocess.CompletedProcess[str]:
    scripts_dir = tmp_path / ".github" / "scripts"
    scripts_dir.mkdir(parents=True)
    (scripts_dir / "release_artifacts.py").write_text(
        "import json\n"
        "print(json.dumps({\n"
        "    'project': {'archive_prefix': 'fixture'},\n"
        "    'target': {'archive': 'zip'},\n"
        "    'binaries': [{'name': 'fixture'}],\n"
        "}))\n",
        encoding="utf-8",
    )
    release_dir = tmp_path / "target" / target_name / "release"
    release_dir.mkdir(parents=True)
    (release_dir / expected_filename).write_text("fixture", encoding="utf-8")
    output = tmp_path / "github-env"
    script = prerelease_archive_packager_python().replace(
        'target_name = "${{ matrix.target }}"', f"target_name = {target_name!r}"
    ).replace(
        'version = "${{ needs.plan.outputs.version }}"', 'version = "1.5.0"'
    )
    result = subprocess.run(
        [sys.executable, "-c", script],
        cwd=tmp_path,
        env={
            **os.environ,
            "PATH": f"{_python3_shim(tmp_path)}{os.pathsep}{os.environ.get('PATH', '')}",
            "RELEASE_ARTIFACT_MANIFEST": str(tmp_path / "release" / "manifest.toml"),
            "GITHUB_ENV": str(output),
        },
        text=True,
        capture_output=True,
        check=False,
    )
    return result


def _python3_shim(tmp_path: Path) -> Path:
    """Put the interpreter behind the literal ``python3`` name used by the heredoc."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    shim_name = "python3.exe" if os.name == "nt" else "python3"
    shutil.copy2(sys.executable, bin_dir / shim_name)
    return bin_dir


class PrereleaseArchiveWorkflowTests(unittest.TestCase):
    def test_workflow_exists_and_does_not_edit_vendored_kit(self) -> None:
        root = discover_repo_root()
        self.assertTrue((root / ".github" / "workflows" / "prerelease-archive.yml").is_file())
        cli_text = (scripts_root() / "release_artifacts.py").read_text(encoding="utf-8")
        self.assertNotIn("package-archive", cli_text)

    def test_packaging_matches_release_yml_byte_for_byte(self) -> None:
        release_script = release_archive_packager_python()
        prerelease_script = prerelease_archive_packager_python()
        release_version_line = 'version = "${{ needs.gate-and-tag.outputs.release_version }}"'
        prerelease_version_line = 'version = "${{ needs.plan.outputs.version }}"'
        self.assertIn(release_version_line, release_script)
        self.assertIn(prerelease_version_line, prerelease_script)
        normalized_release = release_script.replace(release_version_line, 'version = "VERSION"')
        normalized_prerelease = prerelease_script.replace(
            prerelease_version_line, 'version = "VERSION"'
        )
        self.assertEqual(normalized_release, normalized_prerelease)

    def test_packager_executes_windows_suffix_logic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            tmp_path = Path(directory)
            result = run_prerelease_archive_packager(
                tmp_path, target_name="x86_64-pc-windows-msvc", expected_filename="fixture.exe"
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            archive = tmp_path / "fixture_1.5.0_x86_64-pc-windows-msvc.zip"
            with zipfile.ZipFile(archive) as packaged:
                self.assertEqual(
                    packaged.namelist(),
                    ["fixture_1.5.0_x86_64-pc-windows-msvc/bin/fixture.exe"],
                )

    def test_workflow_is_tag_only_and_validates_tag_version_and_release_absence(self) -> None:
        text = workflow_text("prerelease-archive.yml")
        self.assertIn('push:\n    tags:\n      - "prerelease/v*.*.*"', text)
        self.assertNotIn("workflow_dispatch", text)
        self.assertIn('tag="${GITHUB_REF_NAME}"', text)
        self.assertIn("expected prerelease/vX.Y.Z", text)
        self.assertIn("verify-version", text)
        self.assertIn("verify-version-lockstep", text)
        self.assertIn("releases/tags/${tag}", text)
        self.assertNotIn("merge-base", text)

    def test_plan_step_exercises_authenticated_release_probe(self) -> None:
        workflow = workflow_text("prerelease-archive.yml")
        self.assertIn(
            "        env:\n          GH_TOKEN: ${{ github.token }}\n        run: |", workflow
        )
        script = extract_shell_step(
            workflow,
            "Validate prerelease tag and workspace version",
            "Resolve release target matrix",
        )
        with tempfile.TemporaryDirectory() as directory:
            tmp_path = Path(directory)
            scripts_dir = tmp_path / ".github" / "scripts"
            scripts_dir.mkdir(parents=True)
            (scripts_dir / "release_artifacts.py").write_text(
                "import json\n"
                "import sys\n"
                "if sys.argv[1] == 'build-plan':\n"
                "    print(json.dumps({'workspace_toml': 'Cargo.toml', 'rust_toolchain': 'stable'}))\n",
                encoding="utf-8",
            )
            bin_dir = tmp_path / "bin"
            bin_dir.mkdir()
            (bin_dir / "jq").write_text(
                "#!/bin/sh\n"
                "case \"$2\" in\n"
                "  .workspace_toml) printf '%s\\n' Cargo.toml ;;\n"
                "  .rust_toolchain) printf '%s\\n' stable ;;\n"
                "esac\n",
                encoding="utf-8",
            )
            (bin_dir / "gh").write_text(
                "#!/bin/sh\n"
                "printf '%s' \"${GH_TOKEN-}\" > \"${GH_TOKEN_CAPTURE}\"\n"
                "printf '%s\\n' \"HTTP/2 ${GH_PROBE_STATUS}\"\n",
                encoding="utf-8",
            )
            for command in (bin_dir / "jq", bin_dir / "gh"):
                command.chmod(0o755)
            subprocess.run(["git", "init", "-q"], cwd=tmp_path, check=True)
            subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True)
            subprocess.run(["git", "config", "user.name", "AR1.1 test"], cwd=tmp_path, check=True)
            (tmp_path / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            subprocess.run(["git", "add", "Cargo.toml"], cwd=tmp_path, check=True)
            subprocess.run(["git", "commit", "-qm", "fixture"], cwd=tmp_path, check=True)
            output = tmp_path / "github-output"
            token_capture = tmp_path / "gh-token"
            probe_env = {
                **os.environ,
                "PATH": f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '')}",
                "GITHUB_REF_NAME": "prerelease/v1.4.6",
                "GITHUB_REPOSITORY": "randlee/atm-core",
                "GITHUB_OUTPUT": str(output),
                "RELEASE_ARTIFACT_MANIFEST": "release/publish-artifacts.toml",
                "GH_TOKEN": "workflow-token",
                "GH_TOKEN_CAPTURE": str(token_capture),
                "GH_PROBE_STATUS": "404",
            }
            result = subprocess.run(
                ["bash", "-euo", "pipefail", "-c", script],
                cwd=tmp_path,
                env=probe_env,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(token_capture.read_text(encoding="utf-8"), "workflow-token")
            self.assertIn("version=1.4.6", output.read_text(encoding="utf-8"))
            existing_output = tmp_path / "github-output-existing"
            probe_env["GH_PROBE_STATUS"] = "200"
            probe_env["GITHUB_OUTPUT"] = str(existing_output)
            existing = subprocess.run(
                ["bash", "-euo", "pipefail", "-c", script],
                cwd=tmp_path,
                env=probe_env,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertNotEqual(existing.returncode, 0)
            self.assertIn("GitHub Release already exists", existing.stderr)

    def test_workflow_uses_patch_bumped_stable_versions_without_short_sha_scheme(self) -> None:
        text = workflow_text("prerelease-archive.yml")
        self.assertNotIn("short_sha", text)
        self.assertNotIn("-pre.", text)
        self.assertIn('version="${BASH_REMATCH[1]}"', text)

    def test_workflow_never_tags_or_publishes_and_uses_read_permission(self) -> None:
        text = workflow_text("prerelease-archive.yml")
        self.assertIn("permissions:\n  contents: read", text)
        self.assertNotIn("git tag", text)
        self.assertNotIn("git push", text)
        self.assertNotIn("action-gh-release", text)
        self.assertNotIn("secrets.", text)

    def test_checksums_and_provenance_are_retained(self) -> None:
        text = workflow_text("prerelease-archive.yml")
        self.assertIn("name: checksums", text)
        self.assertIn("checksums.txt", text)
        self.assertIn("provenance.json", text)
        self.assertIn('"atm_core_sha"', text)
        self.assertIn('"run_id"', text)
        self.assertIn('checksum_lines.append(f"{digest}  {archive.name}")', text)

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
        self.assertEqual(patch_bump("1.4.5"), "1.4.6")
        self.assertEqual(patch_bump("9.99.0"), "9.99.1")

    def test_candidate_bump_updates_actual_lockfile_collision_safely(self) -> None:
        root = discover_repo_root()
        changes = prerelease_tag.candidate_changes(root, "1.4.5", "1.4.6")
        lock = tomllib.loads(changes[root / "Cargo.lock"])
        directives = [
            package for package in lock["package"] if package["name"] == "sc-lint-directives"
        ]
        self.assertEqual(
            [(package["version"], "source" in package) for package in directives],
            [("0.5.0", True), ("1.4.6", False)],
        )
        self.assertEqual(
            tomllib.loads(changes[root / "crates" / "atm-query-python" / "pyproject.toml"])[
                "project"
            ]["version"],
            "1.4.6",
        )

    def test_write_and_commit_restores_files_when_commit_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=repo, check=True)
            subprocess.run(["git", "config", "user.name", "AR1.1 test"], cwd=repo, check=True)
            manifest = repo / "Cargo.toml"
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
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory) / "atm-core"

            def ignore(_path: str, names: list[str]) -> set[str]:
                return {
                    name
                    for name in names
                    if name in {".git", ".bootstrap-venv", "target", "artifacts"}
                }

            shutil.copytree(root, repo, ignore=ignore)
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=repo, check=True)
            subprocess.run(["git", "config", "user.name", "AR1.1 test"], cwd=repo, check=True)
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
            self.assertIn("workspace version: 1.4.5 -> 1.4.6", result.stdout)
            self.assertIn("would create tag: prerelease/v1.4.6", result.stdout)
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
