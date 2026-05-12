#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, UTC
from html import escape
from pathlib import Path
import json
import os
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[4]
ATM_DOCS = ROOT / "docs" / "atm"
SQLITE_DOCS = ROOT / "docs" / "atm-rusqlite"
DIAGRAMS_DIR = ATM_DOCS / "diagrams"
PANELS_DIR = ATM_DOCS / "diagrams" / "panels"
TEMPLATES_DIR = DIAGRAMS_DIR / "templates"
METADATA_RE = re.compile(r"^%%\s*([a-z_]+)\s*:\s*(.+?)\s*$")
TABLE_RE = re.compile(
    r"CREATE TABLE IF NOT EXISTS\s+([a-zA-Z0-9_]+)\s*\((.*?)\n\);",
    re.DOTALL,
)


@dataclass(frozen=True)
class Panel:
    key: str
    title: str
    summary: str
    commentary: str
    notes: tuple[str, ...]
    read_tables: tuple[str, ...]
    write_tables: tuple[str, ...]
    sets: tuple[str, ...]
    ssot_path: str
    source_text: str
    fragment_output: Path


@dataclass(frozen=True)
class Page:
    set_name: str
    title: str
    intro: str
    output_path: Path
    json_output_path: Path
    stylesheet_href: str
    script_href: str
    panels: tuple[Panel, ...]


PAGE_CONFIG = {
    "cli": {
        "title": "CLI Interface",
        "intro": "These panels show the retained CLI command surfaces and their target command-to-daemon or command-to-service flows, including error paths.",
        "output_path": ATM_DOCS / "cli-diagrams.html",
        "json_output_path": ATM_DOCS / "cli-diagrams.json",
        "stylesheet_href": "./diagrams/assets/diagram-panels.css",
        "script_href": "./diagrams/assets/diagram-panels.js",
    },
    "client": {
        "title": "Client Interface",
        "intro": "These panels cover the daemon message surfaces used by thin clients such as atm-graft. Shared command flows reuse the same diagrams where the packet contract is identical.",
        "output_path": ATM_DOCS / "client-interface-diagrams.html",
        "json_output_path": ATM_DOCS / "client-interface-diagrams.json",
        "stylesheet_href": "./diagrams/assets/diagram-panels.css",
        "script_href": "./diagrams/assets/diagram-panels.js",
    },
    "query": {
        "title": "SQL Queries",
        "intro": "SQLite is the mailbox SSOT. These panels show the target query shapes: status-first selection, deleted-row exclusion in normal queries, and full message fetch only when a message must actually be rendered.",
        "output_path": SQLITE_DOCS / "query-diagrams.html",
        "json_output_path": SQLITE_DOCS / "query-diagrams.json",
        "stylesheet_href": "../atm/diagrams/assets/diagram-panels.css",
        "script_href": "../atm/diagrams/assets/diagram-panels.js",
    },
}


def rel_from_root(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def render_file(template: Path, vars_obj: dict, output: Path) -> None:
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
        json.dump(vars_obj, handle)
        handle.flush()
        var_file = Path(handle.name)
    try:
        subprocess.run(
            [
                "sc-compose",
                "render",
                "--root",
                str(ROOT),
                "--file",
                str(template),
                "--var-file",
                str(var_file),
                "--output",
                str(output),
            ],
            check=True,
            cwd=ROOT,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    finally:
        var_file.unlink(missing_ok=True)


def render_string(template: Path, vars_obj: dict) -> str:
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
        json.dump(vars_obj, handle)
        handle.flush()
        var_file = Path(handle.name)
    try:
        result = subprocess.run(
            [
                "sc-compose",
                "render",
                "--root",
                str(ROOT),
                "--file",
                str(template),
                "--var-file",
                str(var_file),
            ],
            check=True,
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        return result.stdout
    finally:
        var_file.unlink(missing_ok=True)


def parse_metadata(source_path: Path) -> dict[str, str]:
    metadata: dict[str, str] = {}
    for line in source_path.read_text().splitlines():
        match = METADATA_RE.match(line)
        if not match:
            if line.startswith("%%"):
                continue
            break
        metadata[match.group(1)] = match.group(2).strip()
    return metadata


def normalized_value(raw: str) -> str:
    value = raw.strip()
    if len(value) >= 2 and value[0] == "`" and value[-1] == "`":
        return value[1:-1]
    return value


def parse_csv_metadata(raw: str) -> tuple[str, ...]:
    return tuple(item.strip() for item in raw.split(",") if item.strip())


def schema_map() -> dict[str, str]:
    shared_db = (
        ROOT
        / "crates"
        / "atm-rusqlite"
        / "src"
        / "shared_db.rs"
    ).read_text()
    schema: dict[str, str] = {}
    for match in TABLE_RE.finditer(shared_db):
        table = match.group(1)
        ddl = f"CREATE TABLE IF NOT EXISTS {table} ({match.group(2).rstrip()}\n);"
        schema[table] = ddl
    return schema


def all_panels() -> tuple[Panel, ...]:
    panels = []
    for source_path in sorted(DIAGRAMS_DIR.glob("*.mmd")):
        metadata = parse_metadata(source_path)
        missing = {"title", "summary", "commentary", "sets"} - set(metadata)
        if missing:
            missing_list = ", ".join(sorted(missing))
            raise ValueError(f"{source_path} missing metadata: {missing_list}")

        sets = tuple(item.strip() for item in metadata["sets"].split(",") if item.strip())
        if not sets:
            raise ValueError(f"{source_path} has empty sets metadata")
        notes = tuple(
            item.strip() for item in metadata.get("notes", "").split("|") if item.strip()
        )
        read_tables = parse_csv_metadata(metadata.get("read_tables", ""))
        write_tables = parse_csv_metadata(metadata.get("write_tables", ""))

        panels.append(
            Panel(
                key=source_path.stem,
                title=normalized_value(metadata["title"]),
                summary=normalized_value(metadata["summary"]),
                commentary=normalized_value(metadata["commentary"]),
                notes=notes,
                read_tables=read_tables,
                write_tables=write_tables,
                sets=sets,
                ssot_path=rel_from_root(source_path),
                source_text=source_path.read_text(),
                fragment_output=PANELS_DIR / f"{source_path.stem}.html",
            )
        )

    if not panels:
        raise ValueError("No .mmd diagrams found")
    return tuple(panels)


def pages_from_panels(panels: tuple[Panel, ...]) -> tuple[Page, ...]:
    pages = []
    for set_name, config in PAGE_CONFIG.items():
        page_panels = tuple(panel for panel in panels if set_name in panel.sets)
        if not page_panels:
            continue
        pages.append(
            Page(
                set_name=set_name,
                title=config["title"],
                intro=config["intro"],
                output_path=config["output_path"],
                json_output_path=config["json_output_path"],
                stylesheet_href=config["stylesheet_href"],
                script_href=config["script_href"],
                panels=page_panels,
            )
        )
    return tuple(pages)


def render_svg(panel: Panel) -> str:
    with tempfile.TemporaryDirectory() as tmp_dir:
        svg_path = Path(tmp_dir) / f"{panel.key}.svg"
        subprocess.run(
            [
                "npx",
                "-y",
                "@mermaid-js/mermaid-cli",
                "-i",
                str(ROOT / panel.ssot_path),
                "-o",
                str(svg_path),
            ],
            check=True,
            cwd=ROOT,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return svg_path.read_text()


def build_copy_payload(report_title: str, panel: Panel) -> str:
    diagram_lines = []
    metadata_prefix = True
    for line in panel.source_text.splitlines():
        if metadata_prefix and line.startswith("%%"):
            continue
        if metadata_prefix and not line.strip():
            continue
        metadata_prefix = False
        diagram_lines.append(line)
    diagram_source = "\n".join(diagram_lines).strip()

    return "\n".join(
        [
            f"{report_title} - {panel.title}",
            "",
            "In this diagram:",
            "",
            "```mermaid",
            diagram_source,
            "```",
            "",
            "Commentary:",
            panel.commentary,
        ]
    )


def fragment_href(page: Page, panel: Panel) -> str:
    return os.path.relpath(panel.fragment_output, page.output_path.parent)


def render_section_html(
    report_title: str,
    panel: Panel,
    href: str | None,
    schemas: dict[str, str],
) -> str:
    table_names = tuple(dict.fromkeys(panel.read_tables + panel.write_tables))
    dialog_ids = {
        table_name: f"schema-{panel.key}-{table_name}"
        for table_name in table_names
    }

    def buttons_html(label: str, tables: tuple[str, ...]) -> str:
        if not tables:
            return ""
        buttons = " ".join(
            f'<button class="schema-link" type="button" data-dialog-id="{escape(dialog_ids[table_name])}">{escape(table_name)}</button>'
            for table_name in tables
        )
        return f"<p><strong>{escape(label)}:</strong> {buttons}</p>"

    tables_html = ""
    if panel.read_tables or panel.write_tables:
        parts = ['<div class="diagram-tables"><h3>Tables</h3>']
        read_html = buttons_html("Read", panel.read_tables)
        write_html = buttons_html("Written", panel.write_tables)
        if read_html:
            parts.append(read_html)
        if write_html:
            parts.append(write_html)
        parts.append("</div>")
        tables_html = "".join(parts)

    schema_dialogs_html = "".join(
        (
            f'<dialog class="schema-dialog" id="{escape(dialog_ids[table_name])}">'
            '<form method="dialog" class="schema-dialog-head">'
            f"<h3>{escape(table_name)}</h3>"
            '<button class="icon-button" value="close" title="Close schema">×</button>'
            "</form>"
            '<p class="schema-source">SSOT: <code>crates/atm-rusqlite/src/shared_db.rs</code></p>'
            f'<pre class="schema-ddl"><code>{escape(schemas.get(table_name, f"-- schema for {table_name} not found"))}</code></pre>'
            "</dialog>"
        )
        for table_name in table_names
    )

    return render_string(
        TEMPLATES_DIR / "section-fragment.html.j2",
        {
            "title": panel.title,
            "summary": panel.summary,
            "commentary": panel.commentary,
            "notes": list(panel.notes),
            "tables_html": tables_html,
            "schema_dialogs_html": schema_dialogs_html,
            "ssot_path": panel.ssot_path,
            "svg_markup": render_svg(panel),
            "copy_text": build_copy_payload(report_title, panel),
            "fragment_href": href or "",
        },
    )


def render_fragments(pages: tuple[Page, ...], schemas: dict[str, str]) -> dict[str, str]:
    PANELS_DIR.mkdir(parents=True, exist_ok=True)
    for stale in PANELS_DIR.glob("*"):
        if stale.is_file():
            stale.unlink()

    rendered: dict[str, str] = {}
    all_panels = {panel.key: panel for page in pages for panel in page.panels}
    for panel in all_panels.values():
        section_html = render_section_html("Diagram Panel", panel, None, schemas)
        render_file(
            TEMPLATES_DIR / "fragment-page.html.j2",
            {
                "title": panel.title,
                "stylesheet_href": "../assets/diagram-panels.css",
                "script_href": "../assets/diagram-panels.js",
                "section_html": section_html,
            },
            panel.fragment_output,
        )
        rendered[panel.key] = section_html
    return rendered


def render_pages(pages: tuple[Page, ...], schemas: dict[str, str]) -> None:
    for page in pages:
        sections_html = "\n".join(
            render_section_html(page.title, panel, fragment_href(page, panel), schemas)
            for panel in page.panels
        )
        render_file(
            TEMPLATES_DIR / "report-page.html.j2",
            {
                "title": page.title,
                "intro": page.intro,
                "stylesheet_href": page.stylesheet_href,
                "script_href": page.script_href,
                "sections_html": sections_html,
            },
            page.output_path,
        )
        write_sidecar(page)


def write_sidecar(page: Page) -> None:
    payload = {
        "title": page.title,
        "generated_at": datetime.now(UTC).isoformat(),
        "set": page.set_name,
        "output_path": rel_from_root(page.output_path),
        "sections": [
            {
                "key": panel.key,
                "title": panel.title,
                "summary": panel.summary,
                "commentary": panel.commentary,
                "notes": list(panel.notes),
                "read_tables": list(panel.read_tables),
                "write_tables": list(panel.write_tables),
                "ssot_path": panel.ssot_path,
                "fragment_path": rel_from_root(panel.fragment_output),
                "sets": list(panel.sets),
            }
            for panel in page.panels
        ],
    }
    page.json_output_path.write_text(json.dumps(payload, indent=2) + "\n")


def main() -> None:
    panels = all_panels()
    pages = pages_from_panels(panels)
    schemas = schema_map()
    render_fragments(pages, schemas)
    render_pages(pages, schemas)


if __name__ == "__main__":
    main()
