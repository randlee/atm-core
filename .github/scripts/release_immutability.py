#!/usr/bin/env python3
"""Read-only, fail-closed GitHub immutable release state and receipt check."""
import argparse
import json
import os
import re
import subprocess
import sys


class ImmutabilityError(RuntimeError):
    pass


def api(path):
    """Return HTTP status and JSON, without printing credentials or API bodies."""
    try:
        result = subprocess.run(
            ["gh", "api", "--include", "-H", "Accept: application/vnd.github+json",
             "-H", "X-GitHub-Api-Version: 2026-03-10", path],
            capture_output=True, text=True, timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ImmutabilityError("indeterminate: GitHub API unavailable or timed out") from error
    match = re.search(r"^HTTP/\S+ (\d{3})[^\n]*\n", result.stdout)
    if not match:
        raise ImmutabilityError("indeterminate: GitHub API returned no HTTP status")
    status = int(match[1])
    if status != 200:
        return status, None
    if result.returncode:
        raise ImmutabilityError("indeterminate: GitHub API command failed")
    try:
        body = result.stdout.split("\n\n", 1)[1]
        return status, json.loads(body)
    except (IndexError, json.JSONDecodeError) as error:
        raise ImmutabilityError("indeterminate: invalid GitHub API response") from error


def check(repository, tag, *, finalized=False, replace_assets=False, query=api):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ImmutabilityError("invalid repository; expected OWNER/REPO")
    if not re.fullmatch(r"v?\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.+-]+)?", tag):
        raise ImmutabilityError("invalid release tag")
    if replace_assets:
        raise ImmutabilityError("immutable releases prohibit replace_release_assets; use a new version")
    # The repository setting itself is readable only with Administration:read,
    # which no workflow token carries. Immutability is proven on the release
    # object instead: an existing release must report immutable == true, and
    # --finalized denies downstream publication until the published one does.
    # Listing includes drafts visible to the credential, unlike lookup by tag.
    # Bound pagination; exhaustion is an error rather than guessed absence.
    for page in range(1, 101):
        status, releases = query(f"repos/{repository}/releases?per_page=100&page={page}")
        if status != 200 or not isinstance(releases, list):
            raise ImmutabilityError(f"indeterminate: release inventory unavailable (HTTP {status})")
        for release in releases:
            if not isinstance(release, dict):
                raise ImmutabilityError("indeterminate: invalid release inventory entry")
            if release.get("tag_name") != tag:
                continue
            if release.get("draft") is True:
                if finalized:
                    raise ImmutabilityError("release is still draft; downstream publication denied")
                return {"release_state": "draft"}
            if release.get("draft") is not False or release.get("immutable") is not True:
                raise ImmutabilityError("existing release is mutable or indeterminate; unsupported by this pipeline; use a new version")
            return {"release_state": "immutable"}
        if len(releases) < 100:
            if finalized:
                raise ImmutabilityError("published immutable release not found; downstream publication denied")
            return {"release_state": "absent"}
    raise ImmutabilityError("indeterminate: release inventory pagination limit reached")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--tag", required=True)
    parser.add_argument("--finalized", action="store_true")
    parser.add_argument("--replace-assets", action="store_true")
    args = parser.parse_args()
    tag = args.tag if args.tag.startswith("v") else "v" + args.tag
    try:
        print(json.dumps(check(args.repository, tag, finalized=args.finalized, replace_assets=args.replace_assets)))
    except ImmutabilityError as error:
        print(f"Immutable release prerequisite failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
