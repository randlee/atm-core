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

from fixtures import ensure_parent
from fixtures import level_slug
from fixtures import repo_root
from fixtures import smoke_paths
from fixtures import write_json
from fixtures import write_text_atomic


def template_name(level: str) -> str:
    slug = level_slug(level)
    return f"{slug}.md.j2"


def render_markdown(payload: dict) -> str:
    root = repo_root()
    template = root / "templates" / "smoke-report" / template_name(payload["level"])
    return compose_text(template, {"report": payload}, root=root).rstrip() + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Render smoke markdown reports from canonical JSON.")
    parser.add_argument("payload_json", type=Path)
    parser.add_argument("--write-artifacts", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    payload = json.loads(args.payload_json.read_text(encoding="utf-8"))
    markdown = render_markdown(payload)
    if args.write_artifacts:
        paths = smoke_paths(payload["level"])
        ensure_parent(paths.markdown)
        write_text_atomic(paths.markdown, markdown)
        write_json(paths.json, payload)
        print(f"SMOKE evidence: {paths.report_dir}")
    else:
        print(markdown, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
