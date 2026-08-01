#!/usr/bin/env python3
"""Render AI.48 fuzz-session evidence through the copied sc-compose templates.

The coordinator/probe contract is intentionally kept separate from the report
contract.  This module adapts each bounded worker result into the fields
required by ``fuzz-run-agent.xhtml.j2`` and lets sc-compose render both the
worker panels and the top-level HTML shell.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
from html import escape
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.public_redaction import public_string
from scripts.public_redaction import public_value


REPORTS_ROOT = ROOT / "site" / "reports"
REPORT_TEMPLATE = ROOT / ".claude/skills/html-report/templates/fuzz-run-report.html.j2"
PANEL_TEMPLATE = ROOT / ".claude/skills/html-report/templates/fuzz-run-agent.xhtml.j2"
SCHEMA_VERSION = "adversarial-fuzzing/v1"
WORKERS = ("shape-probe", "template-probe", "boundary-probe", "differential-probe")
STATUSES = {"success", "failed", "timed_out"}
CLASSIFICATIONS = {"pass", "confirmed_bug", "intentional_boundary", "inconclusive"}
OUTCOME_KINDS = ("confirmed_bug", "non_repro", "benign", "inconclusive")
SAFE_STEM = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
SAFE_HOST = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")


class FuzzReportError(ValueError):
    """The campaign or one of its public artifacts is invalid."""


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def safe_stem(value: str) -> str:
    if not isinstance(value, str) or not SAFE_STEM.fullmatch(value):
        raise FuzzReportError(f"unsafe report stem: {value!r}")
    return value


def safe_host(value: Any) -> str:
    if not isinstance(value, str) or not SAFE_HOST.fullmatch(value):
        raise FuzzReportError("host_label must be a safe opaque label")
    return value


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise FuzzReportError(f"unable to read JSON artifact: {path}") from error


def _bounded_count(value: Any, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0 or value > 1000:
        raise FuzzReportError(f"{field} must be an integer between 0 and 1000")
    return value


def _default_input(worker_id: str, status: str, cases_run: int) -> dict[str, Any]:
    return {
        "case_id": f"{worker_id}-campaign",
        "description": f"AI.48 bounded {worker_id} campaign",
        "minimal_template": "---\n---\n{{ value }}",
        "minimal_input": f"status={status}; cases_run={cases_run}",
        "passed": status == "success",
        "outcome": "all bounded cases completed" if status == "success" else f"worker ended with {status}",
    }


def _default_finding(worker_id: str, status: str) -> dict[str, str]:
    return {
        "finding_id": f"{worker_id}-{status}",
        "minimal_template": "---\n---\n{{ value }}",
        "minimal_input": f"worker status={status}",
        "expected_oracle": "worker completes its bounded campaign",
        "observed_result": f"worker returned {status}",
        "requirement_trace": "No requirement or ADR currently covers this behavior.",
        "requirement_follow_up": "Record the owner and evidence before changing a product contract.",
        "root_cause": "Worker did not provide an evidence-backed root cause.",
        "recommended_fix": "Review the worker evidence and rerun the bounded campaign.",
    }


def normalize_worker(raw: Any, session_id: str, target: str) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise FuzzReportError("worker result must be an object")
    worker_id = raw.get("correlation_id")
    if worker_id not in WORKERS:
        raise FuzzReportError(f"unknown worker correlation_id: {worker_id!r}")
    status = raw.get("status")
    if status not in STATUSES:
        raise FuzzReportError(f"worker {worker_id}: invalid status {status!r}")
    cases_run = _bounded_count(raw.get("cases_run"), f"{worker_id}.cases_run")
    passed = _bounded_count(raw.get("passed", cases_run if status == "success" else 0), f"{worker_id}.passed")
    if passed > cases_run:
        raise FuzzReportError(f"worker {worker_id}: passed exceeds cases_run")
    failed = _bounded_count(raw.get("failed", cases_run - passed), f"{worker_id}.failed")
    if failed != cases_run - passed:
        raise FuzzReportError(f"worker {worker_id}: failed does not equal cases_run - passed")
    classification = raw.get("classification")
    if classification is None:
        classification = {"success": "pass", "failed": "confirmed_bug", "timed_out": "inconclusive"}[status]
    if classification not in CLASSIFICATIONS:
        raise FuzzReportError(f"worker {worker_id}: invalid classification {classification!r}")
    test_inputs = raw.get("test_inputs", [_default_input(worker_id, status, cases_run)])
    if not isinstance(test_inputs, list) or any(not isinstance(item, dict) for item in test_inputs):
        raise FuzzReportError(f"worker {worker_id}: test_inputs must be an array of objects")
    normalized_inputs: list[dict[str, Any]] = []
    for item in test_inputs:
        required = {"case_id", "description", "minimal_template", "minimal_input", "passed", "outcome"}
        if required - item.keys():
            raise FuzzReportError(f"worker {worker_id}: test input is missing required fields")
        if not all(isinstance(item[field], str) for field in required - {"passed"}):
            raise FuzzReportError(f"worker {worker_id}: test input text fields must be strings")
        if not isinstance(item["passed"], bool):
            raise FuzzReportError(f"worker {worker_id}: test input passed must be boolean")
        normalized_inputs.append({field: public_value(item[field]) for field in required})
    findings = raw.get("findings", [])
    if not isinstance(findings, list) or any(not isinstance(item, dict) for item in findings):
        raise FuzzReportError(f"worker {worker_id}: findings must be an array of objects")
    if classification != "pass" and not findings:
        findings = [_default_finding(worker_id, status)]
    findings = public_value(findings)
    payload = public_value(dict(raw))
    payload.update({"correlation_id": worker_id, "target": target, "status": status, "cases_run": cases_run})
    description = public_string(raw.get("fuzz_run_description", f"AI.48 {target} {worker_id} bounded campaign"))
    result = "PASS" if failed == 0 else "FAIL"
    return {
        "session_id": session_id,
        "agent_id": worker_id,
        "fuzz_run_description": description,
        "worker_correlation_id": worker_id,
        "classification": classification,
        "iterations": cases_run,
        "passed": passed,
        "failed": failed,
        "result": result,
        "summary": public_string(raw.get("summary", f"{worker_id} returned {status} after {cases_run} bounded cases.")),
        "test_inputs": normalized_inputs,
        "findings": findings,
        "json_payload": payload,
        "copy_json": json.dumps(payload, sort_keys=True),
        "context_text": public_string(raw.get("context_text", f"{worker_id}: {status}; {passed}/{cases_run} cases passed.")),
    }


def normalize_campaign(payload: Any, session_id: str | None = None) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise FuzzReportError("campaign result must be a JSON object")
    if payload.get("schema_version") != SCHEMA_VERSION:
        raise FuzzReportError(f"expected schema_version {SCHEMA_VERSION}")
    campaign = payload.get("campaign")
    if not isinstance(campaign, dict):
        raise FuzzReportError("campaign result must contain a campaign object")
    target = campaign.get("target")
    if not isinstance(target, str) or not target:
        raise FuzzReportError("campaign.target must be a non-empty string")
    workers_raw = payload.get("workers")
    if not isinstance(workers_raw, list):
        raise FuzzReportError("campaign result workers must be an array")
    sid = session_id or str(payload.get("session_id") or campaign.get("session_id") or "ai48-fuzz-session")
    workers = [normalize_worker(item, sid, target) for item in workers_raw]
    if len({worker["agent_id"] for worker in workers}) != len(workers):
        raise FuzzReportError("worker correlation_id values must be unique")
    expected = WORKERS if target == "full" else tuple(worker["agent_id"] for worker in workers)
    present = {worker["agent_id"] for worker in workers}
    missing = [worker for worker in expected if worker not in present]
    for worker_id in missing:
        workers.append(normalize_worker({"correlation_id": worker_id, "status": "timed_out", "cases_run": 0}, sid, target))
    public_campaign = public_value(campaign)
    ledger_raw = payload.get("outcome_ledger", {})
    if not isinstance(ledger_raw, dict):
        raise FuzzReportError("outcome_ledger must be an object when present")
    if set(ledger_raw) - set(OUTCOME_KINDS):
        raise FuzzReportError("outcome_ledger contains an unsupported outcome")
    outcome_ledger: dict[str, list[dict[str, str]]] = {}
    for outcome in OUTCOME_KINDS:
        entries = ledger_raw.get(outcome, [])
        if not isinstance(entries, list) or any(not isinstance(entry, dict) for entry in entries):
            raise FuzzReportError(f"outcome_ledger.{outcome} must be an array of objects")
        normalized_entries: list[dict[str, str]] = []
        for entry in entries:
            if set(entry) != {"candidate_id", "outcome", "detail"}:
                raise FuzzReportError(f"outcome_ledger.{outcome} entry has unsupported fields")
            if entry["outcome"] != outcome or not all(isinstance(entry[field], str) for field in entry):
                raise FuzzReportError(f"outcome_ledger.{outcome} entry is invalid")
            normalized_entries.append(public_value(entry))
        outcome_ledger[outcome] = normalized_entries
    return {
        "schema_version": SCHEMA_VERSION,
        "session_id": sid,
        "generated_at": payload.get("generated_at", utc_now()),
        "host_label": safe_host(payload.get("host_label", "local")),
        "campaign": public_campaign,
        "workers": workers,
        "execution_mode": payload.get("execution_mode", "contract-only"),
        "outcome_ledger": outcome_ledger,
    }


def compose(template: Path, variables: dict[str, Any], output: Path, root: Path = ROOT) -> None:
    variables_path: Path | None = None
    output.parent.mkdir(parents=True, exist_ok=True)
    try:
        with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
            json.dump(variables, handle, sort_keys=True)
            variables_path = Path(handle.name)
        result = subprocess.run(
            ["sc-compose", "render", "--root", str(root), "--file", str(template), "--var-file", str(variables_path), "--output", str(output)],
            cwd=root, capture_output=True, text=True, check=False,
        )
        if result.returncode != 0:
            raise FuzzReportError(f"sc-compose render failed: {result.stderr.strip() or result.stdout.strip()}")
    finally:
        if variables_path is not None:
            variables_path.unlink(missing_ok=True)


def _summary_intro(session: dict[str, Any]) -> str:
    outcome_rows = "".join(
        "<tr>"
        f"<th scope=\"row\">{escape(outcome.replace('_', ' '))}</th>"
        f"<td>{len(session['outcome_ledger'][outcome])}</td>"
        "</tr>"
        for outcome in OUTCOME_KINDS
    )
    return (
        "<p>AI.48 coordinator/probe evidence rendered through the copied sc-compose fuzz-report contract. "
        f"Session <code>{escape(session['session_id'])}</code> contains one bounded panel per worker.</p>"
        "<p>Candidate outcome ledger: zero confirmed bugs is explicit evidence, not an implicit pass.</p>"
        "<table><thead><tr><th scope=\"col\">Candidate outcome</th><th scope=\"col\">Count</th>"
        f"</tr></thead><tbody>{outcome_rows}</tbody></table>"
    )


def render_campaign(payload: Any, stem: str, reports_root: Path = REPORTS_ROOT, invoke_index: bool = True) -> dict[str, Any]:
    stem = safe_stem(stem)
    session = normalize_campaign(payload)
    report_dir = reports_root / stem
    report_html = reports_root / f"{stem}.html"
    sidecar = report_dir / f"{stem}.json"
    workers = session["workers"]
    sections: list[str] = []
    section_records: list[dict[str, Any]] = []
    for worker in workers:
        panel_path = report_dir / f"{stem}-{worker['agent_id']}.xhtml"
        compose(PANEL_TEMPLATE, {"agent": worker}, panel_path)
        sections.append(panel_path.read_text(encoding="utf-8"))
        section_records.append({
            "id": worker["agent_id"],
            "title": worker["agent_id"],
            "status": worker["result"],
            "body_html": sections[-1],
            "context_text": worker["context_text"],
            "json_payload": worker["json_payload"],
            "xhtml_path": f"{stem}/{panel_path.name}",
            "fragment_source": "auto-generated",
        })
    ledger = session["outcome_ledger"]
    status = (
        "ERROR"
        if ledger["confirmed_bug"]
        else "INFO"
        if ledger["non_repro"] or ledger["inconclusive"]
        else "PASS"
    )
    rows = [
        {"label": worker["fuzz_run_description"], "iterations": worker["iterations"], "pass": f"{worker['passed']}/{worker['iterations']}", "result": worker["result"]}
        for worker in workers
    ]
    report_data: dict[str, Any] = {
        "output_path": f"site/reports/{stem}.html",
        "json_output_path": f"site/reports/{stem}/{stem}.json",
        "title": f"Adversarial fuzz session: {session['session_id']}",
        "subtitle": "AI.48 coordinator/probe evidence",
        "status": status,
        "generated_at": session["generated_at"],
        "campaign": session["campaign"],
        "outcome_ledger": session["outcome_ledger"],
        "source_label": "AI.48 fuzz coordinator contract",
        "summary_intro_html": _summary_intro(session),
        "rows": rows,
        "sections": sections,
        "toc_rows": [{"id": worker["agent_id"], "title": worker["agent_id"], "status": worker["result"]} for worker in workers],
        "metadata_rows": [
            {"label": "Session", "value": session["session_id"]},
            {"label": "Execution mode", "value": session["execution_mode"]},
            {"label": "Worker count", "value": len(workers)},
        ],
        "summary_copy_json": json.dumps({"session_id": session["session_id"], "status": status, "workers": len(workers)}, sort_keys=True),
        "summary_copy_context": f"{session['session_id']}: {status}; {len(workers)} worker panels.",
        "footer_html": "<p>Generated from AI.48 coordinator evidence through sc-compose.</p>",
    }
    compose(REPORT_TEMPLATE, report_data, report_html)
    report_data["sections"] = section_records
    sidecar.parent.mkdir(parents=True, exist_ok=True)
    sidecar.write_text(json.dumps(report_data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    envelope = {
        "schema_version": 1,
        "report_type": "fuzz",
        "generated_at": session["generated_at"],
        "host_label": session["host_label"],
        "report_html": f"{stem}.html",
    }
    (report_dir / f"{stem}.envelope.json").write_text(json.dumps(envelope, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if invoke_index:
        result = subprocess.run(["just", "reports-index"], cwd=ROOT, capture_output=True, text=True, check=False)
        if result.returncode != 0:
            raise FuzzReportError(f"reports-index failed: {result.stderr.strip() or result.stdout.strip()}")
    return report_data


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Render an AI.48 fuzz campaign through sc-compose templates.")
    parser.add_argument("campaign", type=Path, help="AI.48 campaign result JSON")
    parser.add_argument("--stem", required=True, help="safe report stem, e.g. 20260801-1-fuzz-report")
    args = parser.parse_args(argv[1:])
    try:
        render_campaign(load_json(args.campaign), args.stem)
    except FuzzReportError as error:
        print(f"fuzz-report: error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
