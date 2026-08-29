"""Tests for the atm-core-owned `prerelease-archive.yml` workflow (AR1.1).

This file is deliberately separate from the vendored sc-publish kit test
suite (`test_release_artifacts.py`), which README.sc-publish.md documents as
installed byte-for-byte and never hand-edited. `prerelease-archive.yml`
itself is a repo-local addition — consumers may add their own workflows —
so its coverage lives here instead of inside the vendored file.

The central risk this file guards against: `prerelease-archive.yml`
duplicates (rather than edits) the packaging step already vendored in
`release.yml` ("Package manifest-declared release archive"), specifically
so `release.yml` and `release_artifacts.py` stay unmodified and pinned.
`test_prerelease_archive_packaging_matches_release_yml_byte_for_byte` proves
that duplication has not drifted from the vendored original.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import zipfile
from pathlib import Path


def repo_root() -> Path:
    # Mirrors test_release_artifacts.py's repo_root(): keep this relative to
    # the consumer root so it works regardless of checkout location.
    return next(
        path for path in Path(__file__).resolve().parents if (path / "install.py").is_file()
    )


def scripts_root() -> Path:
    return repo_root() / ".github" / "scripts"


def workflow_text(name: str) -> str:
    return (repo_root() / ".github" / "workflows" / name).read_text(encoding="utf-8")


def extract_python_step(workflow: str, step_name: str, next_step_name: str) -> str:
    """Extract the `python3 - <<'PY' ... PY` body of one workflow step."""
    step = workflow.split(f"      - name: {step_name}\n", 1)[1].split(
        f"      - name: {next_step_name}\n", 1
    )[0]
    script = step.split("          python3 - <<'PY'\n", 1)[1].split("          PY\n", 1)[0]
    lines = script.splitlines()
    assert all(not line or line.startswith("          ") for line in lines)
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
    script = (
        prerelease_archive_packager_python()
        .replace('target_name = "${{ matrix.target }}"', f"target_name = {target_name!r}")
        .replace(
            'version = "${{ needs.plan.outputs.version }}"',
            'version = "1.5.0-pre.abc123def"',
        )
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
    assert output.read_text(encoding="utf-8").startswith("ARCHIVE=fixture_1.5.0-pre.abc123def_")
    return result


def test_prerelease_archive_workflow_exists_and_is_not_a_vendored_kit_file() -> None:
    assert (repo_root() / ".github" / "workflows" / "prerelease-archive.yml").is_file()
    # release_artifacts.py stays untouched by this sprint: still the
    # unmodified vendored kit script, with no new "package-archive"
    # subcommand added.
    cli_text = (scripts_root() / "release_artifacts.py").read_text(encoding="utf-8")
    assert "package-archive" not in cli_text


def test_prerelease_archive_packaging_matches_release_yml_byte_for_byte() -> None:
    """The duplicated packaging step must differ from release.yml's only in
    which job output supplies the version string — everything else,
    including archive layout and Windows `.exe` suffix logic, must be
    identical so the two stay provably behaviour-equivalent."""
    release_script = release_archive_packager_python()
    prerelease_script = prerelease_archive_packager_python()

    release_version_line = 'version = "${{ needs.gate-and-tag.outputs.release_version }}"'
    prerelease_version_line = 'version = "${{ needs.plan.outputs.version }}"'
    assert release_version_line in release_script
    assert prerelease_version_line in prerelease_script

    normalized_release = release_script.replace(release_version_line, 'version = "VERSION"')
    normalized_prerelease = prerelease_script.replace(
        prerelease_version_line, 'version = "VERSION"'
    )
    assert normalized_release == normalized_prerelease


def test_prerelease_archive_packager_executes_windows_suffix_logic(tmp_path: Path) -> None:
    result = run_prerelease_archive_packager(
        tmp_path, target_name="x86_64-pc-windows-msvc", expected_filename="fixture.exe"
    )
    assert result.returncode == 0, result.stderr
    archive = tmp_path / "fixture_1.5.0-pre.abc123def_x86_64-pc-windows-msvc.zip"
    with zipfile.ZipFile(archive) as packaged:
        assert packaged.namelist() == [
            "fixture_1.5.0-pre.abc123def_x86_64-pc-windows-msvc/bin/fixture.exe"
        ]


def test_prerelease_archive_workflow_never_tags_publishes_or_writes() -> None:
    text = workflow_text("prerelease-archive.yml")
    assert "permissions:\n  contents: read" in text
    assert "git tag" not in text
    assert "git push" not in text
    assert "action-gh-release" not in text
    assert "secrets." not in text


def test_prerelease_archive_workflow_defaults_to_every_manifest_target() -> None:
    text = workflow_text("prerelease-archive.yml")
    # Resolved in resolve_matrix.py-equivalent: an empty targets input keeps
    # the full manifest-declared release-target-matrix (all four targets
    # today, including both macOS targets — not opt-in).
    assert "if not requested:" in text
    assert 'print(json.dumps(full, separators=(",", ":")))' in text


def test_prerelease_archive_workflow_rejects_tag_style_version() -> None:
    text = workflow_text("prerelease-archive.yml")
    assert 'version="${base_version}-pre.${short_sha}"' in text
    assert "is tag-style; refusing" in text


def test_prerelease_archive_workflow_writes_checksums_and_provenance() -> None:
    text = workflow_text("prerelease-archive.yml")
    assert 'name: checksums' in text
    assert "checksums.txt" in text
    assert "provenance.json" in text
    assert '"atm_core_sha"' in text
    assert '"run_id"' in text


def test_prerelease_archive_checksums_use_the_same_line_format_as_release_yml() -> None:
    """Both release.yml's `sha256sum` output and this workflow's hashlib
    output are `<sha256>  <filename>` (two spaces), the format
    `shasum -a 256 -c` verifies — matching loki's docker-testbed build.sh
    verification step."""
    release_text = workflow_text("release.yml")
    prerelease_text = workflow_text("prerelease-archive.yml")
    assert "sha256sum" in release_text
    assert 'checksum_lines.append(f"{digest}  {archive.name}")' in prerelease_text
