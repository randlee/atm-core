#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import argparse
import json
import sys

REPO_ROOT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.report_runtime import compose_text  # noqa: E402


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def render_markdown(payload: dict[str, object]) -> str:
    root = repo_root()
    template = root / "templates" / "coverage-report" / f"{payload['platform']}.md.j2"
    return compose_text(template, {"report": payload}, root=root).rstrip() + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Render coverage markdown from canonical JSON.")
    parser.add_argument("payload_json", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    payload = json.loads(args.payload_json.read_text(encoding="utf-8"))
    print(render_markdown(payload), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
