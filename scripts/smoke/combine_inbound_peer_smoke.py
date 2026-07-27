#!/usr/bin/env python3
"""Combine current per-host sc-compose smoke panes into one review XHTML page."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
from html import escape
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile


REPO_ROOT = Path(__file__).resolve().parents[2]
TEMPLATE = REPO_ROOT / "templates/smoke-report/inbound-peer-review.xhtml.j2"
HOST_META = re.compile(r'<meta name="smoke-host" content="([^"]+)" />')
TIME_META = re.compile(r'<meta name="smoke-generated-at" content="([^"]+)" />')
BODY = re.compile(r"<body>(.*)</body>", re.DOTALL)


def fail(message: str) -> None:
    raise RuntimeError(message)


def compose(variables: dict[str, str], output: Path) -> None:
    with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
        json.dump(variables, handle)
        var_file = Path(handle.name)
    try:
        completed = subprocess.run(["sc-compose", "render", "--root", str(REPO_ROOT), "--file", str(TEMPLATE), "--var-file", str(var_file), "--output", str(output)], capture_output=True, text=True, check=False)
        if completed.returncode:
            fail("sc-compose render failed: " + (completed.stderr.strip() or completed.stdout.strip()))
    finally:
        var_file.unlink(missing_ok=True)


def load_current_pane(path: Path, expected_host: str, max_age_minutes: float) -> str:
    try:
        raw = path.read_text(encoding="utf-8")
    except OSError as error:
        fail(f"required pane `{expected_host}` is absent: {error}")
    host = HOST_META.search(raw)
    generated = TIME_META.search(raw)
    body = BODY.search(raw)
    if not host or host.group(1) != expected_host:
        fail(f"pane {path} is not labelled for required host `{expected_host}`")
    if not generated or not body:
        fail(f"pane {path} is missing required smoke metadata/body")
    try:
        created = datetime.fromisoformat(generated.group(1).replace("Z", "+00:00"))
    except ValueError as error:
        fail(f"pane {path} has invalid generated timestamp: {error}")
    age = (datetime.now(timezone.utc) - created).total_seconds() / 60
    if age > max_age_minutes:
        fail(f"pane {path} is outdated ({age:.1f}m > {max_age_minutes:.1f}m)")
    return f'<section class="pane"><h2>{escape(expected_host)}</h2>{body.group(1)}</section>'


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--panes-dir", required=True, type=Path)
    parser.add_argument("--hosts", required=True, help="comma-separated required host labels, e.g. local,m5,fastpc4")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--max-age-minutes", type=float, default=30.0)
    args = parser.parse_args()
    hosts = [host.strip() for host in args.hosts.split(",") if host.strip()]
    if not hosts:
        fail("--hosts must name at least one host")
    panes = [load_current_pane(args.panes_dir / f"{host}.xhtml", host, args.max_age_minutes) for host in hosts]
    compose({"title": "ATM cross-host smoke review", "generated_at": datetime.now(timezone.utc).isoformat(), "pane_html": "".join(panes)}, args.output)
    print(f"PASS combined-review: {args.output}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as error:
        print(f"combine-inbound-peer-smoke error: {error}", file=sys.stderr)
        raise SystemExit(2)
