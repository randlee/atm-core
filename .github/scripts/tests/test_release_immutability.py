"""No network or publication: exercise immutable-release admission and receipts."""
import json
from pathlib import Path
import subprocess
import sys
from unittest.mock import patch

import pytest
import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from release_immutability import ImmutabilityError, api, check
from test_install import INSTALL


def query_for(settings=(200, {"enabled": True}), releases=None):
    def query(path):
        if path.endswith("immutable-releases"):
            return settings
        return 200, releases or []
    return query


@pytest.mark.parametrize("status,body,reason", [
    (200, {"enabled": False}, "disabled:"),
    (200, {}, "indeterminate:"),
    (200, {"enabled": "true"}, "indeterminate:"),
    (401, None, "indeterminate:"), (403, None, "indeterminate:"),
    (404, None, "indeterminate:"), (429, None, "indeterminate:"),
    (500, None, "indeterminate:"),
])
def test_disabled_and_unreadable_settings_fail_closed(status, body, reason):
    with pytest.raises(ImmutabilityError, match=reason):
        check("owner/repo", "v1.2.3", query=query_for((status, body)))


@pytest.mark.parametrize("release,state", [
    (None, "absent"),
    ({"tag_name": "v1.2.3", "draft": True, "immutable": False}, "draft"),
    ({"tag_name": "v1.2.3", "draft": False, "immutable": True}, "immutable"),
])
def test_enabled_repository_new_draft_and_existing_immutable_release(release, state):
    query = query_for(releases=[release] if release else [])
    assert check("owner/repo", "v1.2.3", query=query)["release_state"] == state
    if state == "immutable":
        assert check("owner/repo", "v1.2.3", query=query, finalized=True)["release_state"] == state
    else:
        with pytest.raises(ImmutabilityError, match="downstream publication denied"):
            check("owner/repo", "v1.2.3", query=query, finalized=True)


@pytest.mark.parametrize("immutable", [False, None, "true"])
def test_mutable_or_unknown_existing_release_never_converted(immutable):
    with pytest.raises(ImmutabilityError, match="unsupported by this pipeline"):
        check("owner/repo", "v1.2.3", query=query_for(releases=[
            {"tag_name": "v1.2.3", "draft": False, "immutable": immutable}]))


def test_replacement_denied_before_any_request():
    with pytest.raises(ImmutabilityError, match="prohibit"):
        check("owner/repo", "v1.2.3", replace_assets=True,
              query=lambda _: pytest.fail("replacement must fail before API"))


def test_inventory_error_not_absence_and_pagination_finds_existing_release():
    calls = []
    def query(path):
        calls.append(path)
        if path.endswith("immutable-releases"):
            return 200, {"enabled": True}
        if path.endswith("&page=1"):
            return 200, [{"tag_name": "v0.0.1"}] * 100
        return 200, [{"tag_name": "v1.2.3", "draft": False, "immutable": True}]
    assert check("owner/repo", "v1.2.3", query=query)["release_state"] == "immutable"
    assert len(calls) == 3
    with pytest.raises(ImmutabilityError, match="inventory unavailable"):
        check("owner/repo", "v1.2.3", query=lambda p: (200, {"enabled": True}) if p.endswith("immutable-releases") else (403, None))


def test_api_transport_is_bounded_read_only_and_does_not_echo_failure_body():
    with patch("release_immutability.subprocess.run") as run:
        run.return_value = subprocess.CompletedProcess([], 0, 'HTTP/2.0 200 OK\ncontent-type: application/json\n\n{"enabled":true}', '')
        assert api("repos/o/r/immutable-releases") == (200, {"enabled": True})
        assert run.call_args.kwargs["timeout"] == 30
        assert "--method" not in run.call_args.args[0]
        run.return_value = subprocess.CompletedProcess([], 1, '', 'SECRET sentinel')
        with pytest.raises(ImmutabilityError, match="no HTTP status") as error:
            api("repos/o/r/immutable-releases")
        assert "SECRET" not in str(error.value)
        run.side_effect = subprocess.TimeoutExpired("gh", 30)
        with pytest.raises(ImmutabilityError, match="timed out"):
            api("repos/o/r/immutable-releases")


def test_shared_workflow_admission_and_finalization_order():
    root = INSTALL.PACKAGE_ROOT / ".github/workflows"
    release = yaml.safe_load((root / "release.yml").read_text())
    gate = release["jobs"]["gate-and-tag"]["steps"]
    check_index = next(i for i, step in enumerate(gate) if "release_immutability.py" in step.get("run", ""))
    tag_index = next(i for i, step in enumerate(gate) if step.get("id") == "release-ref")
    assert check_index < tag_index
    steps = release["jobs"]["release"]["steps"]
    assert "--finalized" in steps[-1]["run"]
    assert steps[-1].get("continue-on-error", False) is False
    preflight = yaml.safe_load((root / "release-preflight.yml").read_text())["jobs"]["preflight"]["steps"]
    prerequisite = next(s for s in preflight if s.get("id") == "immutable_releases")
    assert prerequisite["env"]["GH_TOKEN"] == "${{ github.token }}"
    summary = preflight[-1]
    assert summary["env"]["IMMUTABLE_RELEASES"] == "${{ steps.immutable_releases.outcome }}"
    assert 'record immutable-releases "${IMMUTABLE_RELEASES}"' in summary["run"]


def test_missing_or_failed_prerequisite_blocks_every_channel():
    from release_manifest import _channel_preflight_result
    channel = {"name": "example", "agent": "publisher"}
    base = {"ownership": "success", "release_metadata": "success"}
    assert _channel_preflight_result(channel, base, "v1.2.3")["status"] == "blocked"
    assert _channel_preflight_result(channel, {**base, "immutable_releases": "failure"}, "v1.2.3")["status"] == "failed"
    assert _channel_preflight_result(channel, {**base, "immutable_releases": "success"}, "v1.2.3")["status"] == "passed"
