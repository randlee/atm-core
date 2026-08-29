"""Tests for the atm-core-owned prerelease archive workflow (AR1.1)."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile

JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_common import discover_repo_root
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
            "RELEASE_ARTIFACT_MANIFEST": str(tmp_path / "release" / "manifest.toml"),
            "GITHUB_ENV": str(output),
        },
        text=True,
        capture_output=True,
        check=False,
    )
    return result


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


if __name__ == "__main__":
    unittest.main()
