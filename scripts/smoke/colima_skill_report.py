#!/usr/bin/env python3
"""Render one colima skill run (atm-hermes-testbed ``./test.sh``) as smoke-report evidence.

Input is the run directory ``test.sh`` leaves behind, or the evidence directory already committed
under ``site/reports``: ``result.txt``, ``report-N.txt`` (the seven skill reports as received) and
``herdr-doctor.json``. Those files are copied unchanged. Beside them this writes what every other
smoke run has: ``<feature>.json`` (the cases), ``<host>-<feature>.xhtml`` (the evidence pane),
``<feature>.html`` and ``index.html`` (frames) and ``smoke.envelope.json`` for the report index, all
rendered through the sc-compose templates under ``templates/smoke-report``.

    python3 scripts/smoke/colima_skill_report.py <run-dir> [--out <site/reports/smoke/...>]
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
from html import escape
import json
import os
from pathlib import Path
import re
import shutil
import sys
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from feature_smoke_report import render_feature_pane  # noqa: E402
from run_feature_smoke import update_master_report_index  # noqa: E402
from run_inbound_peer_smoke import PANE_TEMPLATE, REPO_ROOT  # noqa: E402
from scripts.report_runtime import (  # noqa: E402
    compose as _compose,
    resolve_procedure_page as _resolve_procedure_page,
    source_revision as _git_source_revision,
)
from smoke_common import SmokeError  # noqa: E402

FEATURE = "colima-hermes-skills"
PLATFORM = "linux"
HOST = "hermes-testbed"
RAW_FILES = ("result.txt", "herdr-doctor.json")
STEP = re.compile(r"^\s+\d+ (PASS|FAIL) (.*)$")
FIELD = re.compile(r"^(skill|agent|result): (.*)$", re.MULTILINE)
ATM_LINE = re.compile(r"^atm:\s+(\S+)", re.MULTILINE)
TRANSPORT_LINE = re.compile(r"^herdr transport: (\S+ \([^)]*\))", re.MULTILINE)
REF_LINE = re.compile(r"@ ([0-9a-f]{7,40})")


def case(name: str, status: str, detail: str, host: str = HOST) -> dict[str, Any]:
    return {"name": name, "status": status, "detail": detail, "origin": host, "destination": host, "attempt": 1}


def parse_report(text: str) -> list[dict[str, Any]]:
    """One case per skill step; the skill and the reporting agent prefix the step name."""
    fields = dict(FIELD.findall(text))
    label = f"{fields['skill']} / {fields['agent'].split()[0].split('@')[0]}"
    cases = []
    for line in text.splitlines():
        match = STEP.match(line)
        if match:
            name, _, detail = match.group(2).partition(" — ")
            cases.append(case(f"{label}: {name}", match.group(1), detail or name))
    return cases


def header_cases(result_text: str, report_cases: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The preflight rows the pane header shows: version plus herdr transport, and the fixture address."""
    version = ATM_LINE.search(result_text)
    transport = TRANSPORT_LINE.search(result_text)
    doctor_steps = [item for item in report_cases if item["name"].endswith(": Doctor passes")]
    status = "PASS" if doctor_steps and all(item["status"] == "PASS" for item in doctor_steps) else "FAIL"
    observed = transport.group(1) if transport else "not recorded"
    revision = REF_LINE.search(result_text)
    return [
        case("doctor", status, f"ATM {version.group(1) if version else 'unknown'}"),
        case("advertised host", "PASS", "127.0.0.1 (sentences enter by docker exec; --no-peer)"),
        case("herdr transport (atm doctor, fixture daemon)", "PASS" if observed.startswith("socket") else "FAIL", observed),
        case("testbed ref", "PASS" if revision else "FAIL", revision.group(1) if revision else "not recorded"),
    ]


def render(run_dir: Path, out_dir: Path) -> Path:
    result_text = (run_dir / "result.txt").read_text(encoding="utf-8")
    reports = sorted(run_dir.glob("report-*.txt"), key=lambda path: int(path.stem.split("-")[1]))
    report_cases = [item for path in reports for item in parse_report(path.read_text(encoding="utf-8"))]
    cases = header_cases(result_text, report_cases) + report_cases
    status = "PASS" if result_text.startswith("PASS") else "FAIL"
    run_id = out_dir.name.removesuffix(f"-{FEATURE}")
    out_dir.mkdir(parents=True, exist_ok=True)
    if out_dir.resolve() != run_dir.resolve():
        for path in [*reports, *(run_dir / name for name in RAW_FILES)]:
            shutil.copy2(path, out_dir / path.name)
    generated_at = datetime.now(timezone.utc).isoformat()
    report = out_dir / f"{FEATURE}.json"
    source_revision = _source_revision()
    procedure_page = _resolve_procedure_page(FEATURE, source_revision, root=REPO_ROOT, error_type=SmokeError)
    procedure_target = (
        REPO_ROOT / "site/reports" / procedure_page.html
        if procedure_page is not None
        else REPO_ROOT / "site/reports/procedures" / FEATURE / "index.html"
    )
    procedure_href = os.path.relpath(
        procedure_target,
        out_dir,
    )
    procedure_revision = procedure_page.revision if procedure_page is not None else None
    payload = {"feature": FEATURE, "host": HOST, "platform": PLATFORM, "run_id": run_id, "status": status, "source_revision": source_revision, "cases": cases}
    report.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    pane = out_dir / f"{HOST}-{FEATURE}.xhtml"
    compose(PANE_TEMPLATE, {
        "title": f"ATM smoke — {HOST}", "generated_at": generated_at, "host": HOST,
        "body_html": render_feature_pane(FEATURE, cases, HOST),
    }, pane)
    compose(REPO_ROOT / "templates/smoke-report/inbound-peer-frame.html.j2", {
        "title": f"ATM smoke — {FEATURE}", "generated_at": generated_at, "pane_src": pane.name,
        "procedure_label": f"{FEATURE} @ {procedure_revision[:8] if procedure_revision else 'unresolved'}", "procedure_href": procedure_href,
    }, report.with_suffix(".html"))
    compose(REPO_ROOT / "templates/smoke-report/inbound-peer-review.html.j2", {
        "title": "ATM colima integration smoke", "generated_at": generated_at,
        "pane_html": f'<section><h2>{escape(HOST)}</h2><iframe title="ATM smoke evidence for {escape(HOST, quote=True)}" '
                     f'src="{escape(pane.name, quote=True)}"></iframe></section>',
        "procedure_label": f"{FEATURE} @ {procedure_revision[:8] if procedure_revision else 'unresolved'}", "procedure_href": procedure_href,
    }, out_dir / "index.html")
    (out_dir / "smoke.envelope.json").write_text(json.dumps({
        "schema_version": 1, "report_type": "smoke", "generated_at": generated_at, "host_label": HOST,
        "report_html": (out_dir / "index.html").resolve().relative_to((REPO_ROOT / "site/reports").resolve()).as_posix(),
        "status": status,
        "source_revision": source_revision,
    }, indent=2) + "\n", encoding="utf-8")
    update_master_report_index()
    return report


def _source_revision() -> str | None:
    return _git_source_revision(REPO_ROOT)


def compose(template: Path, variables: dict[str, Any], output: Path) -> None:
    _compose(template, variables, output, root=REPO_ROOT, error_type=SmokeError)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("run_dir", type=Path, help="test.sh run directory or committed evidence directory")
    parser.add_argument("--out", type=Path, help=f"evidence directory (default: site/reports/smoke/{PLATFORM}/{HOST}/<run>-{FEATURE})")
    args = parser.parse_args()
    run_dir = args.run_dir.resolve()
    out_dir = args.out or REPO_ROOT / "site/reports/smoke" / PLATFORM / HOST / f"{run_dir.name.removesuffix(f'-{FEATURE}')}-{FEATURE}"
    print(render(run_dir, out_dir.resolve()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
