"""Nonpublishing regressions for credential probes and unpublished Cargo dependencies."""
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import urllib.error
from unittest.mock import MagicMock

import pytest
import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release_credentials as credentials

ROOT = next(p for p in Path(__file__).resolve().parents if (p / "install.py").is_file())


def step(identifier):
    workflow = yaml.safe_load((ROOT / ".github/workflows/release-preflight.yml").read_text())
    return next(s for s in workflow["jobs"]["preflight"]["steps"] if s.get("id") == identifier)


@pytest.mark.parametrize("kind,status,body,accepted", [
    ("github", 200, {"id": 12}, True),
    ("github", 401, {"message": "Bad credentials"}, False),
    ("crates_io", 200, {"user": {"id": 12}}, True),
    ("crates_io", 403, {"errors": [{"detail": credentials.CRATES_COOKIE_ONLY}]}, True),
    ("crates_io", 403, {"errors": [{"detail": "authentication failed"}]}, False),
    ("crates_io", 403, {"errors": [{"detail": "token expired"}]}, False),
    ("crates_io", 403, {"errors": [{"detail": "Endpoint scope mismatch"}]}, False),
    ("crates_io", 401, {"errors": [{"detail": credentials.CRATES_COOKIE_ONLY}]}, False),
    ("crates_io", 403, {"errors": [{"detail": credentials.CRATES_COOKIE_ONLY}, {"detail": "other"}]}, False),
    ("crates_io", 200, {}, False),
    ("crates_io", 200, [], False),
    ("crates_io", 302, {"location": "https://example.invalid"}, False),
    ("crates_io", 500, {}, False),
])
def test_credential_response_classification(monkeypatch, kind, status, body, accepted):
    opener = MagicMock()
    raw = json.dumps(body).encode()
    url = "https://api.github.com/user" if kind == "github" else "https://crates.io/api/v1/me"
    if status == 200:
        response = opener.open.return_value.__enter__.return_value
        response.status, response.read.return_value = status, raw
    else:
        opener.open.side_effect = urllib.error.HTTPError(url, status, "synthetic", {}, io.BytesIO(raw))
    monkeypatch.setattr(credentials.urllib.request, "build_opener", lambda handler: opener)
    if accepted:
        credentials.probe(kind, "synthetic-test-credential", url)
    else:
        with pytest.raises(ValueError, match="authentication not established") as error:
            credentials.probe(kind, "synthetic-test-credential", url)
        assert "synthetic-test-credential" not in str(error.value)
    request = opener.open.call_args.args[0]
    assert request.get_method() == "GET"
    assert opener.open.call_args.kwargs["timeout"] == 20
    assert request.get_header("Authorization") == (("Bearer " if kind == "github" else "") + "synthetic-test-credential")


@pytest.mark.parametrize("failure", [urllib.error.URLError("synthetic-test-credential"), TimeoutError("synthetic-test-credential")])
def test_credential_transport_errors_do_not_disclose_secret(monkeypatch, failure):
    opener = MagicMock()
    opener.open.side_effect = failure
    monkeypatch.setattr(credentials.urllib.request, "build_opener", lambda handler: opener)
    with pytest.raises(ValueError, match="authentication not established") as error:
        credentials.probe("crates_io", "synthetic-test-credential", "https://crates.io/api/v1/me")
    assert "synthetic-test-credential" not in str(error.value)


def test_credential_redirects_and_unknown_checks_fail_closed():
    assert credentials.NoRedirect().redirect_request(None, None, 302, "", {}, "https://example.invalid") is None
    with pytest.raises(ValueError, match="missing"):
        credentials.probe("crates_io", "", "https://crates.io/api/v1/me")
    with pytest.raises(ValueError, match="unsupported"):
        credentials.probe("other", "synthetic", "https://crates.io/api/v1/me")
    with pytest.raises(ValueError, match="endpoint"):
        credentials.probe("crates_io", "synthetic", "https://example.invalid")


@pytest.mark.parametrize("kind,exit_code", [("crates_io", 0), ("crates_io", 1), ("unsupported", 1)])
def test_liveness_workflow_records_every_channel_outcome(tmp_path, kind, exit_code):
    script = tmp_path / ".github/scripts/release_credentials.py"
    script.parent.mkdir(parents=True)
    script.write_text(f"import sys\nassert '--kind' in sys.argv\nraise SystemExit({exit_code})\n")
    selected = step("credential_liveness")
    assert selected["env"]["CARGO_REGISTRY_TOKEN"] == "${{ secrets.CARGO_REGISTRY_TOKEN }}"
    output = tmp_path / "output"
    result = subprocess.run(["bash", "-c", selected["run"]], cwd=tmp_path, env={
        **os.environ, "SECRET_PLAN": json.dumps({"liveness_channel_checks": [{"channel": "crates_io", "name": "CARGO_REGISTRY_TOKEN", "kind": kind}]}),
        "RELEASE_ARTIFACT_MANIFEST": "unused", "GITHUB_OUTPUT": str(output),
    }, text=True, capture_output=True, timeout=30)
    assert result.returncode == exit_code, result.stderr
    assert json.loads(output.read_text().split("=", 1)[1]) == {"crates_io": "success" if exit_code == 0 else "failure"}


def cargo_run(root, env, *args):
    return subprocess.run(["cargo", *args], cwd=root, env=env, text=True, capture_output=True, timeout=30)


@pytest.mark.parametrize("broken", [None, "compile", "manifest", "plan"])
def test_package_workflow_checks_unpublished_and_standalone_dependencies(tmp_path, broken):
    if shutil.which("cargo") is None:
        pytest.skip("Cargo is required for the real offline packaging regression")
    # An isolated Cargo home and offline mode make registry access impossible.
    env = {**os.environ, "CARGO_HOME": str(tmp_path / "cargo-home"), "CARGO_NET_OFFLINE": "true"}
    (tmp_path / "Cargo.toml").write_text('[workspace]\nmembers=["dep","app"]\nresolver="2"\n[workspace.package]\nversion="999.0.0"\n')
    manifest = tmp_path / "publish-artifacts.toml"
    manifest.write_text("")
    for order, name in enumerate(["dep", "app", "standalone"]):
        directory = tmp_path / name
        (directory / "src").mkdir(parents=True)
        source = 'pub fn value() -> u8 { 1 }\n' if name == "dep" else 'pub fn value() -> u8 { sc_publish_qa_dep::value() }\n'
        (directory / "src/lib.rs").write_text(source)
        toml = f'[package]\nname="sc-publish-qa-{name}"\nversion="999.0.0"\nedition="2021"\n'
        if name == "standalone":
            toml += '[workspace]\n'
        if name != "dep":
            toml += '[dependencies]\nsc-publish-qa-dep={path="../dep",version="999.0.0"}\n'
        (directory / "Cargo.toml").write_text(toml)
        with manifest.open("a") as output:
            output.write(f'[[crates]]\nartifact="{name}"\npackage="sc-publish-qa-{name}"\ncargo_toml="{name}/Cargo.toml"\npublish=true\npublish_order={order}\n')
    for target in ["Cargo.toml", "standalone/Cargo.toml"]:
        result = cargo_run(tmp_path, env, "generate-lockfile", "--manifest-path", target)
        assert result.returncode == 0, result.stderr
    old = cargo_run(tmp_path, env, "package", "--manifest-path", "app/Cargo.toml", "--locked", "--allow-dirty", "--no-verify")
    assert old.returncode != 0
    assert "no matching package" in old.stderr and "sc-publish-qa-dep" in old.stderr
    if broken == "compile":
        (tmp_path / "standalone/src/lib.rs").write_text('compile_error!("invalid release source");')
    elif broken == "manifest":
        with (tmp_path / "app/Cargo.toml").open("a") as output:
            output.write('[lib]\npath="absent.rs"\n')
    elif broken == "plan":
        manifest.write_text("not valid toml")
    scripts = tmp_path / ".github/scripts"
    scripts.mkdir(parents=True)
    for source in (ROOT / ".github/scripts").glob("*.py"):
        shutil.copy2(source, scripts / source.name)
    body = step("package_checks")["run"].replace("${{ steps.build_plan.outputs.workspace_toml }}", "Cargo.toml")
    result = subprocess.run(["bash", "-c", body], cwd=tmp_path, env={**env, "RELEASE_ARTIFACT_MANIFEST": str(manifest)}, text=True, capture_output=True, timeout=30)
    if broken:
        assert result.returncode != 0, result.stdout
        return
    assert result.returncode == 0, result.stderr
    for target, name in [("target/package", "app"), ("standalone/target/package", "standalone")]:
        with tarfile.open(tmp_path / target / f"sc-publish-qa-{name}-999.0.0.crate") as archive:
            files = archive.getnames()
            assert any(f.endswith("/src/lib.rs") for f in files)
            assert not any(f.endswith("/Cargo.lock") for f in files)
            normalized = archive.extractfile(f"sc-publish-qa-{name}-999.0.0/Cargo.toml").read().decode()
            assert 'version = "999.0.0"' in normalized
            assert 'path = "../dep"' not in normalized
    assert (tmp_path / "target/package/sc-publish-qa-dep-999.0.0").is_dir(), "independent crate must still pass archive build verification"


def test_credential_cli_obeys_contract_without_disclosing_secret(tmp_path, monkeypatch, capsys):
    manifest = tmp_path / "publish-artifacts.toml"
    manifest.write_text("crates=[]\n")
    shutil.copy2(ROOT / "release/publish-channel-contracts.toml.j2", tmp_path / "publish-channel-contracts.toml")
    args = ["release_credentials.py", "--manifest", str(manifest), "--channel", "crates_io", "--kind", "crates_io", "--secret-name", "CARGO_REGISTRY_TOKEN"]
    monkeypatch.setattr(sys, "argv", args)
    monkeypatch.setenv("CARGO_REGISTRY_TOKEN", "synthetic-test-credential")
    probe = MagicMock()
    monkeypatch.setattr(credentials, "probe", probe)
    assert credentials.main() == 0
    probe.assert_called_once_with("crates_io", "synthetic-test-credential", "https://crates.io/api/v1/me")
    output = capsys.readouterr().out
    assert "publish permission not established" in output
    assert "synthetic-test-credential" not in output
    probe.reset_mock()
    monkeypatch.setattr(sys, "argv", args[:-1] + ["UNDECLARED_SECRET"])
    with pytest.raises(SystemExit, match="not declared"):
        credentials.main()
    probe.assert_not_called()


def test_credential_invalid_json_fails_closed(monkeypatch):
    opener = MagicMock()
    response = opener.open.return_value.__enter__.return_value
    response.status, response.read.return_value = 200, b"not-json"
    monkeypatch.setattr(credentials.urllib.request, "build_opener", lambda handler: opener)
    with pytest.raises(ValueError, match="invalid credential service response"):
        credentials.probe("crates_io", "synthetic", "https://crates.io/api/v1/me")
