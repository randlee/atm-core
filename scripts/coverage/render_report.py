#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import argparse
import json

from jinja2 import Environment, FileSystemLoader, StrictUndefined


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def render_markdown(payload: dict[str, object]) -> str:
    root = repo_root()
    env = Environment(
        loader=FileSystemLoader(str(root / "templates" / "coverage-report")),
        undefined=StrictUndefined,
        trim_blocks=True,
        lstrip_blocks=True,
    )
    template = env.get_template(f"{payload['platform']}.md.j2")
    return template.render(report=payload).rstrip() + "\n"


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
