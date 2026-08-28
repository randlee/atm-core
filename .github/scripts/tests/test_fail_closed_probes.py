"""Behavioral fail-closed tests for the release and channel idempotency probes.

Each test extracts the real bash from the vendored workflow/action YAML and
executes it against a stubbed `gh`, asserting that only a confirmed 404
("absent") proceeds while any indeterminate probe result hard-fails.
"""

from __future__ import annotations

import os
import stat
import subprocess
from pathlib import Path


REPO_ROOT = next(
    path for path in Path(__file__).resolve().parents if (path / "install.py").is_file()
)


def extract_run_block(text: str, anchor: str) -> str:
    """Return the dedented bash of the first `run: |` block after anchor."""
    anchor_index = text.index(anchor)
    run_index = text.index("run: |", anchor_index)
    line_start = text.rindex("\n", 0, run_index) + 1
    block_indent = " " * (run_index - line_start + 2)
    lines: list[str] = []
    for line in text[text.index("\n", run_index) + 1 :].splitlines():
        if not line.strip():
            lines.append("")
            continue
        if not line.startswith(block_indent):
            break
        lines.append(line[len(block_indent) :])
    return "\n".join(lines) + "\n"


def write_gh_stub(tmp_path: Path, body: str) -> Path:
    bin_dir = tmp_path / "stub-bin"
    bin_dir.mkdir(exist_ok=True)
    stub = bin_dir / "gh"
    stub.write_text("#!/usr/bin/env bash\n" + body, encoding="utf-8")
    stub.chmod(stub.stat().st_mode | stat.S_IEXEC)
    return bin_dir


def run_probe_script(
    tmp_path: Path, script: str, stub_body: str, env: dict[str, str]
) -> tuple[subprocess.CompletedProcess[str], dict[str, str]]:
    bin_dir = write_gh_stub(tmp_path, stub_body)
    output_file = tmp_path / "github-output"
    output_file.write_text("", encoding="utf-8")
    result = subprocess.run(
        ["bash"],
        input=script,
        text=True,
        capture_output=True,
        check=False,
        env={
            **os.environ,
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "GH_TOKEN": "stub-token",
            "GITHUB_OUTPUT": str(output_file),
            **env,
        },
    )
    outputs = dict(
        line.split("=", 1)
        for line in output_file.read_text(encoding="utf-8").splitlines()
        if "=" in line
    )
    return result, outputs


# --- GitHub Release probe (verify-published-release, issue #40) -------------


RELEASE_GH_STUB = """
case "${FAKE_GH_MODE:?}" in
  found)
    printf '%s' "${FAKE_GH_RELEASE_JSON}"
    exit 0
    ;;
  absent)
    echo "release not found" >&2
    exit 1
    ;;
  http-404)
    echo "gh: Not Found (HTTP 404)" >&2
    exit 1
    ;;
  transient)
    echo "gh: The server had an error while processing your request (HTTP 502)" >&2
    exit 1
    ;;
esac
exit 1
"""

COMPLETE_RELEASE_JSON = (
    '{"isDraft": false, "assets": ['
    '{"name": "example_1.2.3_x86_64-unknown-linux-gnu.tar.gz"},'
    '{"name": "checksums.txt"}]}'
)
RELEASE_ASSET_PATTERNS = "^example_.*\\.tar\\.gz$\n^checksums\\.txt$"


def release_probe_script() -> str:
    text = (
        REPO_ROOT / ".github" / "actions" / "verify-published-release" / "action.yml"
    ).read_text(encoding="utf-8")
    return extract_run_block(text, "id: verify")


def run_release_probe(
    tmp_path: Path,
    *,
    mode: str,
    probe: str,
    release_json: str = "",
    asset_patterns: str = RELEASE_ASSET_PATTERNS,
    tag: str = "v1.2.3",
) -> tuple[subprocess.CompletedProcess[str], dict[str, str]]:
    return run_probe_script(
        tmp_path,
        release_probe_script(),
        RELEASE_GH_STUB,
        {
            "FAKE_GH_MODE": mode,
            "FAKE_GH_RELEASE_JSON": release_json,
            "RELEASE_REPOSITORY": "example/example",
            "RELEASE_TAG": tag,
            "REQUIRED_ASSET_PATTERNS": asset_patterns,
            "PROBE_MODE": probe,
        },
    )


def test_release_probe_confirmed_absent_reports_absent(tmp_path: Path) -> None:
    for mode in ("absent", "http-404"):
        result, outputs = run_release_probe(tmp_path, mode=mode, probe="true")
        assert result.returncode == 0, result.stderr
        assert outputs["release_state"] == "absent"


def test_release_probe_fails_closed_on_transient_api_error(tmp_path: Path) -> None:
    result, outputs = run_release_probe(tmp_path, mode="transient", probe="true")
    assert result.returncode != 0
    assert "indeterminate" in result.stderr
    assert "HTTP 502" in result.stderr
    assert "release_state" not in outputs


def test_release_probe_reports_complete_when_every_asset_is_present(tmp_path: Path) -> None:
    result, outputs = run_release_probe(
        tmp_path, mode="found", probe="true", release_json=COMPLETE_RELEASE_JSON
    )
    assert result.returncode == 0, result.stderr
    assert outputs["release_state"] == "complete"


def test_release_probe_reports_incomplete_for_missing_assets_or_draft(tmp_path: Path) -> None:
    missing_assets_json = '{"isDraft": false, "assets": [{"name": "checksums.txt"}]}'
    result, outputs = run_release_probe(
        tmp_path, mode="found", probe="true", release_json=missing_assets_json
    )
    assert result.returncode == 0, result.stderr
    assert outputs["release_state"] == "incomplete"
    assert "missing an asset matching" in result.stderr

    draft_json = COMPLETE_RELEASE_JSON.replace('"isDraft": false', '"isDraft": true')
    result, outputs = run_release_probe(
        tmp_path, mode="found", probe="true", release_json=draft_json
    )
    assert result.returncode == 0, result.stderr
    assert outputs["release_state"] == "incomplete"


def test_release_verify_mode_still_hard_fails_each_bad_state(tmp_path: Path) -> None:
    result, _ = run_release_probe(tmp_path, mode="absent", probe="false")
    assert result.returncode != 0
    assert "No published GitHub Release found" in result.stderr

    result, outputs = run_release_probe(tmp_path, mode="transient", probe="false")
    assert result.returncode != 0
    assert "indeterminate" in result.stderr
    assert "release_state" not in outputs

    draft_json = COMPLETE_RELEASE_JSON.replace('"isDraft": false', '"isDraft": true')
    result, _ = run_release_probe(
        tmp_path, mode="found", probe="false", release_json=draft_json
    )
    assert result.returncode != 0
    assert "is still a draft" in result.stderr

    missing_assets_json = '{"isDraft": false, "assets": [{"name": "checksums.txt"}]}'
    result, _ = run_release_probe(
        tmp_path, mode="found", probe="false", release_json=missing_assets_json
    )
    assert result.returncode != 0
    assert "missing an asset matching" in result.stderr

    result, outputs = run_release_probe(
        tmp_path, mode="found", probe="false", release_json=COMPLETE_RELEASE_JSON
    )
    assert result.returncode == 0, result.stderr
    assert outputs["release_state"] == "complete"


def test_release_probe_rejects_invalid_tags_before_any_lookup(tmp_path: Path) -> None:
    result, outputs = run_release_probe(
        tmp_path, mode="found", probe="true", tag="not-a-tag"
    )
    assert result.returncode != 0
    assert "Invalid tag" in result.stderr
    assert not outputs


# --- winget-pkgs pre-submission probe (winget-publish.yml, issue #41) -------


WINGET_GH_STUB = """
args="$*"
case "${args}" in
  *search/issues*)
    case "${FAKE_GH_SEARCH_MODE:?}" in
      none)
        printf '{"total_count": 0, "incomplete_results": false}'
        exit 0
        ;;
      found)
        printf '{"total_count": 2, "incomplete_results": false}'
        exit 0
        ;;
      incomplete)
        printf '{"total_count": 0, "incomplete_results": true}'
        exit 0
        ;;
      error)
        echo "gh: HTTP 503 service unavailable" >&2
        exit 1
        ;;
    esac
    ;;
  *winget-pkgs/contents/*)
    case "${FAKE_GH_CONTENTS_MODE:?}" in
      found)
        printf '{"name": "manifest"}'
        exit 0
        ;;
      absent)
        echo "gh: Not Found (HTTP 404)" >&2
        exit 1
        ;;
      error)
        echo "gh: The server had an error while processing your request (HTTP 502)" >&2
        exit 1
        ;;
    esac
    ;;
esac
exit 1
"""


def winget_probe_script() -> str:
    text = (REPO_ROOT / ".github" / "workflows" / "winget-publish.yml").read_text(
        encoding="utf-8"
    )
    return extract_run_block(text, "id: winget_probe")


def run_winget_probe(
    tmp_path: Path, *, contents_mode: str, search_mode: str = "none"
) -> tuple[subprocess.CompletedProcess[str], dict[str, str]]:
    return run_probe_script(
        tmp_path,
        winget_probe_script(),
        WINGET_GH_STUB,
        {
            "FAKE_GH_CONTENTS_MODE": contents_mode,
            "FAKE_GH_SEARCH_MODE": search_mode,
            "IDENTIFIER": "Example.Example",
            "RELEASE_TAG": "v1.2.3",
        },
    )


def test_winget_probe_confirmed_absent_proceeds_to_submission(tmp_path: Path) -> None:
    result, outputs = run_winget_probe(tmp_path, contents_mode="absent", search_mode="none")
    assert result.returncode == 0, result.stderr
    assert outputs["already_published"] == "false"


def test_winget_probe_skips_when_manifest_or_pr_already_exists(tmp_path: Path) -> None:
    result, outputs = run_winget_probe(tmp_path, contents_mode="found")
    assert result.returncode == 0, result.stderr
    assert outputs["already_published"] == "true"
    assert "already publishes" in result.stdout

    result, outputs = run_winget_probe(
        tmp_path, contents_mode="absent", search_mode="found"
    )
    assert result.returncode == 0, result.stderr
    assert outputs["already_published"] == "true"
    assert "pull request (open or merged) already exists" in result.stdout


def test_winget_probe_fails_closed_on_transient_manifest_lookup_error(tmp_path: Path) -> None:
    result, outputs = run_winget_probe(tmp_path, contents_mode="error")
    assert result.returncode != 0
    assert "indeterminate" in result.stderr
    assert "HTTP 502" in result.stderr
    assert "already_published" not in outputs


def test_winget_probe_fails_closed_on_pr_search_failures(tmp_path: Path) -> None:
    result, outputs = run_winget_probe(
        tmp_path, contents_mode="absent", search_mode="error"
    )
    assert result.returncode != 0
    assert "already_published" not in outputs

    result, outputs = run_winget_probe(
        tmp_path, contents_mode="absent", search_mode="incomplete"
    )
    assert result.returncode != 0
    assert "incomplete results" in result.stderr
    assert "already_published" not in outputs
