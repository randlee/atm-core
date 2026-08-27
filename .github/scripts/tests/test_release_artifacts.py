from __future__ import annotations

import io
import json
import subprocess
import sys
import tarfile
import tomllib
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

import pytest


def write_repo_fixture(
    tmp_path: Path,
    *,
    manifest_wheels: list[str],
    include_crates: bool = True,
    include_python: bool = True,
    python_build_system: str = "maturin",
) -> tuple[Path, Path]:
    workspace = tmp_path / "Cargo.toml"
    workspace.write_text(
        "\n".join(
            [
                "[workspace]",
                'members = ["crates/sc-composer", "crates/sc-compose"]',
                "",
                "[workspace.package]",
                'version = "1.1.0"',
                "",
            ]
        ),
        encoding="utf-8",
    )

    for crate_name in ("sc-composer", "sc-compose"):
        crate_dir = tmp_path / "crates" / crate_name
        crate_dir.mkdir(parents=True)
        (crate_dir / "Cargo.toml").write_text(
            "\n".join(
                [
                    "[package]",
                    f'name = "{crate_name}"',
                    'version = "1.1.0"',
                    "",
                ]
            ),
            encoding="utf-8",
        )

    bindings_dir = tmp_path / "bindings" / "python" / "python" / "sc_compose"
    bindings_dir.mkdir(parents=True)
    (bindings_dir / "__init__.py").write_text("", encoding="utf-8")
    (tmp_path / "bindings" / "python" / "pyproject.toml").write_text(
        "\n".join(
            [
                "[project]",
                'name = "sc-compose"',
                'version = "1.1.0"',
                "",
            ]
        ),
        encoding="utf-8",
    )
    (tmp_path / "bindings" / "python" / "Cargo.toml").write_text(
        "[package]\nname = \"sc-compose-python\"\nversion = \"1.1.0\"\n",
        encoding="utf-8",
    )

    manifest = tmp_path / "release" / "publish-artifacts.toml"
    manifest.parent.mkdir(parents=True)
    (manifest.parent / "publish-channel-contracts.toml").write_text(
        (repo_root() / "release" / "publish-channel-contracts.toml.j2").read_text(
            encoding="utf-8"
        ),
        encoding="utf-8",
    )
    wheels = ", ".join(f'"{entry}"' for entry in manifest_wheels)
    crates_section = [
        "[[crates]]",
        'artifact = "sc-composer"',
        'package = "sc-composer"',
        'cargo_toml = "crates/sc-composer/Cargo.toml"',
        "publish_order = 1",
        "wait_after_publish_seconds = 0",
        "",
        "[[crates]]",
        'artifact = "sc-compose"',
        'package = "sc-compose"',
        'cargo_toml = "crates/sc-compose/Cargo.toml"',
        "publish_order = 2",
        "wait_after_publish_seconds = 0",
        "",
    ]
    build_system_lines = {
        "maturin": ['cargo_manifest = "bindings/python/Cargo.toml"'],
        "setuptools": ['build_system = "setuptools"'],
        "unsupported": ['build_system = "flit"'],
        "missing": [],
    }[python_build_system]
    python_section = [
        "[[python_packages]]",
        'artifact = "sc-compose-python"',
        'package = "sc-compose"',
        'manifest = "bindings/python/pyproject.toml"',
        'module = "sc_compose"',
        'publish = "pypi"',
        "",
        "[[python_distributions]]",
        'name = "sc-compose"',
        'source = "bindings/python"',
        *build_system_lines,
        "sdist = true",
        f"wheels = [{wheels}]",
        "",
    ]
    manifest.write_text(
        "\n".join(
            [
                "schema_version = 1",
                "",
                "[project]",
                'name = "fixture"',
                'archive_prefix = "fixture"',
                'description = "Fixture release"',
                'homepage = "https://example.invalid/fixture"',
                'license = "MIT"',
                'readme_dependency_crate = "sc-composer"',
                'renderer_archive_path = "bin/fixture"',
                "",
                "[[release_targets]]",
                'target = "x86_64-unknown-linux-gnu"',
                'os = "ubuntu-latest"',
                'archive = "tar.gz"',
                "",
                "[[release_binaries]]",
                'name = "fixture"',
                "",
                "[[release_binaries]]",
                'name = "fixture-daemon"',
                "",
                *(crates_section if include_crates else []),
                *(python_section if include_python else []),
                "[channels.pypi]",
                'workflow = "pypi-publish.yml"',
                'dispatch_inputs = { target = "production" }',
                'test_repository = "testpypi"',
                'production_repository = "pypi"',
                "",
                "[channels.homebrew]",
                'workflow = "homebrew-publish.yml"',
                'dispatch_inputs = {}',
                'tap_repository = "example/homebrew-tap"',
                'renderer_target = "x86_64-unknown-linux-gnu"',
                "",
                "[[channels.homebrew.formulas]]",
                'path = "Formula/fixture.rb"',
                'template = "release/homebrew/formula.rb.j2"',
                'class = "Fixture"',
                'binaries = ["fixture"]',
                'test_command = "--help"',
                'test_output = "fixture"',
                'release_track = "stable"',
                "",
                "[[channels.homebrew.assets]]",
                'key = "linux"',
                'target = "x86_64-unknown-linux-gnu"',
                "",
                "[channels.winget]",
                'workflow = "winget-publish.yml"',
                'dispatch_inputs = {}',
                'identifier = "example.fixture"',
                'installer_target = "x86_64-unknown-linux-gnu"',
                "",
                "[channels.scoop]",
                'workflow = "scoop-publish.yml"',
                'dispatch_inputs = {}',
                'bucket_repository = "example/scoop-bucket"',
                'manifest_path = "fixture.json"',
                'manifest_template = "release/scoop/manifest.json.j2"',
                'installer_target = "x86_64-unknown-linux-gnu"',
                'binary = "bin/fixture"',
                'renderer_target = "x86_64-unknown-linux-gnu"',
                "",
            ]
        ),
        encoding="utf-8",
    )

    return workspace, manifest


def run_validate_manifest(
    tmp_path: Path, *, manifest_wheels: list[str], **fixture_kwargs: object
) -> subprocess.CompletedProcess[str]:
    workspace, manifest = write_repo_fixture(
        tmp_path,
        manifest_wheels=manifest_wheels,
        **fixture_kwargs,
    )
    return subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            "validate-manifest",
            "--manifest",
            str(manifest),
            "--workspace-toml",
            str(workspace),
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )


def repo_root() -> Path:
    # Tests are installed at <consumer>/.github/scripts/tests.  Keep this
    # relative to the consumer root so the untouched vendored suite works in
    # every repository that installs the publish kit.
    return next(path for path in Path(__file__).resolve().parents if (path / "install.py").is_file())


def scripts_root() -> Path:
    return repo_root() / ".github" / "scripts"


def test_release_artifact_cli_stays_below_the_script_line_ceiling() -> None:
    cli_lines = (scripts_root() / "release_artifacts.py").read_text(
        encoding="utf-8"
    ).splitlines()
    assert len(cli_lines) <= 1000
    assert (scripts_root() / "release_manifest.py").is_file()


def release_workflow_text() -> str:
    return (repo_root() / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")


def pypi_publish_workflow_text() -> str:
    return (repo_root() / ".github" / "workflows" / "pypi-publish.yml").read_text(encoding="utf-8")


def homebrew_publish_workflow_text() -> str:
    return (repo_root() / ".github" / "workflows" / "homebrew-publish.yml").read_text(encoding="utf-8")


def winget_publish_workflow_text() -> str:
    return (repo_root() / ".github" / "workflows" / "winget-publish.yml").read_text(encoding="utf-8")


def scoop_publish_workflow_text() -> str:
    return (repo_root() / ".github" / "workflows" / "scoop-publish.yml").read_text(encoding="utf-8")


def crates_publish_workflow_text() -> str:
    return (repo_root() / ".github" / "workflows" / "crates-publish.yml").read_text(encoding="utf-8")


def release_preflight_workflow_text() -> str:
    return (repo_root() / ".github" / "workflows" / "release-preflight.yml").read_text(encoding="utf-8")


def published_release_guard_text() -> str:
    return (
        repo_root() / ".github" / "actions" / "verify-published-release" / "action.yml"
    ).read_text(encoding="utf-8")


def release_manifest() -> dict:
    return tomllib.loads(
        (repo_root() / "release" / "publish-artifacts.toml").read_text(encoding="utf-8")
    )


def require_manifest_crates() -> dict:
    if not (repo_root() / "release" / "publish-artifacts.toml").is_file():
        pytest.skip("package source has no consumer-specific rendered manifest")
    manifest = release_manifest()
    if not manifest.get("crates"):
        pytest.skip("consumer manifest does not publish Rust crates")
    return manifest


def require_full_channel_set() -> dict:
    manifest = require_manifest_crates()
    required = {"pypi", "homebrew", "winget", "scoop"}
    channels = manifest.get("channels", {})
    if not all(channels.get(name, {}).get("enabled") is True for name in required):
        pytest.skip("consumer does not enable every post-release channel")
    return manifest


def renderer_binary() -> str | None:
    manifest_path = repo_root() / "release" / "publish-artifacts.toml"
    if not manifest_path.is_file():
        return None
    binaries = tomllib.loads(manifest_path.read_text(encoding="utf-8")).get(
        "release_binaries", []
    )
    return binaries[0].get("name") if binaries else None


def python_pyproject_text() -> str:
    return (repo_root() / "bindings" / "python" / "pyproject.toml").read_text(encoding="utf-8")


def python_cargo_toml_text() -> str:
    return (repo_root() / "bindings" / "python" / "Cargo.toml").read_text(encoding="utf-8")


def test_validate_manifest_accepts_matching_python_release_shape(tmp_path: Path) -> None:
    result = run_validate_manifest(
        tmp_path,
        manifest_wheels=["ubuntu-latest", "macos-latest", "windows-latest"],
    )

    assert result.returncode == 0, result.stderr
    assert "manifest validation passed" in result.stdout


def run_fixture_command(
    tmp_path: Path, *args: str, manifest: Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            *args,
            "--manifest",
            str(manifest),
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )


def test_pure_python_manifest_without_crates_loads_and_gates_cargo_legs(tmp_path: Path) -> None:
    result = run_validate_manifest(
        tmp_path, manifest_wheels=["ubuntu-latest"], include_crates=False
    )
    assert result.returncode == 0, result.stderr
    assert "manifest validation passed" in result.stdout

    manifest = tmp_path / "release" / "publish-artifacts.toml"
    plan_result = run_fixture_command(tmp_path, "build-plan", manifest=manifest)
    assert plan_result.returncode == 0, plan_result.stderr
    plan = json.loads(plan_result.stdout)
    assert plan["has_crates"] is False
    assert plan["has_python_wheels"] is True
    assert plan["workspace_toml"] == "Cargo.toml"

    publish_plan = run_fixture_command(tmp_path, "list-publish-plan", manifest=manifest)
    assert publish_plan.returncode == 0, publish_plan.stderr
    assert publish_plan.stdout.strip() == ""


def test_rust_only_manifest_emits_empty_python_matrices(tmp_path: Path) -> None:
    result = run_validate_manifest(
        tmp_path, manifest_wheels=["ubuntu-latest"], include_python=False
    )
    assert result.returncode == 0, result.stderr

    manifest = tmp_path / "release" / "publish-artifacts.toml"
    wheel_result = run_fixture_command(tmp_path, "python-wheel-matrix", manifest=manifest)
    sdist_result = run_fixture_command(tmp_path, "python-sdist-matrix", manifest=manifest)
    plan_result = run_fixture_command(tmp_path, "build-plan", manifest=manifest)

    assert wheel_result.returncode == 0, wheel_result.stderr
    assert json.loads(wheel_result.stdout) == {"include": []}
    assert sdist_result.returncode == 0, sdist_result.stderr
    assert json.loads(sdist_result.stdout) == {"include": []}
    assert plan_result.returncode == 0, plan_result.stderr
    plan = json.loads(plan_result.stdout)
    assert plan["has_crates"] is True
    assert plan["has_python_wheels"] is False
    assert plan["has_python_sdists"] is False


def test_python_matrices_select_the_declared_build_system(tmp_path: Path) -> None:
    maturin_result = run_validate_manifest(tmp_path, manifest_wheels=["ubuntu-latest"])
    assert maturin_result.returncode == 0, maturin_result.stderr
    manifest = tmp_path / "release" / "publish-artifacts.toml"
    wheel_result = run_fixture_command(tmp_path, "python-wheel-matrix", manifest=manifest)
    assert wheel_result.returncode == 0, wheel_result.stderr
    entry = json.loads(wheel_result.stdout)["include"][0]
    assert entry["build_system"] == "maturin"
    assert entry["cargo_manifest"] == "bindings/python/Cargo.toml"

    setuptools_dir = tmp_path / "setuptools"
    setuptools_dir.mkdir()
    setuptools_result = run_validate_manifest(
        setuptools_dir, manifest_wheels=["ubuntu-latest"], python_build_system="setuptools"
    )
    assert setuptools_result.returncode == 0, setuptools_result.stderr
    setuptools_manifest = setuptools_dir / "release" / "publish-artifacts.toml"
    setuptools_wheels = run_fixture_command(
        setuptools_dir, "python-wheel-matrix", manifest=setuptools_manifest
    )
    setuptools_sdists = run_fixture_command(
        setuptools_dir, "python-sdist-matrix", manifest=setuptools_manifest
    )
    assert setuptools_wheels.returncode == 0, setuptools_wheels.stderr
    entry = json.loads(setuptools_wheels.stdout)["include"][0]
    assert entry["build_system"] == "setuptools"
    assert entry["cargo_manifest"] == ""
    assert entry["source"] == "bindings/python"
    assert setuptools_sdists.returncode == 0, setuptools_sdists.stderr
    assert json.loads(setuptools_sdists.stdout)["include"][0]["build_system"] == "setuptools"


def test_validate_manifest_rejects_missing_or_unsupported_build_system(tmp_path: Path) -> None:
    unsupported_dir = tmp_path / "unsupported"
    unsupported_dir.mkdir()
    unsupported = run_validate_manifest(
        unsupported_dir, manifest_wheels=["ubuntu-latest"], python_build_system="unsupported"
    )
    assert unsupported.returncode != 0
    assert "unsupported build_system" in unsupported.stderr

    missing_dir = tmp_path / "missing"
    missing_dir.mkdir()
    missing = run_validate_manifest(
        missing_dir, manifest_wheels=["ubuntu-latest"], python_build_system="missing"
    )
    assert missing.returncode != 0
    assert "unsupported build_system" in missing.stderr


def test_crates_leg_is_separate_and_independently_retryable() -> None:
    release_text = release_workflow_text()
    crates_text = crates_publish_workflow_text()

    # The GitHub Release leg must not depend on crates.io publication.
    assert "needs: [gate-and-tag, build, build-python-wheels, build-python-sdists]" in release_text
    assert "needs: [gate-and-tag, build, publish," not in release_text

    assert "workflow_dispatch:" in crates_text
    assert "uses: ./.github/actions/verify-published-release" in crates_text
    assert "release_tag: ${{ inputs.tag }}" in crates_text
    assert "group: publish-crates-${{ inputs.tag }}" in crates_text
    assert "cancel-in-progress: false" in crates_text
    assert "environment: crates-io" in crates_text
    assert "publish_if_missing" in crates_text
    assert "already published; skipping" in crates_text
    assert "list-publish-plan" in crates_text
    assert "gate-and-tag" not in crates_text
    assert "CARGO_REGISTRY_TOKEN" in crates_text


def test_github_release_leg_is_detect_and_skip(tmp_path: Path) -> None:
    text = release_workflow_text()

    assert "replace_release_assets:" in text
    assert "release-asset-patterns" in text
    assert "id: published_release_probe" in text
    assert "uses: ./.github/actions/verify-published-release" in text
    # The probe fails closed: no continue-on-error swallowing transient API
    # failures, and every build/upload leg keys off the confirmed state.
    assert "continue-on-error" not in text
    assert "probe: 'true'" in text
    assert (
        text.count(
            "if: ${{ steps.published_release_probe.outputs.release_state != 'complete' || inputs.replace_release_assets == true }}"
        )
        == 4
    )
    assert (
        "if: ${{ steps.published_release_probe.outputs.release_state == 'complete' && inputs.replace_release_assets != true }}"
        in text
    )
    assert "steps.published_release_probe.outcome" not in text
    assert "already exists with every expected asset; skipping upload" in text
    assert "deliberately replacing assets" in text
    assert "'^checksums\\.txt$'" in text

    _, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    result = run_fixture_command(tmp_path, "release-asset-patterns", manifest=manifest)
    assert result.returncode == 0, result.stderr
    assert result.stdout.splitlines() == [
        r"^fixture_.*_x86_64\-unknown\-linux\-gnu\.tar\.gz$"
    ]


def test_no_single_repo_concerns_leak_into_kit_workflows_actions_or_scripts() -> None:
    """Extend the anti-leakage guard to every vendored workflow, action, and script."""
    forbidden = ("sc-compose", "sc_compose", "randlee")
    allowlist = {
        # Deliberate shared ecosystem pin: setup-sc-lint's default repository.
        "actions/setup-sc-lint/action.yml": {"randlee"},
        # The pinned renderer wheel is the sc-compose PyPI package by design.
        "scripts/bootstrap_sc_compose.py": {"sc-compose", "sc_compose"},
    }
    kit_workflows = (
        "release.yml",
        "release-candidate.yml",
        "release-preflight.yml",
        "crates-publish.yml",
        "pypi-publish.yml",
        "homebrew-publish.yml",
        "scoop-publish.yml",
        "winget-publish.yml",
    )
    kit_actions = (
        "extract-published-renderer",
        "setup-lint-toolchain",
        "setup-python-release-build",
        "setup-sc-lint",
        "verify-published-release",
    )
    kit_scripts = (
        "bootstrap_sc_compose.py",
        "release_artifacts.py",
        "release_manifest.py",
        "release_gate.sh",
    )
    github_root = repo_root() / ".github"
    files = [
        *(github_root / "workflows" / name for name in kit_workflows),
        *(github_root / "actions" / name / "action.yml" for name in kit_actions),
        *(github_root / "scripts" / name for name in kit_scripts),
    ]
    if (repo_root() / ".sc-publish-source-root").is_file():
        # In the kit source the inventory above must be complete, so new files
        # cannot dodge the guard. Consumers may add their own workflows.
        assert sorted(path.name for path in (github_root / "workflows").glob("*.yml")) == sorted(kit_workflows)
        assert sorted(path.parent.name for path in (github_root / "actions").rglob("action.yml")) == sorted(kit_actions)
        assert sorted(
            path.name
            for path in (github_root / "scripts").iterdir()
            if path.suffix in (".py", ".sh")
        ) == sorted(kit_scripts)
    for path in files:
        relative = path.relative_to(github_root).as_posix()
        if path.name == "action.yml":
            relative = f"actions/{path.parent.name}/action.yml"
        text = path.read_text(encoding="utf-8")
        for needle in forbidden:
            if needle in allowlist.get(relative, set()):
                continue
            assert needle not in text, f"{relative} leaks single-repo concern {needle!r}"


def test_hygiene_single_sources_pins_paths_and_publish_time_validations(tmp_path: Path) -> None:
    release_text = release_workflow_text()
    preflight_text = release_preflight_workflow_text()
    crates_text = crates_publish_workflow_text()
    homebrew_text = homebrew_publish_workflow_text()
    scoop_text = scoop_publish_workflow_text()
    python_action_text = (
        repo_root() / ".github" / "actions" / "setup-python-release-build" / "action.yml"
    ).read_text(encoding="utf-8")
    sc_lint_action_text = (
        repo_root() / ".github" / "actions" / "setup-sc-lint" / "action.yml"
    ).read_text(encoding="utf-8")

    # Item 1: the Rust toolchain pin is single-sourced through build-plan.
    for text in (release_text, preflight_text, crates_text, python_action_text):
        assert "1.94.1" not in text
        assert "rust_toolchain" in text

    # Item 2: the sc-lint repository slug is an input with a documented pin.
    assert "SC_LINT_REPOSITORY" in sc_lint_action_text
    assert 'default: "randlee/sc-lint"' in sc_lint_action_text
    assert "https://github.com/randlee" not in sc_lint_action_text

    # Item 5: release.yml reads the manifest path from its env everywhere.
    assert release_text.count("release/publish-artifacts.toml") == 1

    # Item 7: tap/bucket pushes fetch-rebase-retry instead of racing.
    for text in (homebrew_text, scoop_text):
        assert "git pull --rebase origin" in text
        assert "for attempt in 1 2 3 4 5; do" in text
        assert "push rejected (attempt" in text

    # Item 8: the pyproject input is required, not layout-inferred.
    assert "bindings/python/pyproject.toml" not in python_action_text

    # Item 6: contract-declared GitHub environments are verified by preflight.
    assert "Verify contract-declared GitHub environments exist" in preflight_text
    assert ".github_environments[]?" in preflight_text
    _, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    plan_result = run_fixture_command(tmp_path, "preflight-secret-plan", manifest=manifest)
    assert plan_result.returncode == 0, plan_result.stderr
    assert json.loads(plan_result.stdout)["github_environments"] == [
        "crates-io",
        "pypi",
        "testpypi",
    ]


def test_validate_manifest_requires_publish_time_channel_fields(tmp_path: Path) -> None:
    pypi_dir = tmp_path / "pypi"
    pypi_dir.mkdir()
    workspace, manifest = write_repo_fixture(pypi_dir, manifest_wheels=["ubuntu-latest"])
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            'test_repository = "testpypi"', 'test_repository = ""', 1
        ),
        encoding="utf-8",
    )
    result = run_fixture_command(
        pypi_dir,
        "validate-manifest",
        "--workspace-toml",
        str(workspace),
        manifest=manifest,
    )
    assert result.returncode != 0
    assert "[channels.pypi].test_repository must be a non-empty string" in result.stderr

    winget_dir = tmp_path / "winget"
    winget_dir.mkdir()
    workspace, manifest = write_repo_fixture(winget_dir, manifest_wheels=["ubuntu-latest"])
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            'identifier = "example.fixture"', 'identifier = ""', 1
        ),
        encoding="utf-8",
    )
    result = run_fixture_command(
        winget_dir,
        "validate-manifest",
        "--workspace-toml",
        str(workspace),
        manifest=manifest,
    )
    assert result.returncode != 0
    assert "[channels.winget].identifier must be a non-empty string" in result.stderr


def test_load_manifest_rejects_unsupported_schema_version(tmp_path: Path) -> None:
    _, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            "schema_version = 1", "schema_version = 2", 1
        ),
        encoding="utf-8",
    )
    result = run_fixture_command(tmp_path, "build-plan", manifest=manifest)
    assert result.returncode != 0
    assert "unsupported manifest schema_version" in result.stderr


def test_winget_leg_probes_before_submitting_and_pins_the_releaser() -> None:
    text = winget_publish_workflow_text()

    assert "id: winget_probe" in text
    assert "repos/microsoft/winget-pkgs/contents/${manifest_path}" in text
    assert "search/issues" in text
    assert "type:pr in:title" in text
    assert "already_published" in text
    assert "if: ${{ steps.winget_probe.outputs.already_published != 'true' }}" in text
    # Fail closed: only a confirmed 404 may fall through to the PR search,
    # and incomplete search results must not be read as "no duplicate".
    assert "grep -Eqi 'HTTP 404|Not Found'" in text
    assert "is indeterminate (not a confirmed 404); failing closed" in text
    assert "incomplete_results" in text
    # The third-party releaser must be pinned to an immutable commit SHA.
    assert (
        "uses: vedantmgoyal2009/winget-releaser@4ffc7888bffd451b357355dc214d43bb9f23917e # v2"
        in text
    )
    assert "winget-releaser@v2\n" not in text


def test_crates_already_published_detection_uses_exact_version_lookup() -> None:
    release_text = release_workflow_text()
    crates_text = crates_publish_workflow_text()
    script_text = (scripts_root() / "release_artifacts.py").read_text(encoding="utf-8")
    manifest_module_text = (scripts_root() / "release_manifest.py").read_text(encoding="utf-8")

    for text in (release_text, crates_text):
        assert "cargo search" not in text
        assert "public-registry-inquiry-plan" in text
        assert "version_lookup_url" in text
        assert "publish-channel-contracts.toml" in text
        assert "indeterminate" in text

    assert "cargo search" not in script_text
    assert "registry_version_state" in script_text
    assert "must_be_absent" not in release_text  # policy lives in the contract
    assert "registry lookup failed" in manifest_module_text


def test_release_workflows_gate_cargo_and_python_legs_on_the_manifest() -> None:
    release_text = release_workflow_text()
    preflight_text = release_preflight_workflow_text()

    assert "build-plan" in release_text
    assert "needs.release-plan.outputs.has_crates == 'true'" in release_text
    assert "needs.release-plan.outputs.has_python_wheels == 'true'" in release_text
    assert "needs.release-plan.outputs.has_python_sdists == 'true'" in release_text
    assert "Build wheels (maturin)" in release_text
    assert "Build wheels (setuptools)" in release_text
    assert "matrix.build_system == 'setuptools'" in release_text
    assert "python -m build --wheel" in release_text
    assert "python -m build --sdist" in release_text
    assert "steps.build_plan.outputs.workspace_toml" in release_text

    assert "build-plan" in preflight_text
    assert preflight_text.count("steps.build_plan.outputs.has_crates == 'true'") >= 5
    assert "steps.build_plan.outputs.workspace_toml" in preflight_text
    assert '--workspace-toml Cargo.toml' not in preflight_text
    assert 'if [[ "${HAS_CRATES}" == "true" ]]; then' in preflight_text


def test_homebrew_workflow_selects_manifest_formula_tracks(tmp_path: Path) -> None:
    _, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    formulas = """
[[channels.homebrew.formulas]]
path = "Formula/fixture-alt.rb"
template = "release/homebrew/alternate.rb.j2"
class = "FixtureAlt"
binaries = ["fixture", "fixture-daemon"]
test_binary = "fixture-daemon"
test_command = "--version"
test_output = "fixture-alt"
release_track = "stable"

[[channels.homebrew.formulas]]
path = "Formula/fixture-preview.rb"
template = "release/homebrew/preview.rb.j2"
class = "FixturePreview"
binaries = ["fixture"]
test_command = "--version"
test_output = "fixture-preview"
release_track = "prerelease"

"""
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            "[[channels.homebrew.assets]]", formulas + "[[channels.homebrew.assets]]", 1
        ),
        encoding="utf-8",
    )

    def channel_config(tag: str) -> dict:
        result = subprocess.run(
            [
                sys.executable,
                str(scripts_root() / "release_artifacts.py"),
                "channel-config",
                "--manifest",
                str(manifest),
                "--channel",
                "homebrew",
                "--tag",
                tag,
            ],
            cwd=tmp_path,
            text=True,
            capture_output=True,
            check=False,
        )
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)

    stable = channel_config("v1.2.3")
    prerelease = channel_config("v1.2.4-rc.1")
    assert [formula["path"] for formula in stable["channel"]["formulas"]] == [
        "Formula/fixture.rb",
        "Formula/fixture-alt.rb",
    ]
    assert [formula["path"] for formula in prerelease["channel"]["formulas"]] == [
        "Formula/fixture-preview.rb"
    ]
    assert stable["channel"]["formulas"][1]["binaries"] == ["fixture", "fixture-daemon"]
    assert stable["channel"]["formulas"][1]["test_binary"] == "fixture-daemon"

    workflow = homebrew_publish_workflow_text()
    assert '--tag "${{ inputs.tag }}"' in workflow
    assert 'channel["formulas"]' in workflow
    assert 'formula["path"]' in workflow
    assert 'formula["template"]' in workflow
    assert "Formula/fixture" not in workflow
    assert "FixturePreview" not in workflow
    assert "sc-compose" not in workflow
    assert "randlee" not in workflow


def test_homebrew_legacy_binary_normalizes_to_a_single_binary_list(tmp_path: Path) -> None:
    _, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            'binaries = ["fixture"]', 'binary = "fixture"', 1
        ),
        encoding="utf-8",
    )
    result = subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            "channel-config",
            "--manifest",
            str(manifest),
            "--channel",
            "homebrew",
            "--tag",
            "v1.2.3",
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr
    formula = json.loads(result.stdout)["channel"]["formulas"][0]
    assert formula["binaries"] == ["fixture"]
    assert formula["test_binary"] == "fixture"


def test_validate_manifest_rejects_unknown_homebrew_formula_binary(tmp_path: Path) -> None:
    workspace, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            'binaries = ["fixture"]', 'binaries = ["not-a-release-binary"]', 1
        ),
        encoding="utf-8",
    )
    result = subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            "validate-manifest",
            "--manifest",
            str(manifest),
            "--workspace-toml",
            str(workspace),
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert "references undeclared release binary(s)" in result.stderr


def test_validate_manifest_rejects_unknown_channel_target(tmp_path: Path) -> None:
    workspace, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            'installer_target = "x86_64-unknown-linux-gnu"',
            'installer_target = "unknown-target"',
            1,
        ),
        encoding="utf-8",
    )
    result = subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            "validate-manifest",
            "--manifest",
            str(manifest),
            "--workspace-toml",
            str(workspace),
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode != 0
    assert "references unknown release target" in result.stderr


def test_validate_manifest_requires_manifest_driven_scoop_channel_inputs(tmp_path: Path) -> None:
    workspace, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            'manifest_template = "release/scoop/manifest.json.j2"\n', "", 1
        ),
        encoding="utf-8",
    )
    result = subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            "validate-manifest",
            "--manifest",
            str(manifest),
            "--workspace-toml",
            str(workspace),
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert "[channels.scoop] missing required keys: manifest_template" in result.stderr


def test_validate_manifest_rejects_unknown_renderer_target(tmp_path: Path) -> None:
    workspace, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            'renderer_target = "x86_64-unknown-linux-gnu"',
            'renderer_target = "unknown-renderer"',
            1,
        ),
        encoding="utf-8",
    )
    result = subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            "validate-manifest",
            "--manifest",
            str(manifest),
            "--workspace-toml",
            str(workspace),
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert "renderer_target references unknown release target" in result.stderr


def test_validate_manifest_requires_explicit_homebrew_bundle_destination(tmp_path: Path) -> None:
    workspace, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest"])
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            "[[release_binaries]]\nname = \"fixture\"",
            "[[release_binaries]]\nname = \"fixture\"\nbundled_paths = [{ source = \"examples\", destination = \"share/fixture/examples\" }]",
        ),
        encoding="utf-8",
    )
    result = subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            "validate-manifest",
            "--manifest",
            str(manifest),
            "--workspace-toml",
            str(workspace),
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert "homebrew_destination_components" in result.stderr


def test_verify_python_release_assets_accepts_manifest_declared_wheels_and_sdist(tmp_path: Path) -> None:
    _, manifest = write_repo_fixture(tmp_path, manifest_wheels=["ubuntu-latest", "windows-latest"])
    assets = tmp_path / "assets"
    assets.mkdir()
    for suffix in ("linux", "windows"):
        with zipfile.ZipFile(assets / f"fixture-{suffix}.whl", "w") as wheel:
            wheel.writestr("fixture-1.1.0.dist-info/METADATA", "Name: sc-compose\nVersion: 1.1.0\n")
    with tarfile.open(assets / "fixture-1.1.0.tar.gz", "w:gz") as sdist:
        metadata = b"Name: sc-compose\nVersion: 1.1.0\n"
        info = tarfile.TarInfo("fixture-1.1.0/PKG-INFO")
        info.size = len(metadata)
        sdist.addfile(info, io.BytesIO(metadata))

    result = subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            "verify-python-release-assets",
            "--manifest",
            str(manifest),
            "--asset-dir",
            str(assets),
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr
    assert "'sc-compose': {'wheel': 2, 'sdist': 1}" in result.stdout


def test_release_manifest_publishes_sc_sha_before_its_consumers() -> None:
    """Keep manifest-declared crate ordering and fields regression-tested."""
    manifest = require_manifest_crates()
    crates = [entry for entry in manifest["crates"] if entry.get("publish", True)]
    orders = [entry["publish_order"] for entry in crates]
    assert orders == sorted(orders)
    assert len(orders) == len(set(orders))
    names = [entry["package"] for entry in crates]
    if {"sc-sha", "sc-composer", "sc-compose"}.issubset(names):
        positions = {name: names.index(name) for name in names}
        assert positions["sc-sha"] < positions["sc-composer"] < positions["sc-compose"]
    for entry in crates:
        assert entry["artifact"]
        assert entry["package"]
        assert entry["cargo_toml"].endswith("Cargo.toml")
        assert entry["wait_after_publish_seconds"] >= 0
    assert manifest["channels"]


def run_manifest_command(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(scripts_root() / "release_artifacts.py"),
            *args,
        ],
        cwd=repo_root(),
        text=True,
        capture_output=True,
        check=False,
    )


def test_manifest_drives_parallel_post_release_dispatch_plan() -> None:
    require_full_channel_set()
    result = run_manifest_command(
        "channel-dispatch-plan",
        "--manifest",
        "release/publish-artifacts.toml",
        "--tag",
        "v1.4.2",
    )

    assert result.returncode == 0, result.stderr
    channels = json.loads(result.stdout)["channels"]
    assert [channel["name"] for channel in channels] == [
        "pypi",
        "homebrew",
        "winget",
        "scoop",
    ]
    assert channels[0] == {
        "name": "pypi",
        "agent": "pypi-publisher",
        "workflow": "pypi-publish.yml",
        "inputs": {"tag": "v1.4.2", "target": "production"},
        "credential_rehearsal": {
            "workflow": "pypi-publish.yml",
            "inputs": {"tag": "v1.4.2", "target": "testpypi"},
        },
        "preflight": {
            "agent": "pypi-publisher",
            "repository_secrets": [],
            "environment_secrets": [
                {"environment": "pypi", "name": "PYPI_API_TOKEN"},
                {"environment": "testpypi", "name": "TEST_PYPI_API_TOKEN"},
            ],
            "liveness_checks": [],
            "public_registry_checks": True,
            "credential_rehearsal": {
                "workflow": "pypi-publish.yml",
                "inputs": {"target": "testpypi"},
            },
        },
    }
    assert channels[1]["preflight"] == {
        "agent": "homebrew-publisher",
        "repository_secrets": ["HOMEBREW_TAP_TOKEN"],
        "environment_secrets": [],
        "liveness_checks": [{"name": "HOMEBREW_TAP_TOKEN", "kind": "github"}],
        "public_registry_checks": False,
        "credential_rehearsal": None,
    }
    assert channels[2]["preflight"] == {
        "agent": "winget-publisher",
        "repository_secrets": ["WINGET_GITHUB_TOKEN"],
        "environment_secrets": [],
        "liveness_checks": [{"name": "WINGET_GITHUB_TOKEN", "kind": "github"}],
        "public_registry_checks": False,
        "credential_rehearsal": None,
    }
    assert channels[3]["preflight"] == {
        "agent": "scoop-publisher",
        "repository_secrets": ["SCOOP_BUCKET_TOKEN"],
        "environment_secrets": [],
        "liveness_checks": [{"name": "SCOOP_BUCKET_TOKEN", "kind": "github"}],
        "public_registry_checks": False,
        "credential_rehearsal": None,
    }


def test_manifest_drives_non_disclosing_preflight_secret_plan() -> None:
    require_full_channel_set()
    result = run_manifest_command(
        "preflight-secret-plan",
        "--manifest",
        "release/publish-artifacts.toml",
    )

    assert result.returncode == 0, result.stderr
    plan = json.loads(result.stdout)
    assert plan["repository_secrets"] == [
        "CARGO_REGISTRY_TOKEN",
        "HOMEBREW_TAP_TOKEN",
        "WINGET_GITHUB_TOKEN",
        "SCOOP_BUCKET_TOKEN",
    ]
    assert plan["repository_secret_channels"] == [
        {"name": "crates_io", "secrets": ["CARGO_REGISTRY_TOKEN"]},
        {"name": "homebrew", "secrets": ["HOMEBREW_TAP_TOKEN"]},
        {"name": "winget", "secrets": ["WINGET_GITHUB_TOKEN"]},
        {"name": "scoop", "secrets": ["SCOOP_BUCKET_TOKEN"]},
    ]
    assert plan["environment_secrets"] == [
        {"environment": "pypi", "name": "PYPI_API_TOKEN"},
        {"environment": "testpypi", "name": "TEST_PYPI_API_TOKEN"},
    ]
    assert plan["liveness_channel_checks"] == [
        {"channel": "crates_io", "name": "CARGO_REGISTRY_TOKEN", "kind": "crates_io"},
        {"channel": "homebrew", "name": "HOMEBREW_TAP_TOKEN", "kind": "github"},
        {"channel": "winget", "name": "WINGET_GITHUB_TOKEN", "kind": "github"},
        {"channel": "scoop", "name": "SCOOP_BUCKET_TOKEN", "kind": "github"},
    ]
    contracts = {
        entry["name"]: entry
        for entry in [*plan["root_channels"], *plan["post_release_channels"]]
    }
    assert {name: entry["agent"] for name, entry in contracts.items()} == {
        "crates_io": "crates-io-publisher",
        "github_release": "github-release-publisher",
        "pypi": "pypi-publisher",
        "homebrew": "homebrew-publisher",
        "winget": "winget-publisher",
        "scoop": "scoop-publisher",
    }
    assert contracts["crates_io"]["public_registry_checks"] is True
    assert contracts["pypi"]["public_registry_checks"] is True
    assert contracts["github_release"]["github_actions_permissions"] == ["contents:write"]
    assert contracts["pypi"]["credential_rehearsal"] == {
        "workflow": "pypi-publish.yml",
        "inputs": {"target": "testpypi"},
    }


def test_channel_preflight_results_execute_contract_outcome_mapping() -> None:
    require_full_channel_set()
    passing_outcomes = json.dumps(
        {
            "ownership": "success",
            "release_metadata": "success",
            "repository_secrets": "success",
            "repository_secret_channels": {
                "crates_io": "success",
                "homebrew": "success",
                "winget": "success",
                "scoop": "success",
            },
            "environment_secrets": "success",
            "credential_liveness": "success",
            "credential_liveness_channels": {
                "crates_io": "success",
                "homebrew": "success",
                "winget": "success",
                "scoop": "success",
            },
            "registry_state": "success",
            "github_release_permissions": "success",
        }
    )
    result = run_manifest_command(
        "channel-preflight-results",
        "--manifest",
        "release/publish-artifacts.toml",
        "--outcomes",
        passing_outcomes,
        "--tag",
        "v1.4.2",
    )

    assert result.returncode == 0, result.stderr
    channels = {entry["name"]: entry for entry in json.loads(result.stdout)["channels"]}
    assert json.loads(result.stdout)["tag"] == "v1.4.2"
    assert list(channels) == [
        "crates_io",
        "github_release",
        "pypi",
        "homebrew",
        "winget",
        "scoop",
    ]
    assert all(channel["status"] == "passed" for channel in channels.values())
    assert all(channel["tag"] == "v1.4.2" for channel in channels.values())
    assert channels["pypi"]["credential_rehearsal"] == {
        "workflow": "pypi-publish.yml",
        "inputs": {"target": "testpypi"},
    }

    failed_outcomes = json.dumps(
        {
            "ownership": "success",
            "release_metadata": "success",
            "repository_secrets": "failure",
            "repository_secret_channels": {
                "crates_io": "success",
                "homebrew": "success",
                "winget": "success",
                "scoop": "failure",
            },
            "environment_secrets": "success",
            "credential_liveness": "success",
            "credential_liveness_channels": {
                "crates_io": "success",
                "homebrew": "success",
                "winget": "success",
                "scoop": "success",
            },
            "registry_state": "success",
            "github_release_permissions": "success",
        }
    )
    failed_result = run_manifest_command(
        "channel-preflight-results",
        "--manifest",
        "release/publish-artifacts.toml",
        "--outcomes",
        failed_outcomes,
        "--tag",
        "v1.4.2",
    )

    assert failed_result.returncode == 0, failed_result.stderr
    failed_channels = {
        entry["name"]
        for entry in json.loads(failed_result.stdout)["channels"]
        if entry["status"] == "failed"
    }
    assert failed_channels == {"scoop"}

    legacy_scalar_result = run_manifest_command(
        "channel-preflight-results",
        "--manifest",
        "release/publish-artifacts.toml",
        "--outcomes",
        json.dumps(
            {
                "ownership": "success",
                "release_metadata": "success",
                "repository_secrets": "success",
                "environment_secrets": "success",
                "credential_liveness": "success",
                "registry_state": "success",
                "github_release_permissions": "success",
            }
        ),
        "--tag",
        "v1.4.2",
    )
    assert legacy_scalar_result.returncode == 0, legacy_scalar_result.stderr
    assert all(
        entry["status"] == "passed"
        for entry in json.loads(legacy_scalar_result.stdout)["channels"]
    )

    unauthorized_outcomes = json.dumps(
        {
            "ownership": "failure",
            "release_metadata": "success",
            "repository_secrets": "success",
            "environment_secrets": "success",
            "credential_liveness": "success",
            "registry_state": "success",
            "github_release_permissions": "success",
        }
    )
    unauthorized_result = run_manifest_command(
        "channel-preflight-results",
        "--manifest",
        "release/publish-artifacts.toml",
        "--outcomes",
        unauthorized_outcomes,
        "--tag",
        "v1.4.2",
    )

    assert unauthorized_result.returncode == 0, unauthorized_result.stderr
    assert all(
        entry["status"] == "failed"
        for entry in json.loads(unauthorized_result.stdout)["channels"]
    )

    invalid_tag_outcomes = json.dumps(
        {
            "ownership": "success",
            "release_metadata": "failure",
            "repository_secrets": "success",
            "repository_secret_channels": {
                "crates_io": "success",
                "homebrew": "success",
                "winget": "success",
                "scoop": "success",
            },
            "environment_secrets": "success",
            "credential_liveness": "success",
            "credential_liveness_channels": {
                "crates_io": "success",
                "homebrew": "success",
                "winget": "success",
                "scoop": "success",
            },
            "registry_state": "success",
            "github_release_permissions": "success",
        }
    )
    invalid_tag_result = run_manifest_command(
        "channel-preflight-results",
        "--manifest",
        "release/publish-artifacts.toml",
        "--outcomes",
        invalid_tag_outcomes,
        "--tag",
        "v1.4.2-preflight-check",
    )

    assert invalid_tag_result.returncode == 0, invalid_tag_result.stderr
    for channel in json.loads(invalid_tag_result.stdout)["channels"]:
        assert channel["status"] == "failed"
        assert {
            "kind": "release_authorization",
            "requirements": ["normalized release tag"],
            "status": "failed",
        } in channel["checks"]

    blocked_result = run_manifest_command(
        "channel-preflight-results",
        "--manifest",
        "release/publish-artifacts.toml",
        "--outcomes",
        "{}",
        "--tag",
        "",
    )

    assert blocked_result.returncode == 0, blocked_result.stderr
    assert {
        entry["name"]
        for entry in json.loads(blocked_result.stdout)["channels"]
        if entry["status"] == "blocked"
    } == set(channels)
    assert json.loads(blocked_result.stdout)["tag"] is None


def test_background_workers_consume_and_gate_their_own_preflight_contracts() -> None:
    require_full_channel_set()
    plan_result = run_manifest_command(
        "preflight-secret-plan",
        "--manifest",
        "release/publish-artifacts.toml",
    )
    assert plan_result.returncode == 0, plan_result.stderr
    plan = json.loads(plan_result.stdout)
    worker_contracts = {
        entry["name"]: entry
        for entry in [*plan["root_channels"], *plan["post_release_channels"]]
    }
    assert set(worker_contracts) == {
        "crates_io",
        "github_release",
        "pypi",
        "homebrew",
        "winget",
        "scoop",
    }

    def results_for(outcomes: dict[str, object]) -> dict[str, dict]:
        result = run_manifest_command(
            "channel-preflight-results",
            "--manifest",
            "release/publish-artifacts.toml",
            "--outcomes",
            json.dumps(outcomes),
            "--tag",
            "v1.4.2",
        )
        assert result.returncode == 0, result.stderr
        return {entry["name"]: entry for entry in json.loads(result.stdout)["channels"]}

    passed_outcomes = {
        "ownership": "success",
        "release_metadata": "success",
        "repository_secrets": "success",
        "repository_secret_channels": {
            "crates_io": "success",
            "homebrew": "success",
            "winget": "success",
            "scoop": "success",
        },
        "environment_secrets": "success",
        "credential_liveness": "success",
        "credential_liveness_channels": {
            "crates_io": "success",
            "homebrew": "success",
            "winget": "success",
            "scoop": "success",
        },
        "registry_state": "success",
        "github_release_permissions": "success",
    }
    passed = results_for(passed_outcomes)
    for channel_name, contract in worker_contracts.items():
        assert passed[channel_name]["agent"] == contract["agent"]
        assert passed[channel_name]["status"] == "passed"

    pypi_credential_failed = results_for(
        {**passed_outcomes, "environment_secrets": "failure"}
    )
    assert pypi_credential_failed["pypi"]["status"] == "failed"
    assert all(
        result["status"] == "passed"
        for channel_name, result in pypi_credential_failed.items()
        if channel_name != "pypi"
    )

    crates_secret_failed = results_for(
        {
            **passed_outcomes,
            "repository_secret_channels": {
                "crates_io": "failure",
                "homebrew": "success",
                "winget": "success",
                "scoop": "success",
            },
            "credential_liveness_channels": {
                "crates_io": "failure",
                "homebrew": "success",
                "winget": "success",
                "scoop": "success",
            },
        }
    )
    assert crates_secret_failed["crates_io"]["status"] == "failed"
    assert all(
        result["status"] == "passed"
        for channel_name, result in crates_secret_failed.items()
        if channel_name != "crates_io"
    )

    scoop_liveness_failed = results_for(
        {
            **passed_outcomes,
            "repository_secret_channels": {
                "crates_io": "success",
                "homebrew": "success",
                "winget": "success",
                "scoop": "success",
            },
            "credential_liveness_channels": {
                "crates_io": "success",
                "homebrew": "success",
                "winget": "success",
                "scoop": "failure",
            },
        }
    )
    assert scoop_liveness_failed["scoop"]["status"] == "failed"
    assert all(
        result["status"] == "passed"
        for channel_name, result in scoop_liveness_failed.items()
        if channel_name != "scoop"
    )

    github_permission_failed = results_for(
        {**passed_outcomes, "github_release_permissions": "failure"}
    )
    assert github_permission_failed["github_release"]["status"] == "failed"
    assert all(
        result["status"] == "passed"
        for channel_name, result in github_permission_failed.items()
        if channel_name != "github_release"
    )


def test_public_registry_check_plan_assigns_named_agents_and_normalizes_python_names() -> None:
    manifest = require_manifest_crates()
    result = run_manifest_command(
        "public-registry-check-plan",
        "--manifest",
        "release/publish-artifacts.toml",
        "--version",
        "1.4.2",
    )

    assert result.returncode == 0, result.stderr
    checks = json.loads(result.stdout)["checks"]
    crates = [entry for entry in checks if entry["channel"] == "crates_io"]
    pypi = [entry for entry in checks if entry["channel"] == "pypi"]
    assert [entry["name"] for entry in crates] == [
        entry["package"] for entry in manifest["crates"]
    ]
    assert all(entry["agent"] == "crates-io-publisher" for entry in crates)
    assert all(entry["version_policy"] == "must_be_absent" for entry in crates)
    assert all("/api/v1/crates/" in entry["project_lookup_url"] for entry in crates)
    if manifest.get("python_packages"):
        assert {entry["registry"] for entry in pypi} == {"pypi", "testpypi"}
        assert all(entry["agent"] == "pypi-publisher" for entry in pypi)
        assert all("_" not in entry["normalized_name"] for entry in pypi)
        assert any(entry["version_policy"] == "informational" for entry in pypi)


def test_public_registry_inquiry_plan_is_contract_derived_and_read_only() -> None:
    crates = run_manifest_command(
        "public-registry-inquiry-plan",
        "--contracts",
        "release/publish-channel-contracts.toml.j2",
        "--channel",
        "crates_io",
        "--name",
        "atm-serde",
        "--version",
        "0.1.0",
    )
    pypi = run_manifest_command(
        "public-registry-inquiry-plan",
        "--contracts",
        "release/publish-channel-contracts.toml.j2",
        "--channel",
        "pypi",
        "--name",
        "ATM_Serde",
    )

    assert crates.returncode == 0, crates.stderr
    assert pypi.returncode == 0, pypi.stderr
    crate_check = json.loads(crates.stdout)["checks"]
    pypi_checks = json.loads(pypi.stdout)["checks"]
    assert crate_check == [
        {
            "channel": "crates_io",
            "agent": "crates-io-publisher",
            "registry": "crates.io",
            "name": "atm-serde",
            "normalized_name": "atm-serde",
            "expected_version": "0.1.0",
            "project_lookup_url": "https://crates.io/api/v1/crates/atm-serde",
            "version_lookup_url": "https://crates.io/api/v1/crates/atm-serde/0.1.0",
            "version_policy": "must_be_absent",
        }
    ]
    assert {entry["registry"] for entry in pypi_checks} == {"pypi", "testpypi"}
    assert all(entry["normalized_name"] == "atm-serde" for entry in pypi_checks)
    assert all(entry["version_lookup_url"] is None for entry in pypi_checks)


def test_release_workflow_enforces_python_release_invariants() -> None:
    text = release_workflow_text()
    pypi_text = pypi_publish_workflow_text()
    action_text = (
        repo_root() / ".github" / "actions" / "setup-python-release-build" / "action.yml"
    ).read_text(encoding="utf-8")

    assert "release-plan:" in text
    assert "release-target-matrix" in text
    assert "python-wheel-matrix" in text
    assert "python-sdist-matrix" in text
    assert "matrix: ${{ fromJSON(needs.release-plan.outputs.python_wheel_matrix) }}" in text
    assert "publish-testpypi:" in text
    assert "needs.gate-and-tag.outputs.release_target == 'testpypi'" in text
    assert "publish-pypi:" not in text
    assert "name: python-sdist-${{ matrix.artifact }}" in text
    assert "TEST_PYPI_API_TOKEN" in text
    assert "secrets.TEST_PYPI_TOKEN" not in text
    assert "--repository testpypi" in text
    assert "for pattern in *.tar.gz *.zip *.whl; do" in text
    assert "uses: ./.github/actions/setup-python-release-build" in text
    assert "update-homebrew:" not in text
    assert "publish-winget:" not in text
    assert "verify-python-version" in action_text
    assert "sync-python-version" in action_text
    assert "release_ref" in action_text
    assert "pyproject" in action_text

    assert "name: Publish PyPI" in pypi_text
    assert "release_tag: ${{ inputs.tag }}" in pypi_text
    assert "gh release download" in pypi_text
    assert "verify-python-release-assets" in pypi_text
    assert "maturin build" not in pypi_text
    assert "maturin sdist" not in pypi_text
    assert "name: Publish manifest-declared wheels and sdists to TestPyPI" in pypi_text
    assert (
        "if: ${{ inputs.target == 'testpypi' && needs.verify-release.outputs.python_upload_tool == 'maturin' }}"
        in pypi_text
    )
    assert "MATURIN_PYPI_TOKEN: ${{ secrets.TEST_PYPI_API_TOKEN }}" in pypi_text
    assert "name: Publish manifest-declared wheels and sdists to PyPI" in pypi_text
    assert (
        "if: ${{ inputs.target == 'production' && needs.verify-release.outputs.python_upload_tool == 'maturin' }}"
        in pypi_text
    )
    assert "MATURIN_PYPI_TOKEN: ${{ secrets.PYPI_API_TOKEN }}" in pypi_text
    assert "secrets.TEST_PYPI_TOKEN" not in pypi_text
    assert "secrets.PYPI_TOKEN" not in pypi_text
    assert "maturin upload --repository \"${PYPI_REPOSITORY}\" --non-interactive --skip-existing dist/*.whl dist/*.tar.gz" in pypi_text


def test_release_preflight_requires_each_standardized_secret() -> None:
    text = release_preflight_workflow_text()

    assert "Missing required GitHub Actions release secret(s):" in text
    for secret_name in (
        "CARGO_REGISTRY_TOKEN",
        "HOMEBREW_TAP_TOKEN",
        "SCOOP_BUCKET_TOKEN",
        "WINGET_GITHUB_TOKEN",
    ):
        assert secret_name in text
    assert "All manifest-required repository secrets are available." in text
    assert "preflight-secret-plan" in text
    assert '--manifest "${RELEASE_ARTIFACT_MANIFEST}"' in text
    assert '\\"${RELEASE_ARTIFACT_MANIFEST}\\"' not in text
    assert "Verify protected Python environment secret metadata" in text
    assert ".environment_secrets[]" in text
    assert "environments/${environment_name}/secrets" in text
    assert "environment:" not in text
    assert "Verify repository credential liveness" in text
    assert "https://crates.io/api/v1/me" in text
    assert "https://api.github.com/user" in text
    assert "rotate or replace it" in text
    assert 'echo "${token}"' not in text
    assert 'echo "${!secret_name}"' not in text


def test_channel_recovery_workflows_require_a_published_release() -> None:
    guard_text = published_release_guard_text()
    pypi_text = pypi_publish_workflow_text()
    homebrew_text = homebrew_publish_workflow_text()
    winget_text = winget_publish_workflow_text()
    scoop_text = scoop_publish_workflow_text()

    assert "No published GitHub Release found" in guard_text
    assert "is still a draft" in guard_text
    assert "optional SemVer prerelease/build metadata" in guard_text
    assert "REQUIRED_ASSET_PATTERNS" in guard_text

    for workflow_text in (pypi_text, homebrew_text, winget_text, scoop_text):
        assert "workflow_dispatch:" in workflow_text
        assert "uses: ./.github/actions/verify-published-release" in workflow_text
        assert "release_tag: ${{ inputs.tag }}" in workflow_text
        assert "gate-and-tag" not in workflow_text

    assert "WINGET_GITHUB_TOKEN" in winget_text
    assert "channel-config" in winget_text
    assert "HOMEBREW_TAP_TOKEN" in homebrew_text
    assert "ref: ${{ inputs.tag }}" in homebrew_text
    assert "channel-config" in homebrew_text
    assert "SCOOP_BUCKET_TOKEN" in scoop_text
    assert "channel-config" in scoop_text
    assert "Render Scoop manifest with published renderer" in scoop_text
    assert 'MANIFEST_TEMPLATE: ${{ fromJSON(needs.verify-release.outputs.channel_config).channel.manifest_template }}' in scoop_text
    assert ".replace(placeholder, value)" not in scoop_text
    assert "cargo run --quiet --manifest-path release-source/Cargo.toml" not in scoop_text
    assert "PUBLISHED_RENDERER" in scoop_text
    assert "Checkout workflow support" in scoop_text
    assert "uses: ./.github/actions/extract-published-renderer" in scoop_text

    assert "Render manifest-selected formulas with the published renderer" in homebrew_text
    assert '--tag "${{ inputs.tag }}"' in homebrew_text
    assert 'channel["formulas"]' in homebrew_text
    assert 'formula["path"]' in homebrew_text
    assert 'formula["template"]' in homebrew_text
    assert ".replace(placeholder, value)" not in homebrew_text
    assert "PUBLISHED_RENDERER" in homebrew_text
    assert "Checkout workflow support" in homebrew_text
    assert "uses: ./.github/actions/extract-published-renderer" in homebrew_text
    assert "install_block" not in homebrew_text
    assert "bundled_paths" in homebrew_text

    renderer_action = (
        repo_root()
        / ".github"
        / "actions"
        / "extract-published-renderer"
        / "action.yml"
    ).read_text(encoding="utf-8")
    assert "binary-path" in renderer_action
    assert "Published renderer archive is missing ${RENDERER_BINARY_PATH}" in renderer_action
    assert "renderer-path=${renderer}" in renderer_action


def render_release_template(
    tmp_path: Path, template: str, variables: dict[str, object]
) -> str:
    import sc_compose

    request = sc_compose.ComposeRequest(
        root=repo_root(),
        mode=sc_compose.ComposeMode.file(template),
        vars_input=variables,
        policy=sc_compose.ComposePolicy(strict_undeclared_variables=False),
    )
    return sc_compose.compose_file(request).rendered_text


def test_release_channel_templates_render_to_valid_ruby_and_json(tmp_path: Path) -> None:
    formula = render_release_template(
        tmp_path,
        "release/homebrew/formula.rb.j2",
        {
            "formula_class": "ScCompose",
            "description": "Standalone template composition CLI",
            "homepage": "https://github.com/randlee/sc-compose",
            "license": "MIT",
            "version": "1.4.2",
            "macos_arm_url": "https://example.invalid/arm.tar.gz",
            "macos_arm_sha256": "a" * 64,
            "macos_intel_url": "https://example.invalid/intel.tar.gz",
            "macos_intel_sha256": "b" * 64,
            "linux_url": "https://example.invalid/linux.tar.gz",
            "linux_sha256": "c" * 64,
            "test_binary": "sc-compose-daemon",
            "test_command": "--help",
            "test_output": "Standalone template composition CLI",
            "binary_paths": ["bin/sc-compose", "bin/sc-compose-daemon"],
            "bundled_paths": [
                {
                    "destination_components": ["pkgshare", "examples"],
                    "source_glob": "share/sc-compose/examples/*",
                }
            ],
        },
    )
    ruby = subprocess.run(
        ["ruby", "-c"], input=formula, text=True, capture_output=True, check=False
    )
    assert ruby.returncode == 0, ruby.stderr
    assert 'bin.install "bin/sc-compose"' in formula
    assert 'bin.install "bin/sc-compose-daemon"' in formula
    assert 'shell_output("#{bin}/" + "sc-compose-daemon"' in formula
    assert '("pkgshare"/"examples").install Dir["share/sc-compose/examples/*"]' in formula

    scoop = render_release_template(
        tmp_path,
        "release/scoop/manifest.json.j2",
        {
            "version": "1.4.2",
            "description": 'Quoted "description"',
            "homepage": "https://github.com/randlee/sc-compose",
            "license": "MIT",
            "windows_url": "https://example.invalid/windows.zip",
            "windows_sha256": "d" * 64,
            "extract_dir": "sc-compose_1.4.2_x86_64-pc-windows-msvc",
            "binary": "bin/sc-compose.exe",
        },
    )
    manifest = json.loads(scoop)
    assert manifest["description"] == 'Quoted "description"'
    assert manifest["architecture"]["64bit"]["bin"] == "bin/sc-compose.exe"


def test_homebrew_formula_tracks_and_binaries_are_documented() -> None:
    required = (
        repo_root() / "docs" / "publish-kit-requirements.md",
        repo_root() / "docs" / "sprints" / "fix-pr507-release-channel-runtime-checklist.md",
        repo_root() / "RELEASING.md",
        repo_root() / "CHANGELOG.md",
    )
    if not all(path.is_file() for path in required):
        pytest.skip("consumer does not include source-repository publishing documentation")
    requirements = (repo_root() / "docs" / "publish-kit-requirements.md").read_text(
        encoding="utf-8"
    )
    sprint = (
        repo_root() / "docs" / "sprints" / "fix-pr507-release-channel-runtime-checklist.md"
    ).read_text(encoding="utf-8")
    releasing = (repo_root() / "RELEASING.md").read_text(encoding="utf-8")
    changelog = (repo_root() / "CHANGELOG.md").read_text(encoding="utf-8")

    for text in (requirements, sprint, releasing, changelog):
        assert "release_track" in text
        assert "prerelease" in text
        assert "binaries" in text


def test_publish_kit_guidance_is_manifest_driven_and_token_non_disclosing() -> None:
    required = (
        repo_root() / "docs" / "publishing-agent.md",
        repo_root() / "docs" / "release-checklist.md",
        repo_root() / "docs" / "eval" / "publishing" / "publish-kit-agent-eval-plan.md",
        repo_root() / "docs" / "eval" / "README.md",
    )
    if not all(path.is_file() for path in required):
        pytest.skip("consumer does not include source-repository publishing documentation")
    publisher_text = (repo_root() / ".claude" / "agents" / "publisher.md").read_text(
        encoding="utf-8"
    )
    guide_text = (repo_root() / "docs" / "publishing-agent.md").read_text(encoding="utf-8")
    checklist_text = (repo_root() / "docs" / "release-checklist.md").read_text(
        encoding="utf-8"
    )
    channel_contract_text = (
        repo_root() / ".claude" / "skills" / "publishing" / "ref" / "channel-contracts.md"
    ).read_text(encoding="utf-8")
    eval_plan_text = (
        repo_root() / "docs" / "eval" / "publishing" / "publish-kit-agent-eval-plan.md"
    ).read_text(encoding="utf-8")
    eval_convention_text = (repo_root() / "docs" / "eval" / "README.md").read_text(
        encoding="utf-8"
    )
    publishing_skill_text = (repo_root() / ".claude" / "skills" / "publishing" / "SKILL.md").read_text(
        encoding="utf-8"
    )
    release_state_text = (
        repo_root() / ".claude" / "skills" / "publishing" / "ref" / "release-state-strategy.md"
    ).read_text(encoding="utf-8")
    preflight_template_text = (
        repo_root() / ".claude" / "skills" / "publishing" / "preflight.xml.j2"
    ).read_text(encoding="utf-8")
    publish_template_text = (
        repo_root() / ".claude" / "skills" / "publishing" / "publish.xml.j2"
    ).read_text(encoding="utf-8")
    preflight_eval_text = (
        repo_root() / ".claude" / "skills" / "publishing" / "evals" / "publisher-preflight.md"
    ).read_text(encoding="utf-8")
    recovery_eval_text = (
        repo_root() / ".claude" / "skills" / "publishing" / "evals" / "publisher-recovery.md"
    ).read_text(encoding="utf-8")
    inquiry_eval_text = (
        repo_root() / ".claude" / "skills" / "publishing" / "evals" / "channel-name-inquiry.md"
    ).read_text(encoding="utf-8")

    for text in (publisher_text, guide_text, checklist_text):
        assert "channel-dispatch-plan" in text
        assert "PYPI_TOKEN" not in text
        assert "TEST_PYPI_TOKEN" not in text
        assert "sc-compose" not in text

    assert "role-specific background workers" in publisher_text
    assert "outcomes are keyed by channel" in (
        repo_root() / "docs" / "publish-kit-requirements.md"
    ).read_text(encoding="utf-8")
    assert '"status": "passed|failed|blocked"' in publisher_text
    assert '"passed|failed|blocked|required"' not in publisher_text
    assert '"required_checks": [{"kind": "<contract check not run>"' in publisher_text
    assert "`required_checks` lists contract checks deliberately not run" in publisher_text
    channel_protocol_text = (
        repo_root() / ".claude" / "agents" / "publisher-channel-protocol.md"
    ).read_text(encoding="utf-8")
    assert "never a check-result status" in channel_protocol_text
    assert '"required_checks": [{"kind": "<contract check not run>"' in channel_protocol_text
    assert '"success": false' in publisher_text
    assert "retain `data`" in publisher_text
    assert "Do not retry a `blocked` channel" in publisher_text
    assert "Retry only the channel" in publisher_text
    assert "Never ask whether a token exists" in publisher_text
    assert "preflight-secret-plan" in publisher_text
    assert "protected-environment secret metadata" in guide_text
    assert "version: 1.6.5" in publisher_text
    assert "closed-world:" in publisher_text
    assert "Never invent a tag, version, ref" in publisher_text
    assert "Emit an observed `checks` entry" in publisher_text
    assert "Send the assignment's named recipient" in publisher_text
    assert "### Synthetic-evaluation response checklist" in publisher_text
    assert "Every channel has `worker.role`, `worker.child_task_id`, and" in publisher_text
    assert "Send `team-lead`" not in publisher_text
    assert "named recipient" in publisher_text
    for template_text in (preflight_template_text, publish_template_text):
        assert "- recipient" in template_text
        assert "<recipient>{{ recipient }}</recipient>" in template_text
        assert "Send {{ recipient }}" in template_text
    assert publisher_text.count(
        '"data": {"tag": "v<VERSION>", "commit": "<COMMIT>", "channels": []}'
    ) == 2
    for eval_text in (preflight_eval_text, recovery_eval_text):
        normalized_eval_text = " ".join(eval_text.split())
        assert "evaluator/coordinator identity" in normalized_eval_text
        assert "not the evaluated publisher teammate" in normalized_eval_text
    assert "## Inputs" in publisher_text
    assert "## Output Format" in publisher_text
    assert "## Error Handling" in publisher_text
    assert "## Constraints" in publisher_text
    registry_text = (repo_root() / ".claude" / "agents" / "registry.yaml").read_text(
        encoding="utf-8"
    )
    contracts = tomllib.loads(
        (repo_root() / "release" / "publish-channel-contracts.toml.j2").read_text(
            encoding="utf-8"
        )
    )["channels"]
    assert 'publisher:\n    version: 1.6.5' in registry_text
    for channel_agent in (
        "crates-io-publisher",
        "pypi-publisher",
        "github-release-publisher",
        "homebrew-publisher",
        "winget-publisher",
        "scoop-publisher",
    ):
        assert f"{channel_agent}:" in registry_text
        agent_path = repo_root() / ".claude" / "agents" / f"{channel_agent}.md"
        assert agent_path.is_file()
        agent_text = agent_path.read_text(encoding="utf-8")
        assert "publisher-channel-protocol.md" in agent_text
        assert ".claude/skills/publishing/ref/channel-contracts.md" in agent_text
        assert "spawn_policy: background_agent_required" in agent_text
    assert not (repo_root() / ".claude" / "agents" / "publisher-channel-worker.md").exists()
    assert {
        contract["agent"] for contract in contracts.values()
    } == {
        "crates-io-publisher",
        "pypi-publisher",
        "github-release-publisher",
        "homebrew-publisher",
        "winget-publisher",
        "scoop-publisher",
    }
    assert contracts["pypi"]["environment_secrets"] == [
        {"environment": "pypi", "name": "PYPI_API_TOKEN"},
        {"environment": "testpypi", "name": "TEST_PYPI_API_TOKEN"},
    ]
    assert 'publishing:\n    version: 1.1.0' in registry_text
    assert "sole channel-contract source" in channel_contract_text
    assert "public-registry-inquiry-plan" in channel_contract_text
    assert "apparently_available" in channel_contract_text
    assert "not a reservation" in channel_contract_text
    assert "candidate-tag validation failure" in publisher_text
    assert "Use `PREFLIGHT.NOT_READY` as the top-level error code" in publisher_text
    assert "still launch one read-only" in publisher_text
    assert "result and child-task/result references. This is required live fanout" in publisher_text
    assert "union of\n`root_channels` and `post_release_channels` in `preflight-secret-plan`" in publisher_text
    assert "channel-dispatch-plan` alone contains only post-release work" in publisher_text
    assert '"child_task_id": "<background task>"' in publisher_text
    assert "simulated missing credential" in eval_plan_text
    assert "not create a tag" in eval_plan_text
    assert "## Goals" in eval_plan_text
    assert "## Expected Outcomes" in eval_plan_text
    assert "Haiku or Luna" in eval_plan_text
    assert "top-level\n  error remains `PREFLIGHT.NOT_READY`" in eval_plan_text
    assert "role-specific background worker" in eval_plan_text
    assert "background tasks, not teammates or panes" in eval_plan_text
    assert "full `sc-compose` ATM team member" in eval_plan_text
    assert "dedicated\n  tmux pane" in eval_plan_text
    assert "rmux claude publisher --team sc-compose --model haiku" in eval_plan_text
    assert "rmux codex publisher --team sc-compose --model luna" in eval_plan_text
    assert "ATM_IDENTITY=publisher" in eval_plan_text
    assert "ATM_TEAM=sc-compose" in eval_plan_text
    assert "configured\n  hooks" in eval_plan_text
    assert "post-run ATM questioning" in eval_plan_text
    assert "do not use\n`channel-dispatch-plan` as the worker inventory" in eval_plan_text
    assert "Every evaluation document must state:" in eval_convention_text
    assert ".claude/skills/publishing/ref/release-state-strategy.md" in publisher_text
    assert "ref/release-state-strategy.md" in publishing_skill_text
    assert "rmux claude publisher" in publishing_skill_text
    assert "rmux codex publisher" in publishing_skill_text
    assert "--team <team-name>" in publishing_skill_text
    assert "identity is exactly `publisher`" in publishing_skill_text
    assert "evals/publisher-preflight.md" in publishing_skill_text
    assert "evals/publisher-recovery.md" in publishing_skill_text
    assert "evals/channel-name-inquiry.md" in publishing_skill_text
    assert "role-specific background worker for the read-only inquiry" in publishing_skill_text
    assert "publish-channel-contracts.toml" in publishing_skill_text
    assert "Code only on `feature/*` or `fix/*`" in release_state_text
    assert "Code on `develop`" in release_state_text
    assert "Code on `main`" in release_state_text
    assert "Code on `release/*`" in release_state_text
    assert "final preflight on the exact `main` commit" in release_state_text
    assert "same authorized release ref and" in publisher_text
    assert "tag. It must read the full ordered manifest" in publisher_text
    assert "already-live" in publisher_text
    for template_text in (preflight_template_text, publish_template_text):
        assert 'assignee="publisher"' in template_text
        assert "release-state-strategy.md" in template_text
        assert "manifest_path" in template_text
    for eval_text in (preflight_eval_text, recovery_eval_text, inquiry_eval_text):
        assert "## Goal" in eval_text
        assert "## Expected outcomes" in eval_text
        assert "fresh" in eval_text
        assert "fenced JSON" in eval_text
        assert "must not" in eval_text
    for eval_text in (preflight_eval_text, recovery_eval_text):
        assert "manifest path" in eval_text
        assert "Do not hardcode a package" in eval_text
    for text in (
        publishing_skill_text,
        preflight_template_text,
        publish_template_text,
        preflight_eval_text,
        recovery_eval_text,
    ):
        assert "sc-compose" not in text


def test_publishing_task_templates_render_recipient_contract(tmp_path: Path) -> None:
    if renderer_binary() is None:
        pytest.skip("consumer does not include a sc-compose renderer workspace")
    cases = (
        (
            ".claude/skills/publishing/preflight.xml.j2",
            {
                "task_id": "EVAL-PREFLIGHT",
                "recipient": "evaluator-preflight",
                "release_version": "1.4.2",
                "candidate_ref": "develop",
                "candidate_commit": "deadbeef",
                "starting_state": "develop",
                "preflight_stage": "readiness",
                "worktree_path": "/tmp/eval",
                "branch": "develop",
                "manifest_path": "release/publish-artifacts.toml",
            },
        ),
        (
            ".claude/skills/publishing/publish.xml.j2",
            {
                "task_id": "EVAL-RECOVERY",
                "recipient": "evaluator-recovery",
                "release_version": "1.4.2",
                "release_ref": "refs/tags/v1.4.2",
                "release_commit": "deadbeef",
                "operation": "retry-failed-channels",
                "failed_channels": "crates_io",
                "worktree_path": "/tmp/eval",
                "manifest_path": "release/publish-artifacts.toml",
            },
        ),
    )

    for template_path, context in cases:
        rendered = render_release_template(tmp_path, template_path, context)
        root = ET.fromstring(rendered)

        assert root.findtext("recipient") == context["recipient"]
        assert f"Send {context['recipient']}" in rendered


def test_release_preflight_collects_independent_failures_before_denial() -> None:
    preflight_text = (repo_root() / ".github" / "workflows" / "release-preflight.yml").read_text(
        encoding="utf-8"
    )

    assert "Deny release after complete preflight summary" in preflight_text
    assert "channel_preflight_results" in preflight_text
    assert "channel-preflight-results" in preflight_text
    assert "Emit manifest-derived per-channel preflight results" in preflight_text
    assert "Preflight complete: failed=[%s] blocked=[%s]" in preflight_text
    assert preflight_text.count("continue-on-error: true") >= 12
    assert "failures=()" in preflight_text
    assert "steps.secret_plan.outcome == 'success'" in preflight_text
    assert "Verify registry versions and new names" in preflight_text
    assert "public-registry-check-plan" in preflight_text
    assert "REGISTRY_STATE" in preflight_text


def test_release_workflow_collects_wheels_without_redundant_zip_sweep() -> None:
    text = release_workflow_text()

    assert (
        "find artifacts -type f \\( -name '*.tar.gz' -o -name '*.zip' \\) -exec mv {} release/ \\;"
        in text
    )
    assert "find artifacts -type f -name '*.whl' -exec mv {} release/ \\;" in text
    assert "find artifacts -type f \\( -name '*.zip' -o -name '*.whl' \\)" not in text


def test_release_workflow_rehearsal_mode_avoids_production_side_effects() -> None:
    text = release_workflow_text()

    assert 'echo "Rehearsal mode: validating release tag ${tag} locally only; not pushing any tag to origin"' in text
    assert "echo \"release_ref=$main_sha\" >> \"$GITHUB_OUTPUT\"" in text
    assert "needs.gate-and-tag.outputs.release_target == 'production'" in text


def test_release_workflow_checks_out_repo_before_local_python_setup_action() -> None:
    text = release_workflow_text()

    wheels_job = """  build-python-wheels:
    if: ${{ needs.release-plan.outputs.has_python_wheels == 'true' }}
    needs: [gate-and-tag, release-plan]
    strategy:
      fail-fast: false
      matrix: ${{ fromJSON(needs.release-plan.outputs.python_wheel_matrix) }}
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{ needs.gate-and-tag.outputs.release_ref }}
      - uses: ./.github/actions/setup-python-release-build"""
    sdist_job = """  build-python-sdists:
    if: ${{ needs.release-plan.outputs.has_python_sdists == 'true' }}
    needs: [gate-and-tag, release-plan]
    strategy:
      fail-fast: false
      matrix: ${{ fromJSON(needs.release-plan.outputs.python_sdist_matrix) }}
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{ needs.gate-and-tag.outputs.release_ref }}
      - uses: ./.github/actions/setup-python-release-build"""

    assert wheels_job in text
    assert sdist_job in text
    assert "matrix.cargo_manifest" in text
    assert "matrix.pyproject" in text


def test_python_package_metadata_uses_local_readme_for_sdist() -> None:
    if not (repo_root() / "bindings" / "python").is_dir():
        pytest.skip("consumer does not include a Python binding")
    pyproject_text = python_pyproject_text()
    cargo_toml_text = python_cargo_toml_text()

    assert 'readme = "README.md"' in pyproject_text
    assert 'readme = "README.md"' in cargo_toml_text
    assert "../../README.md" not in pyproject_text
    assert "../../README.md" not in cargo_toml_text


def write_readme_fixture(
    tmp_path: Path,
    *,
    dependency_version: str,
    status_version: str,
    stability_minor: str,
    dependency_crate: str = "sc-composer",
) -> tuple[Path, Path, Path]:
    workspace = tmp_path / "Cargo.toml"
    workspace.write_text(
        "\n".join(["[workspace.package]", 'version = "1.2.0"', ""]),
        encoding="utf-8",
    )
    readme = tmp_path / "README.md"
    readme.write_text(
        "\n".join(
            [
                "## Library usage",
                "",
                "```toml",
                "[dependencies]",
                f'{dependency_crate} = "{dependency_version}"',
                "```",
                "",
                "## Status",
                "",
                "| | |",
                "|-|-|",
                f"| Version | {status_version} |",
                "| MSRV | Rust 1.94.1 |",
                f"| Stability | stable {stability_minor} release line |",
                "",
            ]
        ),
        encoding="utf-8",
    )
    manifest = tmp_path / "release" / "publish-artifacts.toml"
    manifest.parent.mkdir(parents=True)
    manifest.write_text(
        "\n".join(
            [
                "[project]",
                f'readme_dependency_crate = "{dependency_crate}"',
                "",
                "[[crates]]",
                'artifact = "readme-dependency"',
                f'package = "{dependency_crate}"',
                'cargo_toml = "crates/readme-dependency/Cargo.toml"',
                "publish_order = 1",
                "wait_after_publish_seconds = 0",
                "",
            ]
        ),
        encoding="utf-8",
    )
    return workspace, readme, manifest


def run_sync_readme_version(
    workspace: Path, readme: Path, manifest: Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            "scripts/release_artifacts.py",
            "sync-readme-version",
            "--manifest",
            str(manifest),
            "--workspace-toml",
            str(workspace),
            "--readme",
            str(readme),
        ],
        cwd=Path(__file__).resolve().parents[2],
        text=True,
        capture_output=True,
        check=False,
    )


def run_verify_readme_version(
    workspace: Path, readme: Path, manifest: Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            "scripts/release_artifacts.py",
            "verify-readme-version",
            "--manifest",
            str(manifest),
            "--workspace-toml",
            str(workspace),
            "--readme",
            str(readme),
        ],
        cwd=Path(__file__).resolve().parents[2],
        text=True,
        capture_output=True,
        check=False,
    )


def write_version_lockstep_fixture(
    tmp_path: Path,
    *,
    python_version: str = "1.4.0",
    crate_inherits_workspace_version: bool = True,
) -> tuple[Path, Path]:
    workspace = tmp_path / "Cargo.toml"
    workspace.write_text(
        "[workspace.package]\nversion = \"1.4.0\"\n",
        encoding="utf-8",
    )
    for relative_path in (
        "crates/sc-sha/Cargo.toml",
        "crates/sc-composer/Cargo.toml",
        "crates/sc-compose/Cargo.toml",
        "bindings/python/Cargo.toml",
        "bindings/sc-sha-python/Cargo.toml",
    ):
        path = tmp_path / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        version = "version.workspace = true" if crate_inherits_workspace_version else 'version = "1.3.1"'
        path.write_text(
            "[package]\nname = \"fixture\"\n" + version + "\n",
            encoding="utf-8",
        )
    for relative_path in (
        "bindings/python/pyproject.toml",
        "bindings/sc-sha-python/pyproject.toml",
    ):
        path = tmp_path / relative_path
        path.write_text(
            f'[project]\nname = "fixture"\nversion = "{python_version}"\n',
            encoding="utf-8",
        )
    manifest = tmp_path / "release" / "publish-artifacts.toml"
    manifest.parent.mkdir(parents=True, exist_ok=True)
    manifest.write_text(
        "\n".join(
            [
                "[[crates]]",
                'artifact = "sc-sha"',
                'package = "fixture"',
                'cargo_toml = "crates/sc-sha/Cargo.toml"',
                "publish_order = 1",
                "wait_after_publish_seconds = 0",
                "",
                "[[python_packages]]",
                'artifact = "sc-compose-python"',
                'package = "fixture"',
                'manifest = "bindings/python/pyproject.toml"',
                'module = "fixture"',
                'publish = "pypi"',
                "",
                "[[python_packages]]",
                'artifact = "sc-sha-python"',
                'package = "fixture-sha"',
                'manifest = "bindings/sc-sha-python/pyproject.toml"',
                'module = "fixture_sha"',
                'publish = "pypi"',
                "",
                "[[python_distributions]]",
                'name = "fixture"',
                'source = "bindings/python"',
                'cargo_manifest = "bindings/python/Cargo.toml"',
                "sdist = true",
                'wheels = ["ubuntu-latest"]',
                "",
                "[[python_distributions]]",
                'name = "fixture-sha"',
                'source = "bindings/sc-sha-python"',
                'cargo_manifest = "bindings/sc-sha-python/Cargo.toml"',
                "sdist = true",
                'wheels = ["ubuntu-latest"]',
                "",
            ]
        ),
        encoding="utf-8",
    )
    return workspace, manifest


def run_verify_version_lockstep(workspace: Path, manifest: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            "scripts/release_artifacts.py",
            "verify-version-lockstep",
            "--manifest",
            str(manifest),
            "--workspace-toml",
            str(workspace),
        ],
        cwd=Path(__file__).resolve().parents[2],
        text=True,
        capture_output=True,
        check=False,
    )


def test_verify_readme_version_passes_when_readme_matches_workspace(tmp_path: Path) -> None:
    workspace, readme, manifest = write_readme_fixture(
        tmp_path, dependency_version="1.2.0", status_version="1.2.0", stability_minor="1.2"
    )

    result = run_verify_readme_version(workspace, readme, manifest)

    assert result.returncode == 0, result.stderr
    assert "readme version verification passed" in result.stdout


def test_verify_readme_version_rejects_stale_dependency_example(tmp_path: Path) -> None:
    workspace, readme, manifest = write_readme_fixture(
        tmp_path, dependency_version="1.1.0", status_version="1.2.0", stability_minor="1.2"
    )

    result = run_verify_readme_version(workspace, readme, manifest)

    assert result.returncode != 0
    assert "sc-composer dependency example" in result.stderr


def test_verify_readme_version_rejects_stale_status_table(tmp_path: Path) -> None:
    workspace, readme, manifest = write_readme_fixture(
        tmp_path, dependency_version="1.2.0", status_version="1.1.0", stability_minor="1.1"
    )

    result = run_verify_readme_version(workspace, readme, manifest)

    assert result.returncode != 0
    assert "Status table Version row" in result.stderr
    assert "Status table Stability row" in result.stderr


def test_sync_readme_version_rewrites_stale_references(tmp_path: Path) -> None:
    workspace, readme, manifest = write_readme_fixture(
        tmp_path, dependency_version="1.1.0", status_version="1.1.0", stability_minor="1.1"
    )

    sync_result = run_sync_readme_version(workspace, readme, manifest)

    assert sync_result.returncode == 0, sync_result.stderr
    assert "synced 3 readme version reference(s) to 1.2.0" in sync_result.stdout

    verify_result = run_verify_readme_version(workspace, readme, manifest)
    assert verify_result.returncode == 0, verify_result.stderr


def test_readme_version_commands_use_the_manifest_declared_dependency_crate(
    tmp_path: Path,
) -> None:
    workspace, readme, manifest = write_readme_fixture(
        tmp_path,
        dependency_crate="fixture-composer",
        dependency_version="1.1.0",
        status_version="1.2.0",
        stability_minor="1.2",
    )

    sync_result = run_sync_readme_version(workspace, readme, manifest)

    assert sync_result.returncode == 0, sync_result.stderr
    assert 'fixture-composer = "1.2.0"' in readme.read_text(encoding="utf-8")
    verify_result = run_verify_readme_version(workspace, readme, manifest)
    assert verify_result.returncode == 0, verify_result.stderr


def test_verify_version_lockstep_accepts_all_release_version_sources(tmp_path: Path) -> None:
    result = run_verify_version_lockstep(*write_version_lockstep_fixture(tmp_path))

    assert result.returncode == 0, result.stderr
    assert "version lockstep verification passed" in result.stdout


def test_verify_version_lockstep_rejects_non_inherited_crate_version(tmp_path: Path) -> None:
    result = run_verify_version_lockstep(
        *write_version_lockstep_fixture(tmp_path, crate_inherits_workspace_version=False)
    )

    assert result.returncode != 0
    assert "crates/sc-sha/Cargo.toml" in result.stderr
    assert "must inherit workspace.package.version" in result.stderr


def test_verify_version_lockstep_rejects_python_package_drift(tmp_path: Path) -> None:
    result = run_verify_version_lockstep(
        *write_version_lockstep_fixture(tmp_path, python_version="1.3.1")
    )

    assert result.returncode != 0
    assert "bindings/python/pyproject.toml" in result.stderr
    assert "[project].version mismatch" in result.stderr


def test_build_plan_selects_the_manifest_declared_python_upload_tool(tmp_path: Path) -> None:
    maturin_dir = tmp_path / "maturin"
    maturin_dir.mkdir()
    write_repo_fixture(maturin_dir, manifest_wheels=["ubuntu-latest"])
    plan = run_fixture_command(
        maturin_dir, "build-plan", manifest=maturin_dir / "release" / "publish-artifacts.toml"
    )
    assert plan.returncode == 0, plan.stderr
    assert json.loads(plan.stdout)["python_upload_tool"] == "maturin"

    setuptools_dir = tmp_path / "setuptools"
    setuptools_dir.mkdir()
    write_repo_fixture(
        setuptools_dir, manifest_wheels=["ubuntu-latest"], python_build_system="setuptools"
    )
    plan = run_fixture_command(
        setuptools_dir,
        "build-plan",
        manifest=setuptools_dir / "release" / "publish-artifacts.toml",
    )
    assert plan.returncode == 0, plan.stderr
    assert json.loads(plan.stdout)["python_upload_tool"] == "twine"

    rust_only_dir = tmp_path / "rust-only"
    rust_only_dir.mkdir()
    write_repo_fixture(rust_only_dir, manifest_wheels=["ubuntu-latest"], include_python=False)
    plan = run_fixture_command(
        rust_only_dir,
        "build-plan",
        manifest=rust_only_dir / "release" / "publish-artifacts.toml",
    )
    assert plan.returncode == 0, plan.stderr
    assert json.loads(plan.stdout)["python_upload_tool"] == ""


def test_pypi_workflows_branch_uploads_on_the_declared_build_system() -> None:
    pypi_text = pypi_publish_workflow_text()

    # The uploader is manifest-derived (build-plan), not hardcoded to maturin.
    assert "build-plan" in pypi_text
    assert "python_upload_tool: ${{ steps.config.outputs.python_upload_tool }}" in pypi_text
    assert "if: ${{ needs.verify-release.outputs.python_upload_tool == 'maturin' }}" in pypi_text
    assert "if: ${{ needs.verify-release.outputs.python_upload_tool == 'twine' }}" in pypi_text
    assert "python -m pip install twine==6.1.0" in pypi_text
    assert "name: Publish manifest-declared wheels and sdists to TestPyPI with twine" in pypi_text
    assert "name: Publish manifest-declared wheels and sdists to PyPI with twine" in pypi_text
    assert (
        'python -m twine upload --repository "${PYPI_REPOSITORY}" --skip-existing dist/*.whl dist/*.tar.gz'
        in pypi_text
    )
    assert "TWINE_USERNAME: __token__" in pypi_text
    assert "TWINE_PASSWORD: ${{ secrets.TEST_PYPI_API_TOKEN }}" in pypi_text
    assert "TWINE_PASSWORD: ${{ secrets.PYPI_API_TOKEN }}" in pypi_text
    # No unconditional maturin install remains.
    assert "\n      - name: Install maturin\n        run:" not in pypi_text

    # The TestPyPI rehearsal leg in release.yml takes the same manifest branch.
    release_text = release_workflow_text()
    assert "python_upload_tool: ${{ steps.manifest.outputs.python_upload_tool }}" in release_text
    assert "if: ${{ needs.release-plan.outputs.python_upload_tool == 'maturin' }}" in release_text
    assert "if: ${{ needs.release-plan.outputs.python_upload_tool == 'twine' }}" in release_text
    assert "name: Publish wheels and sdist to TestPyPI with twine" in release_text
    assert (
        "python -m twine upload --repository testpypi --skip-existing dist/*.whl dist/*.tar.gz"
        in release_text
    )


def test_python_version_steps_use_the_manifest_declared_workspace_toml() -> None:
    action_text = (
        repo_root() / ".github" / "actions" / "setup-python-release-build" / "action.yml"
    ).read_text(encoding="utf-8")
    release_text = release_workflow_text()

    assert "--workspace-toml '${{ inputs.workspace_toml }}'" in action_text
    assert "--workspace-toml Cargo.toml" not in action_text
    assert 'default: "Cargo.toml"' in action_text
    assert "workspace_toml: ${{ steps.manifest.outputs.workspace_toml }}" in release_text
    assert release_text.count(
        "workspace_toml: ${{ needs.release-plan.outputs.workspace_toml }}"
    ) == 2


def test_version_resolution_honors_a_pyproject_workspace_toml(tmp_path: Path) -> None:
    """A pure-Python consumer resolves its version without any Cargo.toml."""
    _, manifest = write_repo_fixture(
        tmp_path,
        manifest_wheels=["ubuntu-latest"],
        include_crates=False,
        python_build_system="setuptools",
    )
    version_source = tmp_path / "pyproject.toml"
    version_source.write_text(
        '[project]\nname = "fixture-root"\nversion = "1.1.0"\n', encoding="utf-8"
    )

    verify = run_fixture_command(
        tmp_path,
        "verify-version",
        "--workspace-toml",
        str(version_source),
        "--version",
        "1.1.0",
        manifest=manifest,
    )
    assert verify.returncode == 0, verify.stderr
    assert "version verification passed" in verify.stdout

    lockstep = run_fixture_command(
        tmp_path,
        "verify-version-lockstep",
        "--workspace-toml",
        str(version_source),
        manifest=manifest,
    )
    assert lockstep.returncode == 0, lockstep.stderr

    mismatch = run_fixture_command(
        tmp_path,
        "verify-version",
        "--workspace-toml",
        str(version_source),
        "--version",
        "1.2.0",
        manifest=manifest,
    )
    assert mismatch.returncode != 0
    assert "workspace version mismatch" in mismatch.stderr

    empty_source = tmp_path / "empty.toml"
    empty_source.write_text("", encoding="utf-8")
    unresolved = run_fixture_command(
        tmp_path,
        "verify-version",
        "--workspace-toml",
        str(empty_source),
        "--version",
        "1.1.0",
        manifest=manifest,
    )
    assert unresolved.returncode != 0
    assert "version source must declare" in unresolved.stderr
