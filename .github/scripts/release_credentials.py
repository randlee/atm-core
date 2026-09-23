"""Read-only credential authentication probes; never establish publish authority."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import urllib.error
import urllib.request

from release_manifest import load_manifest

# crates.io authenticates first, then rejects API tokens at its cookie-only /me
# endpoint. Other 403 responses do not establish token authentication.
# https://github.com/rust-lang/crates.io/blob/main/src/auth.rs (AuthCheck::check)
CRATES_COOKIE_ONLY = "this action can only be performed on the crates.io website"


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def probe(kind: str, token: str, url: str) -> None:
    if not token:
        raise ValueError("credential is missing")
    if kind not in {"github", "crates_io"}:
        raise ValueError("unsupported credential liveness check kind")
    expected = "https://api.github.com/user" if kind == "github" else "https://crates.io/api/v1/me"
    if url != expected:
        raise ValueError("unsupported credential liveness endpoint")
    request = urllib.request.Request(url, headers={
        "Authorization": f"Bearer {token}" if kind == "github" else token,
        "Accept": "application/json",
        "User-Agent": "sc-publish-read-only-preflight",
    })
    opener = urllib.request.build_opener(NoRedirect())
    try:
        with opener.open(request, timeout=20) as response:
            status, body = response.status, response.read(65536)
    except urllib.error.HTTPError as error:
        status, body = error.code, error.read(65536)
        error.close()
    except (OSError, urllib.error.URLError, ValueError):
        raise ValueError("credential service unavailable; authentication not established") from None
    try:
        data = json.loads(body)
    except (ValueError, UnicodeDecodeError):
        raise ValueError("invalid credential service response; authentication not established") from None
    if not isinstance(data, dict):
        raise ValueError("invalid credential service response; authentication not established")
    identity = data.get("user") if kind == "crates_io" else data
    if status == 200 and isinstance(identity, dict) and type(identity.get("id")) is int and identity["id"] > 0:
        return
    if kind == "crates_io" and status == 403 and data == {"errors": [{"detail": CRATES_COOKIE_ONLY}]}:
        return
    # Never echo service bodies: they may contain confidential account details.
    raise ValueError(f"credential authentication not established (HTTP {status})")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--channel", required=True)
    parser.add_argument("--kind", required=True)
    parser.add_argument("--secret-name", required=True)
    args = parser.parse_args()
    manifest = load_manifest(Path(args.manifest), with_channel_contracts=True)
    contract = manifest["channel_contracts"].get(args.channel, {})
    check = {"name": args.secret_name, "kind": args.kind}
    if check not in contract.get("liveness_checks", []):
        raise SystemExit("credential check is not declared by the channel contract")
    url = contract.get("account_liveness_url", "") if args.kind == "crates_io" else "https://api.github.com/user"
    try:
        probe(args.kind, os.environ.get(args.secret_name, ""), url)
    except ValueError as error:
        raise SystemExit(f"{args.channel}: {error}") from None
    print(f"{args.channel}: credential authenticated; publish permission not established")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
