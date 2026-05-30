#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import html
import json
import subprocess
import sys
from typing import Any

from lint_common import discover_repo_root
from view_common import VIEW_ROOT
from view_common import relative_artifact_path
from view_common import write_json


TEMPLATE_DIR = Path(".just/templates")
STATUS_CLASSES = {
    "PASS": "status-pass",
    "INFO": "status-info",
    "ERROR": "status-error",
    "NOT_SUPPORTED": "status-not-supported",
}
PANEL_ORDER = ("boundaries", "lines", "deps", "modules", "unsafe")


@dataclass(frozen=True)
class ToolPanel:
    tool_id: str
    title: str
    status: str
    summary: str
    context_text: str
    body_html: str
    json_payload: dict[str, Any]
    panel_path: Path


def status_class(status: str) -> str:
    return STATUS_CLASSES.get(status, "status-info")


def read_json(path: Path) -> dict[str, Any] | None:
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))

def latest_view_log(repo_root: Path, tool_name: str) -> Path | None:
    log_dir = repo_root / ".just/logs"
    matches = sorted(log_dir.glob(f"*-view-{tool_name}.log"))
    return matches[-1] if matches else None


def href(base_path: Path, target_path: Path) -> str:
    return target_path.resolve().as_uri()


def html_links(base_path: Path, links: list[tuple[str, Path]]) -> str:
    if not links:
        return "<p>None.</p>"
    items = []
    for label, path in links:
        items.append(f'<li><a href="{html.escape(href(base_path, path))}">{html.escape(label)}</a></li>')
    return "<ul>" + "".join(items) + "</ul>"


def html_table(rows: list[dict[str, str]], columns: list[tuple[str, str]]) -> str:
    if not rows:
        return "<p>No rows.</p>"
    head = "".join(f"<th>{html.escape(label)}</th>" for _key, label in columns)
    body_rows = []
    for row in rows:
        cells = "".join(f"<td>{html.escape(str(row.get(key, '')))}</td>" for key, _label in columns)
        body_rows.append(f"<tr>{cells}</tr>")
    return f"<table><thead><tr>{head}</tr></thead><tbody>{''.join(body_rows)}</tbody></table>"


def failure_block(message: str) -> str:
    return f'<div class="warning"><strong>Blocked:</strong> {html.escape(message)}</div>'


def build_boundaries_panel(repo_root: Path, panel_path: Path) -> ToolPanel:
    tool_dir = repo_root / VIEW_ROOT / "boundaries"
    summary_json_path = tool_dir / "summary.json"
    summary_txt_path = tool_dir / "summary.txt"
    findings_path = tool_dir / "findings.txt"
    summary = read_json(summary_json_path) or {"docs": [], "record_count": 0, "violation_count": 0}
    doc_rows = summary.get("docs", [])
    violation_count = int(summary.get("violation_count", 0))
    status = "PASS" if violation_count == 0 and summary_json_path.exists() else "ERROR"
    overview = f"{summary.get('record_count', 0)} boundary records across {len(doc_rows)} documents."
    body = [
        f"<p>{html.escape(overview)}</p>",
        html_table(
            doc_rows,
            [("doc", "Doc"), ("records", "Records"), ("active", "Active"), ("planned", "Planned"), ("retired", "Retired")],
        ),
        "<h4>Artifacts</h4>",
        html_links(
            panel_path,
            [
                ("summary.txt", summary_txt_path),
                ("summary.json", summary_json_path),
                ("findings.txt", findings_path),
            ],
        ),
    ]
    return ToolPanel(
        tool_id="boundaries",
        title="Boundary Inventory",
        status=status,
        summary=f"{len(doc_rows)} docs, {summary.get('record_count', 0)} records, {violation_count} violations",
        context_text=f"Boundary inventory analyzed {len(doc_rows)} docs with {summary.get('record_count', 0)} records and {violation_count} violations.",
        body_html="".join(body),
        json_payload=summary,
        panel_path=panel_path,
    )


def build_deps_panel(repo_root: Path, panel_path: Path) -> ToolPanel:
    tool_dir = repo_root / VIEW_ROOT / "deps"
    summary_json_path = tool_dir / "summary.json"
    report_json_path = tool_dir / "report.json"
    index_html_path = tool_dir / "index.html"
    report_html_path = tool_dir / "report.html"
    summary = read_json(summary_json_path) or {}
    report = read_json(report_json_path) or {}
    report_summary = report.get("summary", {})
    duplicates = report.get("diagnostics", {}).get("duplicates", [])
    duplicate_rows = [
        {"crate": entry["name"], "versions": ", ".join(entry.get("versions", []))}
        for entry in duplicates
    ]
    body = [
        f"<p>{html.escape(str(report_summary.get('total_dependencies', 'unknown')))} total dependencies, "
        f"{html.escape(str(report_summary.get('unique_crates', 'unknown')))} unique crates, "
        f"{html.escape(str(report_summary.get('duplicate_crates', 'unknown')))} duplicates.</p>",
        "<h4>Duplicate crates</h4>",
        html_table(duplicate_rows, [("crate", "Crate"), ("versions", "Versions")]),
        "<h4>Artifacts</h4>",
        html_links(
            panel_path,
            [
                ("dependency graph HTML", index_html_path),
                ("analysis report HTML", report_html_path),
                ("analysis report JSON", report_json_path),
                ("summary.txt", tool_dir / "summary.txt"),
                ("summary.json", summary_json_path),
            ],
        ),
    ]
    return ToolPanel(
        tool_id="deps",
        title="Dependency Graph",
        status="PASS" if index_html_path.exists() and report_json_path.exists() else "ERROR",
        summary=f"{report_summary.get('total_dependencies', 'unknown')} total deps, {report_summary.get('duplicate_crates', 'unknown')} duplicates",
        context_text=f"Dependency visualization reports {report_summary.get('total_dependencies', 'unknown')} total dependencies and {report_summary.get('duplicate_crates', 'unknown')} duplicate crates.",
        body_html="".join(body),
        json_payload={"summary": summary, "report": report},
        panel_path=panel_path,
    )


def build_lines_panel(repo_root: Path, panel_path: Path) -> ToolPanel:
    tool_dir = repo_root / VIEW_ROOT / "lines"
    summary_json_path = tool_dir / "summary.json"
    summary_txt_path = tool_dir / "summary.txt"
    table_txt_path = tool_dir / "table.txt"
    summary = read_json(summary_json_path) or {"limits": {"summary": "unknown"}, "files": [], "crate_totals": []}
    totals = summary.get("crate_totals", [])
    body = [
        f"<p>Active limits: {html.escape(str(summary.get('limits', {}).get('summary', 'unknown')))}</p>",
        html_table(
            [{k: str(v) for k, v in row.items()} for row in totals],
            [("crate", "Crate"), ("total", "Total"), ("prod", "Prod"), ("test", "Test"), ("prod_test", "Prod+Test")],
        ),
        "<h4>Artifacts</h4>",
        html_links(
            panel_path,
            [
                ("summary.txt", summary_txt_path),
                ("table.txt", table_txt_path),
                ("summary.json", summary_json_path),
            ],
        ),
    ]
    return ToolPanel(
        tool_id="lines",
        title="Source Size Inventory",
        status="PASS" if summary_json_path.exists() else "ERROR",
        summary=f"{len(totals)} crates, {len(summary.get('files', []))} files",
        context_text=f"Source size inventory covers {len(totals)} crates and {len(summary.get('files', []))} files.",
        body_html="".join(body),
        json_payload=summary,
        panel_path=panel_path,
    )


def crate_rows_from_index(index_data: dict[str, Any] | None) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    for entry in (index_data or {}).get("crates", []):
        rows.append(
            {
                "crate": str(entry.get("crate", "")),
                "package": str(entry.get("package", "")),
                "structure": str(entry.get("structure", "")),
                "dependencies": str(entry.get("dependencies", "")),
                "svg": str(entry.get("svg", "")),
            }
        )
    return rows


def build_modules_panel(repo_root: Path, panel_path: Path) -> ToolPanel:
    tool_dir = repo_root / VIEW_ROOT / "modules"
    index_json_path = tool_dir / "index.json"
    index_txt_path = tool_dir / "index.txt"
    index_data = read_json(index_json_path)
    rows = crate_rows_from_index(index_data)
    latest_log = latest_view_log(repo_root, "modules")
    failure_message = ""
    if latest_log is not None:
        lines = [line.strip() for line in latest_log.read_text(encoding="utf-8").splitlines() if line.strip()]
        if "exit_code: 0" not in lines:
            for line in lines:
                if "Assertion failed:" in line or "failed" in line.lower():
                    failure_message = line
                    break
    artifact_links: list[tuple[str, Path]] = []
    if index_txt_path.exists():
        artifact_links.append(("index.txt", index_txt_path))
    if index_json_path.exists():
        artifact_links.append(("index.json", index_json_path))
    for crate_entry in rows:
        crate_dir = tool_dir / crate_entry["crate"]
        if (crate_dir / "structure.txt").exists():
            artifact_links.append((f'{crate_entry["crate"]} structure.txt', crate_dir / "structure.txt"))
        if (crate_dir / "dependencies.dot").exists():
            artifact_links.append((f'{crate_entry["crate"]} dependencies.dot', crate_dir / "dependencies.dot"))
        if crate_entry.get("svg") and (crate_dir / "dependencies.svg").exists():
            artifact_links.append((f'{crate_entry["crate"]} dependencies.svg', crate_dir / "dependencies.svg"))
    body_parts = []
    if failure_message:
        body_parts.append(failure_block(failure_message))
    if rows:
        body_parts.append(
            html_table(rows, [("crate", "Crate"), ("package", "Package"), ("structure", "Structure"), ("dependencies", "DOT"), ("svg", "SVG")])
        )
    else:
        body_parts.append("<p>No completed module index was generated.</p>")
    body_parts.extend(["<h4>Artifacts</h4>", html_links(panel_path, artifact_links)])
    status = "PASS" if index_json_path.exists() and not failure_message else "ERROR"
    summary = f"{len(rows)} crates indexed" if rows else "No completed module report"
    if failure_message:
        summary += "; Graphviz rendering currently blocked"
    return ToolPanel(
        tool_id="modules",
        title="Module Structure",
        status=status,
        summary=summary,
        context_text=summary,
        body_html="".join(body_parts),
        json_payload={"index": index_data, "latest_log": relative_artifact_path(repo_root, latest_log) if latest_log else None},
        panel_path=panel_path,
    )


def unsafe_rows_from_index(index_data: dict[str, Any] | None) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    for entry in (index_data or {}).get("crates", []):
        rows.append(
            {
                "crate": str(entry.get("crate", "")),
                "package": str(entry.get("package", "")),
                "text": str(entry.get("text", "")),
                "json": str(entry.get("json", "")),
            }
        )
    return rows


def build_unsafe_panel(repo_root: Path, panel_path: Path) -> ToolPanel:
    tool_dir = repo_root / VIEW_ROOT / "unsafe"
    index_json_path = tool_dir / "index.json"
    index_txt_path = tool_dir / "index.txt"
    index_data = read_json(index_json_path)
    latest_log = latest_view_log(repo_root, "unsafe")
    failure_message = ""
    if latest_log is not None:
        for line in latest_log.read_text(encoding="utf-8").splitlines():
            if "Failed to match" in line:
                failure_message = line.strip()
                break
    rows = unsafe_rows_from_index(index_data)
    artifact_links: list[tuple[str, Path]] = []
    if index_txt_path.exists():
        artifact_links.append(("index.txt", index_txt_path))
    if index_json_path.exists():
        artifact_links.append(("index.json", index_json_path))
    body_parts = []
    if failure_message:
        body_parts.append(failure_block(failure_message))
    if rows:
        body_parts.append(html_table(rows, [("crate", "Crate"), ("package", "Package"), ("text", "Text"), ("json", "JSON")]))
    else:
        body_parts.append("<p>No completed unsafe report artifacts exist yet.</p>")
    if latest_log is not None:
        artifact_links.append(("latest unsafe log", latest_log))
    body_parts.extend(["<h4>Artifacts</h4>", html_links(panel_path, artifact_links)])
    summary = "cargo-geiger blocked on package resolution"
    if rows:
        summary = f"{len(rows)} crates indexed"
    return ToolPanel(
        tool_id="unsafe",
        title="Unsafe Surface",
        status="ERROR",
        summary=summary,
        context_text=summary,
        body_html="".join(body_parts),
        json_payload={"index": index_data, "latest_log": relative_artifact_path(repo_root, latest_log) if latest_log else None},
        panel_path=panel_path,
    )


def panel_builders() -> dict[str, Any]:
    return {
        "boundaries": build_boundaries_panel,
        "lines": build_lines_panel,
        "deps": build_deps_panel,
        "modules": build_modules_panel,
        "unsafe": build_unsafe_panel,
    }


def section_html(panel: ToolPanel, index_path: Path) -> str:
    panel_link = href(index_path, panel.panel_path)
    payload_json = json.dumps(panel.json_payload, indent=2, sort_keys=True)
    return f"""
  <section class="section">
    <div class="section-head">
      <div>
        <h2>{html.escape(panel.title)}</h2>
        <p><strong>{html.escape(panel.summary)}</strong></p>
      </div>
      <div class="section-actions">
        <button class="icon-button" title="Copy JSON" data-copy-text='{html.escape(payload_json)}'>{{}}</button>
        <button class="icon-button" title="Copy Context" data-copy-text='{html.escape(panel.context_text)}'>⎘</button>
      </div>
    </div>
    <div class="status {status_class(panel.status)}">{html.escape(panel.status)}</div>
    <div class="fragments">
      <a href="{html.escape(panel_link)}">Open XHTML panel</a>
    </div>
    {panel.body_html}
  </section>
""".rstrip()


def render_template(repo_root: Path, template_path: Path, var_path: Path, output_path: Path) -> None:
    result = subprocess.run(
        [
            "sc-compose",
            "render",
            "--root",
            str(repo_root),
            "--file",
            str(template_path),
            "--var-file",
            str(var_path),
            "--strict",
            "--output",
            str(output_path),
        ],
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(result.stderr.strip() or result.stdout.strip())


def validate_xhtml(path: Path) -> None:
    result = subprocess.run(
        ["xmllint", "--noout", str(path)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(result.stderr.strip() or result.stdout.strip())


def build_site(repo_root: Path) -> Path:
    view_root = repo_root / VIEW_ROOT
    panels_dir = view_root / "panels"
    site_data_dir = view_root / "site-data"
    panels_dir.mkdir(parents=True, exist_ok=True)
    site_data_dir.mkdir(parents=True, exist_ok=True)

    template_report = repo_root / TEMPLATE_DIR / "view-report.html.j2"
    template_panel = repo_root / TEMPLATE_DIR / "view-panel.xhtml.j2"
    index_path = view_root / "index.html"
    index_json_path = view_root / "index.json"

    panels: list[ToolPanel] = []
    for tool_id in PANEL_ORDER:
        panel_path = panels_dir / f"{tool_id}.xhtml"
        panel = panel_builders()[tool_id](repo_root, panel_path)
        panels.append(panel)
        panel_vars = {
            "title": panel.title,
            "header_color": "#1e40af",
            "accent_color": "#3b82f6",
            "fragment_source": "auto-generated",
            "copy_json": html.escape(json.dumps(panel.json_payload, indent=2, sort_keys=True), quote=True),
            "copy_context": html.escape(panel.context_text, quote=True),
            "body_html": panel.body_html,
        }
        panel_var_path = site_data_dir / f"{tool_id}.json"
        write_json(panel_var_path, panel_vars)
        render_template(repo_root, template_panel, panel_var_path, panel_path)
        validate_xhtml(panel_path)

    statuses = {panel.status for panel in panels}
    overall_status = "PASS" if statuses == {"PASS"} else "INFO" if "PASS" in statuses else "ERROR"
    sections_html = "\n".join(section_html(panel, index_path) for panel in panels)
    summary_html = (
        "<p>Architecture view index linking the current boundary, dependency, module, and unsafe-surface artifacts.</p>"
        "<p>Supported today: boundaries, lines, and dependency graph. Modules has partial raw output, and unsafe remains blocked by cargo-geiger.</p>"
    )
    recommendations_html = (
        "<ul>"
        "<li>Use the dependency graph HTML first for package-level structure.</li>"
        "<li>Use the boundary panel for current document coverage and validation state.</li>"
        "<li>Use the lines panel for crate-level size pressure and line-count limits.</li>"
        "<li>Treat module and unsafe panels as current-state diagnostics until those generators stabilize.</li>"
        "</ul>"
    )
    footer_html = "<p>Generated from current artifacts under <code>artifacts/view/</code>.</p>"
    report_model = {
        "output_path": str(index_path),
        "json_output_path": str(index_json_path),
        "title": "Architecture View Index",
        "subtitle": "Boundary, dependency, module, and unsafe-surface artifacts",
        "status": overall_status,
        "status_class": status_class(overall_status),
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "summary_html": summary_html,
        "sections_html": sections_html,
        "recommendations_html": recommendations_html,
        "footer_html": footer_html,
    }
    report_sidecar = {
        **report_model,
        "sections": [
            {
                "id": panel.tool_id,
                "title": panel.title,
                "status": panel.status,
                "summary": panel.summary,
                "context_text": panel.context_text,
                "json_payload": panel.json_payload,
                "xhtml_path": relative_artifact_path(repo_root, panel.panel_path),
            }
            for panel in panels
        ],
    }
    report_var_path = site_data_dir / "index.json"
    write_json(report_var_path, report_model)
    render_template(repo_root, template_report, report_var_path, index_path)
    write_json(index_json_path, report_sidecar)
    return index_path


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Build XHTML panels and a static HTML index for architecture view artifacts.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    output_path = build_site(repo_root)
    print(f"view site generated: {relative_artifact_path(repo_root, output_path)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
