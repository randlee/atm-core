#!/usr/bin/env python3
from __future__ import annotations

from datetime import datetime, timezone
from html import escape
from pathlib import Path
import argparse
import json
import subprocess
import sys

from jinja2 import Environment, FileSystemLoader, StrictUndefined

from fixtures import SmokePaths
from fixtures import level_slug
from fixtures import repo_root
from fixtures import smoke_paths
from fixtures import write_json
from fixtures import write_text_atomic


class SmokeReportError(RuntimeError):
    """The smoke report evidence could not be published to the site."""


def template_name(level: str) -> str:
    slug = level_slug(level)
    return f"{slug}.md.j2"


def render_markdown(payload: dict) -> str:
    root = repo_root()
    env = Environment(
        loader=FileSystemLoader(str(root / "templates" / "smoke-report")),
        undefined=StrictUndefined,
        trim_blocks=True,
        lstrip_blocks=True,
    )
    template = env.get_template(template_name(payload["level"]))
    return template.render(report=payload).rstrip() + "\n"


def generated_at_for(payload: dict) -> str:
    timestamp = payload.get("timestamp")
    if isinstance(timestamp, str) and timestamp:
        return timestamp
    return datetime.now(timezone.utc).isoformat()


def write_envelope(paths: SmokePaths, generated_at: str) -> None:
    envelope = {
        "schema_version": 1,
        "report_type": "smoke",
        "generated_at": generated_at,
        "host_label": paths.host_label,
        "report_html": paths.report_html.name,
    }
    write_json(paths.envelope_json, envelope)


def render_index_page(paths: SmokePaths, level: str) -> None:
    """Render the flat root-level aggregate page linking every run in report_dir.

    Mirrors the ``site/reports/send-message-benchmark.html`` aggregate shell:
    a self-contained page with no templating dependency, listing every
    timestamped, host-labeled run in newest-first order.
    """
    rows: list[str] = []
    if paths.report_dir.is_dir():
        json_paths = sorted(
            (path for path in paths.report_dir.glob("*.json") if not path.name.endswith(".envelope.json")),
            reverse=True,
        )
        for json_path in json_paths:
            stem = json_path.stem
            markdown_path = paths.report_dir / f"{stem}.md"
            slug = paths.report_dir.name
            links = [f'<a href="{escape(slug, quote=True)}/{escape(json_path.name, quote=True)}">json</a>']
            if markdown_path.is_file():
                links.append(
                    f'<a href="{escape(slug, quote=True)}/{escape(markdown_path.name, quote=True)}">markdown</a>'
                )
            rows.append(f"      <li><code>{escape(stem)}</code> &middot; {' &middot; '.join(links)}</li>")
    body = "\n".join(rows) if rows else '      <li class="empty">No smoke runs recorded yet.</li>'
    title = f"ATM {escape(level_slug(level))} smoke reports"
    paths.report_html.parent.mkdir(parents=True, exist_ok=True)
    paths.report_html.write_text(
        "<!doctype html>\n"
        '<html lang="en">\n'
        "<head>\n"
        '  <meta charset="utf-8">\n'
        f"  <title>{title}</title>\n"
        "</head>\n"
        "<body>\n"
        f"  <h1>{title}</h1>\n"
        "  <ul>\n"
        f"{body}\n"
        "  </ul>\n"
        "</body>\n"
        "</html>\n",
        encoding="utf-8",
    )


def regenerate_index(root: Path) -> None:
    completed = subprocess.run(["just", "reports-index"], cwd=root, capture_output=True, text=True, check=False)
    if completed.returncode != 0:
        raise SmokeReportError(f"reports-index failed: {completed.stderr.strip() or completed.stdout.strip()}")


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
        write_text_atomic(paths.timestamped_markdown, markdown)
        write_json(paths.timestamped_json, payload)
        write_envelope(paths, generated_at_for(payload))
        render_index_page(paths, payload["level"])
        try:
            regenerate_index(paths.repo_root)
        except SmokeReportError as error:
            print(f"render-report: {error}", file=sys.stderr)
            return 1
    else:
        print(markdown, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
