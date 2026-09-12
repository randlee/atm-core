#!/usr/bin/env python3
"""Render immutable, revision-addressed procedure pages."""

from __future__ import annotations

import argparse
from datetime import date
import html
import json
from pathlib import Path
import re
import sys
import tempfile
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from docs.reports.generate_diagram_pages import render_file, render_mermaid_source  # noqa: E402
DOCS = ROOT / "docs" / "procedures"
OUTPUT = ROOT / "site" / "reports" / "procedures"
TEMPLATE = ROOT / "templates" / "procedure-report" / "procedure.html.j2"
SHA = re.compile(r"^[0-9a-f]{40}$")


class ProcedureRenderError(ValueError):
    pass


def compose(template: Path, variables: dict[str, Any], output: Path) -> str:
    try:
        with tempfile.TemporaryDirectory() as tempdir:
            rendered = Path(tempdir) / output.name
            render_file(template, variables, rendered)
            return rendered.read_text(encoding="utf-8")
    except Exception as error:
        raise ProcedureRenderError(str(error)) from error


def markdown_fragment(text: str) -> str:
    """Render the deliberately small markdown subset used by procedure docs."""
    lines = text.strip().splitlines()
    out: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        if not line.strip():
            index += 1
            continue
        if line.startswith("```mermaid"):
            index += 1
            mermaid = []
            while index < len(lines) and not lines[index].startswith("```"):
                mermaid.append(lines[index]); index += 1
            index += 1
            out.append(f'<div class="mermaid-source">{html.escape("\n".join(mermaid))}</div>')
            continue
        if line.startswith("|") and index + 1 < len(lines) and lines[index + 1].startswith("|"):
            rows = []
            while index < len(lines) and lines[index].startswith("|"):
                cells = [c.strip() for c in lines[index].strip().strip("|").split("|")]
                if not all(set(c) <= set("-: ") for c in cells):
                    rows.append(cells)
                index += 1
            if rows:
                out.append("<table><thead><tr>" + "".join(f"<th>{html.escape(c)}</th>" for c in rows[0]) + "</tr></thead><tbody>" +
                           "".join("<tr>" + "".join(f"<td>{html.escape(c)}</td>" for c in row) + "</tr>" for row in rows[1:]) + "</tbody></table>")
            continue
        heading = re.match(r"^(#{1,6})\s+(.+)$", line)
        if heading:
            level = len(heading.group(1)); out.append(f"<h{level}>{html.escape(heading.group(2))}</h{level}>"); index += 1; continue
        if line.startswith("- "):
            items = []
            while index < len(lines) and lines[index].startswith("- "):
                items.append(f"<li>{html.escape(lines[index][2:])}</li>"); index += 1
            out.append("<ul>" + "".join(items) + "</ul>"); continue
        paragraph = [line]
        index += 1
        while index < len(lines) and lines[index].strip() and not lines[index].startswith(("#", "|", "```", "- ")):
            paragraph.append(lines[index]); index += 1
        value = html.escape(" ".join(paragraph))
        value = re.sub(r"`([^`]+)`", r"<code>\1</code>", value)
        out.append(f"<p>{value}</p>")
    return "\n".join(out)


def parse_document(path: Path) -> tuple[dict[str, Any], str, dict[str, str]]:
    text = path.read_text(encoding="utf-8")
    if not text.startswith("---\n"):
        raise ProcedureRenderError(f"{path}: missing YAML front matter")
    end = text.find("\n---\n", 4)
    if end < 0:
        raise ProcedureRenderError(f"{path}: unterminated YAML front matter")
    metadata: dict[str, Any] = {"revisions": []}
    current: dict[str, str] | None = None
    for line in text[4:end].splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if stripped == "revisions:":
            continue
        if stripped.startswith("- rev:"):
            current = {"rev": stripped.split(":", 1)[1].strip()}
            metadata["revisions"].append(current)
            continue
        if current is not None and line.startswith("    ") and ":" in stripped:
            key, value = stripped.split(":", 1)
            current[key.strip()] = value.strip().strip('"')
            continue
        if ":" in stripped:
            key, value = stripped.split(":", 1)
            metadata[key.strip()] = value.strip().strip('"')
    if not isinstance(metadata, dict) or not isinstance(metadata.get("revisions"), list):
        raise ProcedureRenderError(f"{path}: revisions must be a YAML list")
    sections: dict[str, str] = {}
    matches = list(re.finditer(r"^## Revision ([0-9a-f]{8}) \((\d{4}-\d{2}-\d{2})\)\s*$", text[end + 6:], re.MULTILINE))
    # The expression above intentionally accepts only the documented section shape.
    for number, match in enumerate(matches):
        start = end + 6 + match.end()
        stop = end + 6 + matches[number + 1].start() if number + 1 < len(matches) else len(text)
        sections[match.group(1)] = text[start:stop]
    body = text[end + 6:]
    if matches:
        body = body[:matches[0].start()]
    return metadata, body, sections


def mermaid_svg(source: str) -> str:
    try:
        return render_mermaid_source(source)
    except Exception as error:
        raise ProcedureRenderError(str(error)) from error


def revision_content(section: str) -> tuple[str, str]:
    flow = re.search(r"```mermaid\s*\n(.*?)\n```", section, re.DOTALL)
    if not flow:
        raise ProcedureRenderError("revision section is missing a mermaid flow")
    steps = re.search(r"## Steps\s*\n(.*?)(?=\n## |\Z)", section, re.DOTALL)
    if not steps:
        raise ProcedureRenderError("revision section is missing a Steps table")
    return flow.group(1).strip(), steps.group(1).strip()


def summary_content(body: str) -> str:
    """Keep narrative sections in the page body; revision markup is rendered once."""
    return re.sub(
        r"\n## (?:Flow|Steps)\s*\n.*?(?=\n## |\Z)",
        "",
        body,
        flags=re.DOTALL,
    )


def render(root: Path = ROOT, check: bool = False) -> int:
    docs = root / "docs/procedures"; output = root / "site/reports/procedures"
    expected: dict[Path, str] = {}
    manifest: list[dict[str, Any]] = []
    for path in sorted(docs.glob("*.md")):
        metadata, body, sections = parse_document(path)
        procedure = metadata.get("procedure")
        revisions = metadata.get("revisions")
        if not isinstance(procedure, str) or not isinstance(revisions, list):
            raise ProcedureRenderError(f"{path}: procedure and revisions are required")
        entries: list[dict[str, Any]] = []
        for item in revisions:
            if not isinstance(item, dict) or not isinstance(item.get("rev"), str) or not SHA.fullmatch(item["rev"]):
                raise ProcedureRenderError(f"{path}: invalid revision entry")
            rev = item["rev"]; rev8 = rev[:8]
            if rev8 not in sections:
                raise ProcedureRenderError(f"{path}: revision {rev8} has no matching section")
            flow, steps = revision_content(sections[rev8])
            svg = mermaid_svg(flow)
            title = f"{procedure} procedure — revision {rev8}"
            vars_obj = {"title": title, "procedure": procedure, "revision": rev, "revision_short": rev8,
                        "date": str(item.get("date", "")), "note": str(item.get("note", "")),
                        "flow_svg": svg, "steps_html": markdown_fragment(steps),
                        "body_html": markdown_fragment(summary_content(body))}
            destination = output / procedure / f"{rev8}.html"
            destination.parent.mkdir(parents=True, exist_ok=True)
            expected[destination] = compose(root / "templates/procedure-report/procedure.html.j2", vars_obj, destination)
            entry_date = item.get("date")
            entries.append({"rev": rev, "date": entry_date.isoformat() if isinstance(entry_date, date) else str(entry_date or ""), "note": item.get("note", ""), "html": f"procedures/{procedure}/{rev8}.html"})
        if {key for key in sections} != {str(item["rev"])[:8] for item in revisions}:
            raise ProcedureRenderError(f"{path}: revision section and front matter entries differ")
        index_rows = "".join(f'<li><a href="{html.escape(item["rev"][:8])}.html">revision {html.escape(item["rev"][:8])}</a> — {html.escape(str(item.get("date", "")))} — {html.escape(str(item.get("note", "")))}</li>' for item in entries)
        expected[output / procedure / "index.html"] = f'<!doctype html><html><head><meta charset="utf-8"><title>{html.escape(procedure)}</title></head><body><h1>{html.escape(procedure)}</h1><ul>{index_rows}</ul></body></html>\n'
        manifest.append({"procedure": procedure, "family": metadata.get("family"), "runner": metadata.get("runner"), "revisions": entries})
    manifest_text = json.dumps({"schema_version": 1, "procedures": manifest}, indent=2, sort_keys=False) + "\n"
    expected[output / "manifest.json"] = manifest_text
    existing = {p: p.read_text(encoding="utf-8") for p in output.rglob("*") if p.is_file()} if output.exists() else {}
    if check:
        return 0 if existing == expected else 1
    for path, content in expected.items():
        path.parent.mkdir(parents=True, exist_ok=True); path.write_text(content, encoding="utf-8")
    for path in set(existing) - set(expected):
        path.unlink()
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(); parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try: return render(check=args.check)
    except ProcedureRenderError as error:
        print(f"procedures: error: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
