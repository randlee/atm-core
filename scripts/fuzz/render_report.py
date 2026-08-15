#!/usr/bin/env python3
"""Render validated v2 adversarial-fuzz evidence through sc-compose templates.

The runner's tooling-only contract probe remains distinct from executed product
campaigns. This module validates a v2 report once, projects that exact evidence
into the stable panel contract, and lets sc-compose render the panels and shell.
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
JUST_ROOT = ROOT / ".just"
if str(JUST_ROOT) not in sys.path:
    sys.path.insert(0, str(JUST_ROOT))

from scripts.public_redaction import public_string
from scripts.public_redaction import public_value
from run_fuzz import FuzzInputError as V2FuzzInputError
from run_fuzz import validate_report


REPORTS_ROOT = ROOT / "site" / "reports"
REPORT_TEMPLATE = ROOT / ".claude/skills/html-report/templates/fuzz-run-report.html.j2"
PANEL_TEMPLATE = ROOT / ".claude/skills/html-report/templates/fuzz-run-agent.xhtml.j2"
SCHEMA_VERSION = "adversarial-fuzzing/v2"
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
    payload.update({
        "correlation_id": worker_id,
        "target": target,
        "status": status,
        "cases_run": cases_run,
        "findings": findings,
        "test_inputs": normalized_inputs,
    })
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


def _v2_worker_for_panel(worker: dict[str, Any], findings: list[dict[str, Any]], session_id: str) -> dict[str, Any]:
    """Project validated v2 evidence into the stable HTML-panel DTO."""
    worker_findings = [finding for finding in findings if finding["worker_correlation_id"] == worker["correlation_id"]]
    classification = (
        worker_findings[0]["classification"]
        if worker_findings else "pass" if worker["status"] == "success" else "inconclusive"
    )
    passed = worker["cases_run"] if worker["status"] == "success" else 0
    inputs = [
        {
            "case_id": case["case_id"],
            "description": case["expected_oracle"],
            "minimal_template": "negative contract case",
            "minimal_input": case["observed_result"],
            "passed": all(case["diagnostic_contract"]["field_matches"].values()),
            "outcome": case["observed_result"],
        }
        for case in worker["negative_cases"]
    ]
    payload = public_value(worker)
    panel_findings = [
        {
            "finding_id": finding["finding_id"],
            "minimal_template": finding["minimal_template"],
            "minimal_input": finding["minimal_input"],
            "expected_oracle": finding["expected_oracle"],
            "observed_result": finding["observed_result"],
            "requirement_trace": "See campaign evidence and governing template contract.",
            "requirement_follow_up": "Tracked through the durable campaign finding.",
            "root_cause": "Pending separate investigation unless classification is intentional_boundary.",
            "recommended_fix": "Follow the finding's owning-crate regression and triage record.",
        }
        for finding in worker_findings
    ]
    negative_cases = []
    for case in worker["negative_cases"]:
        projected = public_value(case)
        projected["diagnostic_match"] = (
            "PASS" if all(case["diagnostic_contract"]["field_matches"].values()) else "FAIL"
        )
        negative_cases.append(projected)
    return {
        "session_id": session_id,
        "agent_id": worker["correlation_id"],
        "fuzz_run_description": f"{worker['target']} {worker['correlation_id']} product seam",
        "worker_correlation_id": worker["correlation_id"],
        "classification": classification,
        "iterations": worker["cases_run"],
        "passed": passed,
        "failed": worker["cases_run"] - passed,
        "result": "PASS" if worker["status"] == "success" else "FAIL",
        "summary": f"{worker['status']}; v2 target invocation and diagnostic evidence retained.",
        "target_invocation": worker["target_invocation"],
        "negative_cases": negative_cases,
        "encountered_issues": worker["encountered_issues"],
        "test_inputs": public_value(inputs),
        "findings": public_value(panel_findings),
        "json_payload": payload,
        "copy_json": json.dumps(payload, sort_keys=True),
        "context_text": f"{worker['correlation_id']}: {worker['status']}; {passed}/{worker['cases_run']} cases passed.",
    }


def normalize_campaign(payload: Any, session_id: str | None = None) -> dict[str, Any]:
    """Validate v2 once, then render exactly that evidence without synthesis."""
    if not isinstance(payload, dict) or payload.get("schema_version") != SCHEMA_VERSION:
        raise FuzzReportError(f"expected schema_version {SCHEMA_VERSION}")
    try:
        report = validate_report(payload, ROOT)
    except V2FuzzInputError as error:
        raise FuzzReportError(f"invalid v2 fuzz report: {error}") from error
    campaign = report["campaign"]
    sid = session_id or campaign["campaign_id"]
    workers = [_v2_worker_for_panel(worker, report["findings"], sid) for worker in report["workers"]]
    ledger = {outcome: [] for outcome in OUTCOME_KINDS}
    for finding in report["findings"]:
        outcome = "confirmed_bug" if finding["classification"] == "confirmed_bug" else "inconclusive" if finding["classification"] == "inconclusive" else "benign"
        ledger[outcome].append({"candidate_id": finding["finding_id"], "outcome": outcome, "detail": finding["observed_result"]})
    return {
        "schema_version": SCHEMA_VERSION,
        "session_id": sid,
        "generated_at": utc_now(),
        "host_label": "local",
        "campaign": public_value(campaign),
        "workers": workers,
        "execution_mode": report["execution_mode"],
        "outcome_ledger": ledger,
        "campaign_issues": public_value(report["campaign_issues"]),
        "summary": public_value(report["summary"]),
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
    execution_mode = session["execution_mode"]
    mode_summary = (
        "This is tooling-only contract evidence; it does not claim product-seam coverage."
        if execution_mode == "contract-probe"
        else "This is executed product-seam evidence; each retained command log proves the named seam's bounded corpus."
    )
    return (
        "<p>Validated v2 adversarial-fuzz evidence rendered through the sc-compose report contract. "
        f"Session <code>{escape(session['session_id'])}</code> contains one bounded panel per worker. {mode_summary}</p>"
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
        # sc-compose accepts arrays of objects only at the var-file top level.
        # Keep the worker envelope scalar at the template boundary and pass
        # the repeated panel rows as explicit top-level variables.
        panel_agent = {
            key: value
            for key, value in worker.items()
            if key not in {"test_inputs", "findings", "json_payload"}
        }
        compose(
            PANEL_TEMPLATE,
            {
                "agent": panel_agent,
                "test_inputs": worker["test_inputs"],
                "findings": worker["findings"],
            },
            panel_path,
        )
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
        "schema_version": SCHEMA_VERSION,
        "output_path": f"site/reports/{stem}.html",
        "json_output_path": f"site/reports/{stem}/{stem}.json",
        "title": f"Adversarial fuzz session: {session['session_id']}",
        "subtitle": "Validated adversarial-fuzz v2 evidence",
        "status": status,
        "generated_at": session["generated_at"],
        "campaign": session["campaign"],
        "source_label": "adversarial-fuzz v2 contract",
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
        "footer_html": "<p>Generated from validated adversarial-fuzz v2 evidence through sc-compose.</p>",
    }
    compose(REPORT_TEMPLATE, report_data, report_html)
    # sc-compose accepts arrays of objects only at top-level var-file paths.
    # Keep the nested ledger in the durable JSON sidecar after shell rendering.
    report_data["outcome_ledger"] = session["outcome_ledger"]
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
    parser = argparse.ArgumentParser(description="Render a validated adversarial-fuzz v2 campaign through sc-compose templates.")
    parser.add_argument("campaign", type=Path, help="adversarial-fuzz v2 campaign result JSON")
    parser.add_argument("--stem", required=True, help="safe report stem, e.g. 20260801-1-fuzz-report")
    parser.add_argument("--no-index", action="store_true", help="do not regenerate the repository-wide reports index")
    args = parser.parse_args(argv[1:])
    try:
        render_campaign(load_json(args.campaign), args.stem, invoke_index=not args.no_index)
    except FuzzReportError as error:
        print(f"fuzz-report: error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
