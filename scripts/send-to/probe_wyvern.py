#!/usr/bin/env python3
"""Bounded optional Wyvern compatibility probe for the Send-To adapter."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

VERSION = re.compile(r"(?<!\d)(\d+)\.(\d+)\.(\d+)(?!\d)")
PROBE_SECONDS = 1.5


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pin", required=True)
    parser.add_argument("--asset", required=True)
    args = parser.parse_args(argv[1:])
    binary = os.environ.get("ATM_SEND_TO_WYVERN_BIN", "wyvern")
    if shutil.which(binary) is None:
        print("wyvern is absent from PATH", file=sys.stderr)
        return 1
    expected = VERSION.fullmatch(args.pin)
    if expected is None:
        print("Wyvern pin is not a complete semantic version", file=sys.stderr)
        return 1
    try:
        completed = subprocess.run(
            [binary, "--version"], capture_output=True, text=True,
            timeout=PROBE_SECONDS, check=False,
        )
    except subprocess.TimeoutExpired:
        print(f"wyvern --version exceeded {PROBE_SECONDS:.1f}s probe deadline", file=sys.stderr)
        return 1
    match = VERSION.search(completed.stdout + "\n" + completed.stderr)
    if completed.returncode != 0 or match is None:
        print("wyvern --version was unparsable", file=sys.stderr)
        return 1
    if tuple(map(int, match.groups())) < tuple(map(int, expected.groups())):
        print(f"wyvern {match.group(0)} is below pinned {args.pin}", file=sys.stderr)
        return 1
    asset = Path(args.asset)
    if not asset.is_file():
        print(f"Wyvern picker asset is missing: {asset}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
