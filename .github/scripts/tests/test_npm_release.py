"""Nonpublishing npm contract, artifact, and retry tests (all writes mocked)."""
import argparse
import base64
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import urllib.error
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import npm_release as npm
from release_manifest import _public_registry_checks, load_channel_contracts
from test_install import INSTALL, InstallValuesTests


@pytest.fixture
def release(tmp_path):
    manifest = {"npm_packages": [{"name": "@example/client", "source": "bindings/typescript"}], "channels": {"npm": {"workflow": "npm-publish.yml", "dispatch_inputs": {}}}}
    path = tmp_path / "example-client-1.2.3.tgz"
    data = json.dumps({"name": "@example/client", "version": "1.2.3"}).encode()
    with tarfile.open(path, "w:gz") as archive:
        member = tarfile.TarInfo("package/package.json")
        member.size = len(data)
        archive.addfile(member, io.BytesIO(data))
    (tmp_path / "checksums.txt").write_text(hashlib.sha256(path.read_bytes()).hexdigest() + "  " + path.name + "\n")
    return manifest, tmp_path, path


def existing(path):
    return {"name": "@example/client", "version": "1.2.3", "dist": {"integrity": "sha512-" + base64.b64encode(hashlib.sha512(path.read_bytes()).digest()).decode()}}


def test_installer_npm_roundtrip(tmp_path):
    values = InstallValuesTests.valid_values()
    values["npm_packages"] = [{"name": "@example/client", "source": "bindings/typescript"}]
    values["channels"]["npm"] = {"workflow": "npm-publish.yml", "dispatch_inputs": {"dry_run": "false"}}
    input_path = tmp_path / "input.json"
    input_path.write_text(json.dumps(values))
    loaded = INSTALL.load_install_values(input_path)
    assert INSTALL.template_values(loaded)["has_channel_npm"] is True
    import tomllib
    output = tmp_path / "manifest.toml"
    INSTALL.render_template(Path("release/publish-artifacts.toml.j2"), loaded, output)
    rendered = tomllib.loads(output.read_text())
    assert rendered["npm_packages"] == values["npm_packages"]
    assert rendered["channels"]["npm"] == values["channels"]["npm"]


def test_channel_scope_and_scoped_urls():
    contracts = load_channel_contracts(INSTALL.PACKAGE_ROOT / "release/publish-channel-contracts.toml.j2")
    assert contracts["npm"]["environment_secrets"] == [{"environment": "npm", "name": "NPM_TOKEN"}]
    check, = _public_registry_checks(contracts, "npm", "@example/client", "1.2.3")
    assert check["version_lookup_url"] == "https://registry.npmjs.org/%40example%2Fclient/1.2.3"


def test_verify_metadata_and_checksum(release):
    manifest, directory, path = release
    assert npm.verify(manifest, "v1.2.3", directory) == [("@example/client", path)]
    path.write_bytes(path.read_bytes() + b"tampered")
    with pytest.raises(ValueError, match="checksum"):
        npm.verify(manifest, "v1.2.3", directory)


def test_mismatched_archive_identity(release):
    _, _, path = release
    with pytest.raises(ValueError, match="identity"):
        npm.check_metadata(path, "@other/client", "1.2.3")


def test_preflight_never_publishes(release):
    manifest, directory, _ = release
    with patch.object(npm, "registry_version", return_value=None), patch.object(npm.subprocess, "run") as run:
        npm.publish(manifest, "v1.2.3", directory)
    run.assert_not_called()


def test_identical_version_skips(release):
    manifest, directory, path = release
    with patch.object(npm, "registry_version", return_value=existing(path)), patch.object(npm.subprocess, "run") as run:
        npm.publish(manifest, "v1.2.3", directory, dry_run=False)
    run.assert_not_called()


def test_conflicting_version_fails_closed(release):
    manifest, directory, _ = release
    with patch.object(npm, "registry_version", return_value={"dist": {"integrity": "other"}}), patch.object(npm.subprocess, "run") as run:
        with pytest.raises(ValueError, match="differs"):
            npm.publish(manifest, "v1.2.3", directory, dry_run=False)
    run.assert_not_called()


def test_publish_only_tarball_and_disable_scripts(release):
    manifest, directory, path = release
    with patch.object(npm, "registry_version", return_value=None), patch.object(npm.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
        npm.publish(manifest, "v1.2.3", directory, dry_run=False)
    command = run.call_args.args[0]
    assert command[:3] == ["npm", "publish", str(path.resolve())]
    assert "--ignore-scripts" in command
    assert command[-2:] == ["--tag", "latest"]


@pytest.mark.parametrize("accepted", [True, False])
def test_failed_publish_rechecks_registry_for_retry(release, accepted):
    manifest, directory, path = release
    with patch.object(npm, "registry_version", side_effect=[None, existing(path) if accepted else None]), patch.object(npm.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)):
        if accepted:
            npm.publish(manifest, "v1.2.3", directory, dry_run=False)
        else:
            with pytest.raises(RuntimeError, match="retry"):
                npm.publish(manifest, "v1.2.3", directory, dry_run=False)


def test_failed_publish_preserves_redacted_diagnostic(release):
    manifest, directory, _ = release
    failed = subprocess.CompletedProcess([], 1, stdout="", stderr="upstream denied token=synthetic-secret Bearer bearer-secret Authorization: Bearer auth-secret")
    with patch.object(npm, "registry_version", side_effect=[None, None]), patch.object(npm.subprocess, "run", return_value=failed):
        with pytest.raises(RuntimeError) as error:
            npm.publish(manifest, "v1.2.3", directory, dry_run=False)
    message = str(error.value)
    assert "NPM.PUBLISH_FAILED" in message
    assert "upstream denied" in message
    assert "synthetic-secret" not in message
    assert "bearer-secret" not in message
    assert "auth-secret" not in message
    assert "Bearer <redacted>" in message
    assert "Authorization=<redacted>" in message


def test_failed_publish_preserves_stdout_and_stderr(release):
    manifest, directory, _ = release
    failed = subprocess.CompletedProcess([], 1, stdout="upstream response body", stderr="cli warning")
    with patch.object(npm, "registry_version", side_effect=[None, None]), patch.object(npm.subprocess, "run", return_value=failed):
        with pytest.raises(RuntimeError) as error:
            npm.publish(manifest, "v1.2.3", directory, dry_run=False)
    message = str(error.value)
    assert "upstream response body" in message
    assert "cli warning" in message


def test_failed_publish_preserves_full_diagnostic_without_truncation(release):
    manifest, directory, _ = release
    detail = "upstream diagnostic " + ("x" * 5000)
    failed = subprocess.CompletedProcess([], 1, stdout="", stderr=detail)
    with patch.object(npm, "registry_version", side_effect=[None, None]), patch.object(npm.subprocess, "run", return_value=failed):
        with pytest.raises(RuntimeError) as error:
            npm.publish(manifest, "v1.2.3", directory, dry_run=False)
    assert detail in str(error.value)


def test_failed_publish_preserves_npm_scope_not_found_diagnostic(release):
    manifest, directory, _ = release
    failed = subprocess.CompletedProcess(
        [], 1, stdout="",
        stderr="npm error code E404\\nnpm error 404 PUT https://registry.npmjs.org/@sc-observability%2fclient - Scope not found",
    )
    with patch.object(npm, "registry_version", side_effect=[None, None]), patch.object(npm.subprocess, "run", return_value=failed):
        with pytest.raises(RuntimeError) as error:
            npm.publish(manifest, "v1.2.3", directory, dry_run=False)
    message = str(error.value)
    assert "E404" in message
    assert "Scope not found" in message
    assert "registry.npmjs.org/@sc-observability%2fclient" in message


@pytest.mark.parametrize("code", [401, 403, 429, 500])
def test_registry_errors_not_absence(code):
    error = urllib.error.HTTPError("https://registry.npmjs.org", code, "error", {}, None)
    with patch.object(npm.urllib.request, "urlopen", side_effect=error):
        with pytest.raises(RuntimeError, match="indeterminate"):
            npm.registry_version("@example/client", "1.2.3")


def test_registry_404_is_absent():
    error = urllib.error.HTTPError("https://registry.npmjs.org", 404, "missing", {}, None)
    with patch.object(npm.urllib.request, "urlopen", side_effect=error):
        assert npm.registry_version("@example/client", "1.2.3") is None


def test_unsafe_source_rejected():
    with pytest.raises(ValueError, match="unsafe"):
        npm.packages({"npm_packages": [{"name": "example", "source": "../escape"}], "channels": {"npm": {}}})


def test_build_uses_lockfile_and_never_publishes(release, monkeypatch):
    manifest, directory, path = release
    monkeypatch.chdir(directory)
    source = directory / "bindings/typescript"
    source.mkdir(parents=True)
    (source / "package.json").write_text(json.dumps({"name": "@example/client", "version": "1.2.3"}))
    with patch.object(npm.subprocess, "run") as run:
        npm.build(manifest, "v1.2.3", directory)
    assert [call.args[0][:2] for call in run.call_args_list] == [["npm", "ci"], ["npm", "run"], ["npm", "pack"]]
    assert "--ignore-scripts" in run.call_args_list[-1].args[0]


def test_version_mismatch_fails_before_build(release, monkeypatch):
    manifest, directory, _ = release
    monkeypatch.chdir(directory)
    source = directory / "bindings/typescript"
    source.mkdir(parents=True)
    (source / "package.json").write_text(json.dumps({"name": "@example/client", "version": "0.0.1"}))
    with patch.object(npm.subprocess, "run") as run:
        with pytest.raises(ValueError, match="source npm"):
            npm.build(manifest, "v1.2.3", directory)
    run.assert_not_called()


def test_all_archives_checked_before_first_publication(release):
    manifest, directory, _ = release
    manifest["npm_packages"].append({"name": "other", "source": "other"})
    with patch.object(npm.subprocess, "run") as run, patch.object(npm, "registry_version") as lookup:
        with pytest.raises(ValueError, match="missing regular"):
            npm.publish(manifest, "v1.2.3", directory, dry_run=False)
    run.assert_not_called()
    lookup.assert_not_called()


def test_tarball_slug_collision_rejected():
    manifest = {"npm_packages": [{"name": "@example/client", "source": "a"}, {"name": "example-client", "source": "b"}], "channels": {"npm": {}}}
    with pytest.raises(ValueError, match="duplicate"):
        npm.packages(manifest)


def test_workflow_credential_and_artifact_contract():
    import yaml
    root = INSTALL.PACKAGE_ROOT / ".github"
    workflow = yaml.safe_load((root / "workflows/npm-publish.yml").read_text())
    job = workflow["jobs"]["publish"]
    assert job["environment"] == "npm"
    assert workflow["permissions"] == {"contents": "read"}
    credential_steps = [step for step in job["steps"] if "NPM_TOKEN" in str(step)]
    assert len(credential_steps) == 1
    assert credential_steps[0]["if"] == "${{ !inputs.dry_run }}"
    assert "npm_release.py publish" in credential_steps[0]["run"]
    download = next(step for step in job["steps"] if step.get("name") == "Download immutable release assets")
    assert '.immutable == true' in download["run"]
    assert "checksums.txt" in download["run"]
    assert all("npm ci" not in step.get("run", "") for step in job["steps"])


def test_node24_runtime_floors():
    import re
    floors = {"checkout": 5, "setup-python": 6, "setup-node": 6, "cache": 5, "upload-artifact": 6, "download-artifact": 7}
    # Consumers retain independent workflows/actions outside the vendored kit.
    # The source inventory guard in test_release_artifacts keeps these shared
    # inventories complete without imposing kit policy on caller-owned CI.
    workflows = ("release", "release-candidate", "release-preflight", "crates-publish",
                 "pypi-publish", "npm-publish", "homebrew-publish", "scoop-publish", "winget-publish")
    actions = ("extract-published-renderer", "install-linux-native-deps", "setup-lint-toolchain",
               "setup-python-release-build", "setup-renderer", "setup-sc-lint", "verify-published-release")
    github = INSTALL.PACKAGE_ROOT / ".github"
    paths = [*(github / "workflows" / f"{name}.yml" for name in workflows),
             *(github / "actions" / name / "action.yml" for name in actions)]
    for path in paths:
        for name, major in re.findall(r"uses:\s*actions/([\w-]+)@v(\d+)", path.read_text()):
            assert int(major) >= floors[name], str(path)
    assert "softprops/action-gh-release@v3" in (INSTALL.PACKAGE_ROOT / ".github/workflows/release.yml").read_text()


@pytest.mark.parametrize('immutable,expected', [(True, 0), (False, 1)])
def test_download_step_requires_immutable_release(tmp_path, immutable, expected):
    import os
    import yaml
    workflow = yaml.safe_load((INSTALL.PACKAGE_ROOT / '.github/workflows/npm-publish.yml').read_text())
    step = next(step for step in workflow['jobs']['publish']['steps'] if step.get('name') == 'Download immutable release assets')
    gh = tmp_path / 'gh'
    gh.write_text('#!/bin/sh\nif [ "$1" = api ]; then\n  printf \'%s\\n\' \'{"immutable":' + str(immutable).lower() + ',"draft":false}\'\nelse\n  touch downloaded\nfi\n')
    gh.chmod(0o755)
    env = {**os.environ, 'PATH': str(tmp_path) + os.pathsep + os.environ['PATH'], 'RELEASE_TAG': 'v1.2.3', 'RELEASE_REPOSITORY': 'example/project'}
    result = subprocess.run(['bash', '-c', step['run']], cwd=tmp_path, env=env, capture_output=True)
    assert result.returncode == expected
    assert (tmp_path / 'downloaded').exists() is immutable


@pytest.mark.parametrize('override', [{'version':'0.0.1'}, {'name':'other'}, {'private':True}, {'publishConfig':{'access':'restricted'}}, {'publishConfig':{'registry':'https://other.invalid'}}])
def test_lockstep_blocks_unsuitable_npm_sources_before_tag(tmp_path, override):
    source = tmp_path / 'bindings/client'
    source.mkdir(parents=True)
    (source / 'package.json').write_text(json.dumps({'name':'@example/client','version':'1.2.3',**override}))
    (tmp_path / 'Cargo.toml').write_text('[workspace.package]\nversion="1.2.3"\n')
    manifest = tmp_path / 'publish.toml'
    manifest.write_text('[[npm_packages]]\nname="@example/client"\nsource="bindings/client"\n[channels.npm]\nworkflow="npm-publish.yml"\ndispatch_inputs={}\n')
    result = subprocess.run([sys.executable,str(INSTALL.PACKAGE_ROOT/'.github/scripts/release_artifacts.py'),'verify-version-lockstep','--manifest',str(manifest),'--workspace-toml',str(tmp_path/'Cargo.toml')],cwd=tmp_path,capture_output=True,text=True)
    assert result.returncode != 0
    assert 'source npm package' in result.stderr


def test_lockstep_accepts_public_matching_npm_source(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path/'package.json').write_text(json.dumps({'name':'example','version':'1.2.3','private':False}))
    npm.validate_sources({'npm_packages':[{'name':'example','source':'.'}],'channels':{'npm':{}}},'1.2.3')


def test_exact_release_commit_is_checked_instead_of_dispatch_checkout():
    manifest = '[[npm_packages]]\nname="example"\nsource="client"\n[channels.npm]\nworkflow="npm-publish.yml"\ndispatch_inputs={}\n'
    sha = 'a' * 40
    with patch.object(npm.subprocess,'check_output',side_effect=[manifest,json.dumps({'name':'example','version':'0.0.1'})]) as git:
        with pytest.raises(ValueError,match='identity/version/private'):
            npm.check_release_source('release/publish-artifacts.toml','v1.2.3',sha)
    assert git.call_args_list[0].args[0] == ['git','show',sha+':release/publish-artifacts.toml']
    assert git.call_args_list[1].args[0] == ['git','show',sha+':client/package.json']


def test_npm_checks_precede_tag_creation_and_registry_jobs():
    import yaml
    workflow = yaml.safe_load((INSTALL.PACKAGE_ROOT/'.github/workflows/release.yml').read_text())
    steps = workflow['jobs']['gate-and-tag']['steps']
    lockstep = next(i for i,step in enumerate(steps) if 'verify-version-lockstep' in step.get('run',''))
    tag_index = next(i for i,step in enumerate(steps) if step.get('id') == 'release-ref')
    assert lockstep < tag_index
    script = steps[tag_index]['run']
    exact_source = next(i for i,step in enumerate(steps) if 'npm_release.py check-source' in step.get('run',''))
    assert lockstep < exact_source < tag_index
    assert steps[exact_source]['env']['RELEASE_SHA'] == '${{ steps.release_gate.outputs.release_sha }}'
    assert '--source-ref "$RELEASE_SHA"' in steps[exact_source]['run']
    assert script.index('git tag "$tag"') < script.index('git push origin "$tag"')
    assert 'continue-on-error' not in steps[exact_source]
    for name in ['build-npm','publish']:
        needs = workflow['jobs'][name]['needs']
        assert needs == 'gate-and-tag' or 'gate-and-tag' in needs


@pytest.mark.parametrize("secret", [
    "//registry.npmjs.org/:_authToken=SYNTHETIC_SECRET",
    "https://user:SYNTHETIC_SECRET@registry.npmjs.org/pkg",
    '{"authToken":"SYNTHETIC_SECRET"}',
])
def test_npm_failure_is_a_complete_fenced_worker_result(release, secret):
    from worker_result import parse_fenced_result
    manifest, directory, _ = release
    failed = subprocess.CompletedProcess([], 1, stdout=secret, stderr="E403 publish denied")
    with patch.object(npm, "registry_version", side_effect=[None, None]), patch.object(npm.subprocess, "run", return_value=failed):
        with pytest.raises(npm.NpmPublicationError) as error:
            npm.publish(manifest, "v1.2.3", directory, dry_run=False)
    report = parse_fenced_result(str(error.value))
    assert report["channel"] == "npm"
    assert report["exit_status"] == 1
    assert report["checks"] == [{"kind": "npm_publish", "status": "failed"}]
    assert report["required_checks"] == []
    assert "E403 publish denied" in report["sanitized_diagnostic"]
    assert "SYNTHETIC_SECRET" not in str(error.value)
