#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["jev-check"]
# [tool.uv.sources]
# jev-check = { path = "..", editable = true }
# ///
"""jev-check run <check.toml> --rev <rev> --backend jev|haiku|luna   -> JevCheckResult JSON
jev-check ask --session <id> "<question>"                          -> the follow-up answer (haiku/luna sessions)"""
import argparse, dataclasses, json, sys
from pathlib import Path

from jev_check import JevCheckError, SessionStore, load_check, run_check


def main() -> int:
    p = argparse.ArgumentParser(prog="jev-check")
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("check", type=Path)
    r.add_argument("--rev", default="HEAD")
    r.add_argument("--backend", choices=["jev", "haiku", "luna"], default="jev")
    a = sub.add_parser("ask")
    a.add_argument("--session", required=True)
    a.add_argument("question")
    args = p.parse_args()
    try:
        if args.cmd == "run":
            print(json.dumps(dataclasses.asdict(run_check(load_check(args.check), args.rev, args.backend)), indent=1))
        else:
            print(SessionStore.default().ask(args.session, args.question))
    except JevCheckError as exc:
        print(json.dumps({"error": exc.code, "message": str(exc)}), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
