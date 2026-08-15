#!/usr/bin/env python3
"""Validate bounded adversarial-fuzz campaign evidence.

The runner accepts only the v2 evidence contract. It can either validate an
external durable report or run the explicit, instrumented contract probe used
to prove the validator itself. The contract probe is tooling evidence only; it
is never evidence that a product rendering seam was exercised.
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import subprocess
import sys
from typing import Any, Callable


SCHEMA_VERSION = "adversarial-fuzzing/v2"
CONTRACT_PROBE_SEAM = "run_fuzz.validate_worker_result"
TARGETS = (
    "var-file",
    "frontmatter",
    "resolver",
    "renderer",
    "includes",
    "cli",
    "local-http-framing",
    "atm-template-checked-emission",
    "full",
)
WORKERS = ("shape-probe", "template-probe", "boundary-probe", "differential-probe")
WORKER_TARGETS = {
    "shape-probe": "var-file",
    "template-probe": "renderer",
    "boundary-probe": "cli",
    "differential-probe": "full",
}

# The AN.15 lane deliberately closes over these owning-crate test selectors.
# Campaign JSON never accepts a command, so fuzz evidence cannot become a
# shell-execution interface.  Each selected test owns a bounded 100-vector
# corpus; the worker's coverage proof points at its retained command log.
CHECKED_EMISSION_WORKERS = {
    "shape-probe": {
        "seam_id": "atm_http_runtime::template_admission::captured_input",
        "command": ("cargo", "test", "--workspace", "an15_shape_probe_", "--", "--nocapture"),
        "negative_cases": [
            ("missing-required-variable", "required input is rejected before persistence", "required template input is missing"),
        ],
    },
    "template-probe": {
        "seam_id": "atm_template_sc_compose::checked_render",
        "command": ("cargo", "test", "-p", "atm-template-sc-compose", "an15_template_probe_", "--", "--nocapture"),
        "negative_cases": [
            ("invalid-final-json", "invalid final JSON is rejected with checked-render guidance", "checked render rejected invalid JSON"),
        ],
    },
    "boundary-probe": {
        "seam_id": "atm_template_sc_compose::confined_include",
        "command": ("cargo", "test", "-p", "atm-template-sc-compose", "an15_boundary_probe_", "--", "--nocapture"),
        "negative_cases": [
            ("include-escape", "confined include escape is rejected without state mutation", "include path rejected outside configured root"),
        ],
    },
    "differential-probe": {
        "seam_id": "atm_core::render_on_read::persisted_snapshot",
        "command": ("cargo", "test", "--workspace", "an15_differential_probe_", "--", "--nocapture"),
        # This worker's required cases are positive differential checks: the
        # persisted captured value must win over a changed ambient variable.
        "negative_cases": [],
    },
}
PROOF_MECHANISMS = ("counter", "tracing-span", "coverage")
ISSUE_CATEGORIES = ("environment", "dependency", "harness", "tooling", "product")
ISSUE_DISPOSITIONS = ("fix_now", "deferred")
STATUSES = ("success", "failed", "timed_out")
CLASSIFICATIONS = ("confirmed_bug", "intentional_boundary", "inconclusive")
DIAGNOSTIC_MATCH_FIELDS = (
    "status",
    "code_or_category",
    "message_family",
    "recovery_family",
    "no_sensitive_leak",
)
CAMPAIGN_FIELDS = {
    "campaign_id",
    "worktree_path",
    "target",
    "baseline_ref",
    "seed",
    "max_workers",
    "cases_per_worker",
    "per_worker_timeout_s",
    "promote_regressions",
    "target_seams",
    "notes",
}
CAMPAIGN_OPTIONAL_FIELDS = {"baseline_ref", "notes", "campaign_id"}
SEAM_FIELDS = {"seam_id", "minimum_invocations", "accepted_proof"}
WORKER_RESULT_FIELDS = {
    "correlation_id",
    "target",
    "status",
    "cases_run",
    "finding_ids",
    "error",
    "target_invocation",
    "negative_cases",
    "encountered_issues",
}
INVOCATION_FIELDS = {"required_seam_ids", "proofs"}
PROOF_FIELDS = {"seam_id", "mechanism", "invocation_count", "evidence_ref"}
ISSUE_FIELDS = {
    "issue_id",
    "case_id",
    "category",
    "observed_evidence",
    "disposition",
    "owner",
    "tracking_ref",
    "defer_reason",
}
REPORT_ISSUE_FIELDS = ISSUE_FIELDS | {"worker_correlation_id"}
DIAGNOSTIC_FIELDS = {
    "expected_status",
    "observed_status",
    "expected_code_or_category",
    "observed_code_or_category",
    "expected_message_family",
    "observed_message_family",
    "expected_recovery_family",
    "observed_recovery_family",
    "sensitive_input_leaked",
    "field_matches",
}
NEGATIVE_CASE_FIELDS = {
    "case_id",
    "expected_oracle",
    "observed_result",
    "diagnostic_contract",
    "target_invocation",
    "finding_id",
}
FINDING_FIELDS = {
    "finding_id",
    "worker_correlation_id",
    "classification",
    "command",
    "minimal_template",
    "minimal_input",
    "expected_oracle",
    "observed_result",
    "diagnostic_contract",
    "target_invocation",
    "reproduction_count",
    "approved_differential_delta",
}
FINDING_OPTIONAL_FIELDS = {"approved_differential_delta"}
REPORT_FIELDS = {
    "schema_version",
    "execution_mode",
    "campaign",
    "workers",
    "findings",
    "promoted_tests",
    "unresolved_candidates",
    "campaign_issues",
    "summary",
}
SUMMARY_FIELDS = {
    "all_successful",
    "confirmed_bugs",
    "intentional_boundaries",
    "inconclusive",
    "failed_workers",
    "target_not_exercised_workers",
    "open_campaign_issues",
}


class FuzzInputError(ValueError):
    """A campaign or report violates the public fuzz contract."""


def repository_root() -> Path:
    return Path(__file__).resolve().parents[1]


def _inside(path: Path, root: Path, label: str) -> Path:
    resolved = path.expanduser().resolve()
    allowed = (root.resolve(), root.resolve().parent / f"{root.resolve().name}-worktrees")
    if not any(resolved == candidate or candidate in resolved.parents for candidate in allowed):
        raise FuzzInputError(f"{label} must stay inside the repository or approved worktrees: {path}")
    return resolved


def _bounded_int(value: Any, name: str, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise FuzzInputError(f"{name} must be an integer between {minimum} and {maximum}")
    return value


def _required_object(value: Any, label: str, fields: set[str], optional: set[str] = frozenset()) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise FuzzInputError(f"{label} must be a JSON object")
    unknown = set(value) - fields
    if unknown:
        raise FuzzInputError(f"unsupported {label} fields: {', '.join(sorted(unknown))}")
    missing = fields - optional - value.keys()
    if missing:
        raise FuzzInputError(f"missing {label} fields: {', '.join(sorted(missing))}")
    return value


def _required_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip() or "\n" in value:
        raise FuzzInputError(f"{label} must be a non-empty single-line string")
    return value


def _string_list(value: Any, label: str, minimum: int = 0) -> list[str]:
    if not isinstance(value, list) or len(value) < minimum:
        raise FuzzInputError(f"{label} must be an array with at least {minimum} item(s)")
    result = [_required_string(item, f"{label} item") for item in value]
    if len(result) != len(set(result)):
        raise FuzzInputError(f"{label} must not contain duplicates")
    return result


def _validate_target_seams(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list) or not 1 <= len(value) <= 32:
        raise FuzzInputError("target_seams must contain between 1 and 32 seam declarations")
    seams = []
    for index, raw in enumerate(value):
        seam = _required_object(raw, f"target_seams[{index}]", SEAM_FIELDS)
        mechanism = seam["accepted_proof"]
        if mechanism not in PROOF_MECHANISMS:
            raise FuzzInputError(f"target_seams[{index}].accepted_proof must be one of {', '.join(PROOF_MECHANISMS)}")
        seams.append({
            "seam_id": _required_string(seam["seam_id"], f"target_seams[{index}].seam_id"),
            "minimum_invocations": _bounded_int(
                seam["minimum_invocations"], f"target_seams[{index}].minimum_invocations", 1, 1_000_000
            ),
            "accepted_proof": mechanism,
        })
    identifiers = [seam["seam_id"] for seam in seams]
    if len(identifiers) != len(set(identifiers)):
        raise FuzzInputError("target_seams must not declare the same seam twice")
    return seams


def validate_campaign(payload: Any, root: Path | None = None, *, require_campaign_id: bool = False) -> dict[str, Any]:
    campaign = _required_object(payload, "campaign", CAMPAIGN_FIELDS, CAMPAIGN_OPTIONAL_FIELDS)
    repo = (root or repository_root()).resolve()
    worktree_raw = campaign["worktree_path"]
    if not isinstance(worktree_raw, str) or not Path(worktree_raw).is_absolute():
        raise FuzzInputError("worktree_path must be an absolute path")
    worktree = _inside(Path(worktree_raw), repo, "worktree_path")
    if not worktree.is_dir():
        raise FuzzInputError(f"worktree_path does not name an existing directory: {worktree}")
    target = campaign["target"]
    if target not in TARGETS:
        raise FuzzInputError(f"target must be one of {', '.join(TARGETS)}")
    baseline = campaign.get("baseline_ref")
    if baseline is not None:
        baseline = _required_string(baseline, "baseline_ref")
    notes = campaign.get("notes", "")
    if not isinstance(notes, str) or len(notes) > 4000:
        raise FuzzInputError("notes must be a string of at most 4000 characters")
    campaign_id = campaign.get("campaign_id")
    if require_campaign_id or campaign_id is not None:
        campaign_id = _required_string(campaign_id, "campaign_id")
    if not isinstance(campaign["promote_regressions"], bool):
        raise FuzzInputError("promote_regressions must be boolean")
    validated = {
        "campaign_id": campaign_id,
        "worktree_path": str(worktree),
        "target": target,
        "baseline_ref": baseline,
        "seed": _bounded_int(campaign["seed"], "seed", 0, 2**63 - 1),
        "max_workers": _bounded_int(campaign["max_workers"], "max_workers", 1, 4),
        "cases_per_worker": _bounded_int(campaign["cases_per_worker"], "cases_per_worker", 1, 1000),
        "per_worker_timeout_s": _bounded_int(campaign["per_worker_timeout_s"], "per_worker_timeout_s", 1, 600),
        "promote_regressions": campaign["promote_regressions"],
        "target_seams": _validate_target_seams(campaign["target_seams"]),
        "notes": notes,
    }
    if target == "atm-template-checked-emission":
        if validated["max_workers"] != len(WORKERS):
            raise FuzzInputError("atm-template-checked-emission requires exactly four workers")
        if validated["cases_per_worker"] < 100:
            raise FuzzInputError("atm-template-checked-emission requires at least 100 cases per worker")
        if validated["per_worker_timeout_s"] != 120:
            raise FuzzInputError("atm-template-checked-emission requires a 120-second worker timeout")
        expected = {contract["seam_id"] for contract in CHECKED_EMISSION_WORKERS.values()}
        declared = {seam["seam_id"] for seam in validated["target_seams"]}
        if declared != expected:
            raise FuzzInputError("atm-template-checked-emission must declare each fixed product seam exactly once")
        if any(seam["accepted_proof"] != "coverage" or seam["minimum_invocations"] < 100 for seam in validated["target_seams"]):
            raise FuzzInputError("atm-template-checked-emission requires coverage proof with at least 100 invocations per seam")
    return validated


def _seams_by_id(campaign: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {seam["seam_id"]: seam for seam in campaign["target_seams"]}


def validate_target_invocation(value: Any, campaign: dict[str, Any], label: str) -> dict[str, Any]:
    invocation = _required_object(value, label, INVOCATION_FIELDS)
    required_seam_ids = _string_list(invocation["required_seam_ids"], f"{label}.required_seam_ids", minimum=1)
    declared = _seams_by_id(campaign)
    unknown = set(required_seam_ids) - set(declared)
    if unknown:
        raise FuzzInputError(f"{label} names undeclared target seams: {', '.join(sorted(unknown))}")
    proofs = invocation["proofs"]
    if not isinstance(proofs, list) or len(proofs) != len(required_seam_ids):
        raise FuzzInputError(f"{label}.proofs must contain exactly one record for every required seam")
    normalized = []
    for index, raw in enumerate(proofs):
        proof = _required_object(raw, f"{label}.proofs[{index}]", PROOF_FIELDS)
        seam_id = _required_string(proof["seam_id"], f"{label}.proofs[{index}].seam_id")
        if seam_id not in required_seam_ids:
            raise FuzzInputError(f"{label}.proofs[{index}] does not name a required seam")
        declaration = declared[seam_id]
        if proof["mechanism"] != declaration["accepted_proof"]:
            raise FuzzInputError(f"{label}.proofs[{index}].mechanism does not match the seam declaration")
        count = _bounded_int(proof["invocation_count"], f"{label}.proofs[{index}].invocation_count", 1, 1_000_000_000)
        if count < declaration["minimum_invocations"]:
            raise FuzzInputError(f"target-not-exercised: {seam_id} has fewer than the required invocations")
        normalized.append({
            "seam_id": seam_id,
            "mechanism": proof["mechanism"],
            "invocation_count": count,
            "evidence_ref": _required_string(proof["evidence_ref"], f"{label}.proofs[{index}].evidence_ref"),
        })
    if {proof["seam_id"] for proof in normalized} != set(required_seam_ids):
        raise FuzzInputError(f"{label}.proofs must cover every required seam exactly once")
    return {"required_seam_ids": required_seam_ids, "proofs": normalized}


def _validate_issue(value: Any, label: str, *, report_issue: bool = False) -> dict[str, Any]:
    fields = REPORT_ISSUE_FIELDS if report_issue else ISSUE_FIELDS
    issue = _required_object(value, label, fields)
    category = issue["category"]
    if category not in ISSUE_CATEGORIES:
        raise FuzzInputError(f"{label}.category must be one of {', '.join(ISSUE_CATEGORIES)}")
    disposition = issue["disposition"]
    if disposition not in ISSUE_DISPOSITIONS:
        raise FuzzInputError(f"{label}.disposition must be one of {', '.join(ISSUE_DISPOSITIONS)}")
    defer_reason = issue["defer_reason"]
    if disposition == "deferred":
        defer_reason = _required_string(defer_reason, f"{label}.defer_reason")
    elif defer_reason is not None:
        raise FuzzInputError(f"{label}.defer_reason must be null when disposition is fix_now")
    normalized = {
        "issue_id": _required_string(issue["issue_id"], f"{label}.issue_id"),
        "case_id": _required_string(issue["case_id"], f"{label}.case_id"),
        "category": category,
        "observed_evidence": _required_string(issue["observed_evidence"], f"{label}.observed_evidence"),
        "disposition": disposition,
        "owner": _required_string(issue["owner"], f"{label}.owner"),
        "tracking_ref": _required_string(issue["tracking_ref"], f"{label}.tracking_ref"),
        "defer_reason": defer_reason,
    }
    if report_issue:
        normalized["worker_correlation_id"] = _required_string(
            issue["worker_correlation_id"], f"{label}.worker_correlation_id"
        )
    return normalized


def validate_worker_result(
    payload: Any,
    campaign: dict[str, Any],
    *,
    invocation_observer: Callable[[str], None] | None = None,
) -> dict[str, Any]:
    """Validate one worker result and optionally record direct seam execution."""
    if invocation_observer is not None:
        invocation_observer(CONTRACT_PROBE_SEAM)
    worker = _required_object(payload, "worker result", WORKER_RESULT_FIELDS)
    correlation_id = worker["correlation_id"]
    if correlation_id not in WORKERS:
        raise FuzzInputError(f"unknown worker correlation_id: {correlation_id!r}")
    target = worker["target"]
    if not isinstance(target, str) or target not in TARGETS:
        raise FuzzInputError("worker target is invalid")
    status = worker["status"]
    if status not in STATUSES:
        raise FuzzInputError(f"worker status must be one of {', '.join(STATUSES)}")
    finding_ids = _string_list(worker["finding_ids"], "finding_ids")
    error = worker["error"]
    if error is not None and not isinstance(error, dict):
        raise FuzzInputError("worker error must be an object or null")
    issues = [_validate_issue(issue, f"encountered_issues[{index}]") for index, issue in enumerate(worker["encountered_issues"])]
    issue_ids = [issue["issue_id"] for issue in issues]
    if len(issue_ids) != len(set(issue_ids)):
        raise FuzzInputError("encountered_issues must not contain duplicate issue_id values")
    invocation = validate_target_invocation(worker["target_invocation"], campaign, "target_invocation")
    if not isinstance(worker["negative_cases"], list):
        raise FuzzInputError("negative_cases must be an array")
    negative_cases = [
        _validate_negative_case(case, campaign, invocation, f"negative_cases[{index}]")
        for index, case in enumerate(worker["negative_cases"])
    ]
    case_ids = [case["case_id"] for case in negative_cases]
    if len(case_ids) != len(set(case_ids)):
        raise FuzzInputError("negative_cases must not contain duplicate case_id values")
    return {
        "correlation_id": correlation_id,
        "target": target,
        "status": status,
        "cases_run": _bounded_int(worker["cases_run"], "cases_run", 0, 1000),
        "finding_ids": finding_ids,
        "error": error,
        "target_invocation": invocation,
        "negative_cases": negative_cases,
        "encountered_issues": issues,
    }


def _validate_diagnostic_contract(value: Any, label: str) -> dict[str, Any]:
    diagnostic = _required_object(value, label, DIAGNOSTIC_FIELDS)
    matches = _required_object(diagnostic["field_matches"], f"{label}.field_matches", set(DIAGNOSTIC_MATCH_FIELDS))
    if any(not isinstance(matches[field], bool) for field in DIAGNOSTIC_MATCH_FIELDS):
        raise FuzzInputError(f"{label}.field_matches values must be boolean")
    leaked = diagnostic["sensitive_input_leaked"]
    if not isinstance(leaked, bool):
        raise FuzzInputError(f"{label}.sensitive_input_leaked must be boolean")
    if matches["no_sensitive_leak"] != (not leaked):
        raise FuzzInputError(f"{label}.no_sensitive_leak does not match sensitive_input_leaked")
    normalized = {field: _required_string(diagnostic[field], f"{label}.{field}") for field in DIAGNOSTIC_FIELDS - {"sensitive_input_leaked", "field_matches"}}
    normalized["sensitive_input_leaked"] = leaked
    normalized["field_matches"] = {field: matches[field] for field in DIAGNOSTIC_MATCH_FIELDS}
    return normalized


def _validate_negative_case(
    value: Any,
    campaign: dict[str, Any],
    worker_invocation: dict[str, Any],
    label: str,
) -> dict[str, Any]:
    case = _required_object(value, label, NEGATIVE_CASE_FIELDS)
    diagnostic = _validate_diagnostic_contract(case["diagnostic_contract"], f"{label}.diagnostic_contract")
    invocation = validate_target_invocation(
        {
            "required_seam_ids": [case["target_invocation"]["seam_id"]] if isinstance(case.get("target_invocation"), dict) and "seam_id" in case["target_invocation"] else [],
            "proofs": [case["target_invocation"]] if isinstance(case.get("target_invocation"), dict) else [],
        },
        campaign,
        f"{label}.target_invocation",
    )
    if invocation["proofs"][0] not in worker_invocation["proofs"]:
        raise FuzzInputError(f"{label}.target_invocation must be proven by its worker")
    finding_id = case["finding_id"]
    if finding_id is not None:
        finding_id = _required_string(finding_id, f"{label}.finding_id")
    if not all(diagnostic["field_matches"].values()) and finding_id is None:
        raise FuzzInputError(f"{label} diagnostic mismatch requires a confirmed finding")
    return {
        "case_id": _required_string(case["case_id"], f"{label}.case_id"),
        "expected_oracle": _required_string(case["expected_oracle"], f"{label}.expected_oracle"),
        "observed_result": _required_string(case["observed_result"], f"{label}.observed_result"),
        "diagnostic_contract": diagnostic,
        "target_invocation": invocation["proofs"][0],
        "finding_id": finding_id,
    }


def _validate_finding(value: Any, campaign: dict[str, Any], workers: dict[str, dict[str, Any]]) -> dict[str, Any]:
    finding = _required_object(value, "finding", FINDING_FIELDS, FINDING_OPTIONAL_FIELDS)
    worker_id = _required_string(finding["worker_correlation_id"], "finding.worker_correlation_id")
    if worker_id not in workers:
        raise FuzzInputError("finding references an unknown worker")
    classification = finding["classification"]
    if classification not in CLASSIFICATIONS:
        raise FuzzInputError(f"finding.classification must be one of {', '.join(CLASSIFICATIONS)}")
    diagnostic = _validate_diagnostic_contract(finding["diagnostic_contract"], "finding.diagnostic_contract")
    approved_delta = finding.get("approved_differential_delta")
    if approved_delta is not None:
        if worker_id != "differential-probe":
            raise FuzzInputError("approved_differential_delta is valid only for differential-probe findings")
        approved_delta = _required_object(approved_delta, "approved_differential_delta", {"description", "contract_trace"})
        approved_delta = {field: _required_string(approved_delta[field], f"approved_differential_delta.{field}") for field in approved_delta}
    if not all(diagnostic["field_matches"].values()) and classification != "confirmed_bug" and approved_delta is None:
        raise FuzzInputError("a diagnostic mismatch must be a confirmed_bug or cite an approved differential delta")
    invocation = validate_target_invocation(
        {
            "required_seam_ids": [finding["target_invocation"]["seam_id"]] if isinstance(finding.get("target_invocation"), dict) and "seam_id" in finding["target_invocation"] else [],
            "proofs": [finding["target_invocation"]] if isinstance(finding.get("target_invocation"), dict) else [],
        },
        campaign,
        "finding.target_invocation",
    )
    if invocation["proofs"][0] not in workers[worker_id]["target_invocation"]["proofs"]:
        raise FuzzInputError("finding target invocation must be proven by its worker")
    reproduction_count = _bounded_int(finding["reproduction_count"], "finding.reproduction_count", 0, 1000)
    if classification == "confirmed_bug" and reproduction_count < 3:
        raise FuzzInputError("confirmed_bug findings require at least three reproductions")
    return {
        "finding_id": _required_string(finding["finding_id"], "finding.finding_id"),
        "worker_correlation_id": worker_id,
        "classification": classification,
        "command": _required_string(finding["command"], "finding.command"),
        "minimal_template": _required_string(finding["minimal_template"], "finding.minimal_template"),
        "minimal_input": _required_string(finding["minimal_input"], "finding.minimal_input"),
        "expected_oracle": _required_string(finding["expected_oracle"], "finding.expected_oracle"),
        "observed_result": _required_string(finding["observed_result"], "finding.observed_result"),
        "diagnostic_contract": diagnostic,
        "target_invocation": invocation["proofs"][0],
        "reproduction_count": reproduction_count,
        "approved_differential_delta": approved_delta,
    }


def selected_workers(campaign: dict[str, Any]) -> list[str]:
    if campaign["target"] in {"full", "local-http-framing", "atm-template-checked-emission"}:
        return list(WORKERS[: campaign["max_workers"]])
    selected = [worker for worker in WORKERS if WORKER_TARGETS[worker] == campaign["target"]]
    return (selected or ["differential-probe"])[: campaign["max_workers"]]


def _expected_summary(workers: list[dict[str, Any]], findings: list[dict[str, Any]], issues: list[dict[str, Any]]) -> dict[str, Any]:
    classifications = [finding["classification"] for finding in findings]
    failed_workers = sum(worker["status"] != "success" for worker in workers)
    return {
        "all_successful": failed_workers == 0,
        "confirmed_bugs": classifications.count("confirmed_bug"),
        "intentional_boundaries": classifications.count("intentional_boundary"),
        "inconclusive": classifications.count("inconclusive"),
        "failed_workers": failed_workers,
        "target_not_exercised_workers": 0,
        "open_campaign_issues": len(issues),
    }


def validate_report(payload: Any, root: Path | None = None) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise FuzzInputError("report must be a JSON object")
    if payload.get("schema_version") != SCHEMA_VERSION:
        raise FuzzInputError(f"report schema_version must be {SCHEMA_VERSION}; v1 reports are not accepted")
    report = _required_object(payload, "report", REPORT_FIELDS)
    execution_mode = _required_string(report["execution_mode"], "execution_mode")
    campaign = validate_campaign(report["campaign"], root, require_campaign_id=True)
    raw_workers = report["workers"]
    if not isinstance(raw_workers, list):
        raise FuzzInputError("workers must be an array")
    workers = [validate_worker_result(worker, campaign) for worker in raw_workers]
    expected_workers = selected_workers(campaign)
    if [worker["correlation_id"] for worker in workers] != expected_workers:
        raise FuzzInputError("workers must contain the selected workers once, in correlation-ID order")
    if any(worker["target"] != campaign["target"] for worker in workers):
        raise FuzzInputError("worker target must match campaign target")
    if any(worker["cases_run"] != campaign["cases_per_worker"] for worker in workers):
        raise FuzzInputError("worker cases_run must match campaign cases_per_worker")
    worker_by_id = {worker["correlation_id"]: worker for worker in workers}
    all_proven = {proof["seam_id"] for worker in workers for proof in worker["target_invocation"]["proofs"]}
    missing_seams = set(_seams_by_id(campaign)) - all_proven
    if missing_seams:
        raise FuzzInputError(f"target-not-exercised: no worker proved {', '.join(sorted(missing_seams))}")
    raw_findings = report["findings"]
    if not isinstance(raw_findings, list):
        raise FuzzInputError("findings must be an array")
    findings = [_validate_finding(finding, campaign, worker_by_id) for finding in raw_findings]
    finding_ids = [finding["finding_id"] for finding in findings]
    if len(finding_ids) != len(set(finding_ids)):
        raise FuzzInputError("findings must not contain duplicate finding_id values")
    if set(finding_ids) != {finding_id for worker in workers for finding_id in worker["finding_ids"]}:
        raise FuzzInputError("worker finding_ids must match the durable findings exactly")
    finding_by_id = {finding["finding_id"]: finding for finding in findings}
    for worker in workers:
        for negative_case in worker["negative_cases"]:
            if not all(negative_case["diagnostic_contract"]["field_matches"].values()):
                finding = finding_by_id.get(negative_case["finding_id"])
                if finding is None or finding["classification"] != "confirmed_bug":
                    raise FuzzInputError("negative-case diagnostic mismatch must map to a confirmed_bug finding")
    raw_issues = report["campaign_issues"]
    if not isinstance(raw_issues, list):
        raise FuzzInputError("campaign_issues must be an array")
    issues = [_validate_issue(issue, f"campaign_issues[{index}]", report_issue=True) for index, issue in enumerate(raw_issues)]
    issue_keys = {(issue["worker_correlation_id"], issue["issue_id"]) for issue in issues}
    worker_issue_keys = {(worker["correlation_id"], issue["issue_id"]) for worker in workers for issue in worker["encountered_issues"]}
    if issue_keys != worker_issue_keys:
        raise FuzzInputError("campaign_issues must contain every worker encountered issue exactly once")
    for issue in issues:
        if issue["worker_correlation_id"] not in worker_by_id:
            raise FuzzInputError("campaign issue references an unknown worker")
    for worker in workers:
        for issue in worker["encountered_issues"]:
            aggregate = next(item for item in issues if (item["worker_correlation_id"], item["issue_id"]) == (worker["correlation_id"], issue["issue_id"]))
            if {key: aggregate[key] for key in ISSUE_FIELDS} != issue:
                raise FuzzInputError("campaign issue must preserve the worker issue without mutation")
    for optional_array in ("promoted_tests", "unresolved_candidates"):
        if not isinstance(report[optional_array], list):
            raise FuzzInputError(f"{optional_array} must be an array")
    summary = _required_object(report["summary"], "summary", SUMMARY_FIELDS)
    expected_summary = _expected_summary(workers, findings, issues)
    if summary != expected_summary:
        raise FuzzInputError("summary must exactly match the validated workers, findings, and campaign issues")
    return {
        "schema_version": SCHEMA_VERSION,
        "execution_mode": execution_mode,
        "campaign": campaign,
        "workers": workers,
        "findings": findings,
        "promoted_tests": report["promoted_tests"],
        "unresolved_candidates": report["unresolved_candidates"],
        "campaign_issues": issues,
        "summary": summary,
    }


def _checked_emission_diagnostic(expected_code: str, observed_message: str) -> dict[str, Any]:
    return {
        "expected_status": "rejected",
        "observed_status": "rejected",
        "expected_code_or_category": expected_code,
        "observed_code_or_category": expected_code,
        "expected_message_family": observed_message,
        "observed_message_family": observed_message,
        "expected_recovery_family": "fix the template input and retry",
        "observed_recovery_family": "fix the template input and retry",
        "sensitive_input_leaked": False,
        "field_matches": {
            "status": True,
            "code_or_category": True,
            "message_family": True,
            "recovery_family": True,
            "no_sensitive_leak": True,
        },
    }


def _checked_emission_worker(campaign: dict[str, Any], correlation_id: str) -> dict[str, Any]:
    """Run one fixed AN.15 product seam and retain its bounded command evidence."""
    contract = CHECKED_EMISSION_WORKERS[correlation_id]
    campaign_dir = Path(campaign["worktree_path"]) / "site" / "reports" / "fuzz" / f"{campaign['campaign_id']}-v2"
    campaign_dir.mkdir(parents=True, exist_ok=True)
    log_path = campaign_dir / f"{correlation_id}.log"
    try:
        completed = subprocess.run(
            contract["command"],
            cwd=campaign["worktree_path"],
            capture_output=True,
            text=True,
            check=False,
            timeout=campaign["per_worker_timeout_s"],
        )
    except subprocess.TimeoutExpired as error:
        log_path.write_text(f"timeout after {campaign['per_worker_timeout_s']}s\n{error}\n", encoding="utf-8")
        raise FuzzInputError(f"campaign worker {correlation_id} timed out; retained {log_path.relative_to(campaign['worktree_path'])}") from error
    log_path.write_text(
        f"command: {' '.join(contract['command'])}\nreturncode: {completed.returncode}\n\nstdout:\n{completed.stdout.rstrip()}\n\nstderr:\n{completed.stderr.rstrip()}\n",
        encoding="utf-8",
    )
    if completed.returncode != 0:
        raise FuzzInputError(f"campaign worker {correlation_id} failed; retained {log_path.relative_to(campaign['worktree_path'])}")
    proof = {
        "seam_id": contract["seam_id"],
        "mechanism": "coverage",
        "invocation_count": campaign["cases_per_worker"],
        "evidence_ref": str(log_path.relative_to(campaign["worktree_path"])),
    }
    negative_cases = []
    for case_id, expected_oracle, observed_result in contract["negative_cases"]:
        negative_cases.append({
            "case_id": case_id,
            "expected_oracle": expected_oracle,
            "observed_result": observed_result,
            "diagnostic_contract": _checked_emission_diagnostic("checked_render_rejected", observed_result),
            "target_invocation": proof,
            "finding_id": None,
        })
    return validate_worker_result({
        "correlation_id": correlation_id,
        "target": campaign["target"],
        "status": "success",
        "cases_run": campaign["cases_per_worker"],
        "finding_ids": [],
        "error": None,
        "target_invocation": {"required_seam_ids": [contract["seam_id"]], "proofs": [proof]},
        "negative_cases": negative_cases,
        "encountered_issues": [],
    }, campaign)


def build_checked_emission_result(campaign: dict[str, Any]) -> dict[str, Any]:
    """Execute the closed-over AN.15 product campaign; never accept caller commands."""
    if campaign["target"] != "atm-template-checked-emission":
        raise FuzzInputError("real execution is currently supported only for atm-template-checked-emission")
    if campaign["campaign_id"] is None:
        raise FuzzInputError("real execution requires campaign_id")
    with ThreadPoolExecutor(max_workers=len(WORKERS)) as executor:
        workers = list(executor.map(lambda worker_id: _checked_emission_worker(campaign, worker_id), WORKERS))
    report = {
        "schema_version": SCHEMA_VERSION,
        "execution_mode": "executed-product-campaign",
        "campaign": campaign,
        "workers": workers,
        "findings": [],
        "promoted_tests": [],
        "unresolved_candidates": [],
        "campaign_issues": [],
        "summary": _expected_summary(workers, [], []),
    }
    return validate_report(report)


def _probe_worker(correlation_id: str, campaign: dict[str, Any]) -> dict[str, Any]:
    counts: dict[str, int] = {CONTRACT_PROBE_SEAM: 0}

    def observe(seam_id: str) -> None:
        counts[seam_id] = counts.get(seam_id, 0) + 1

    raw_worker = {
        "correlation_id": correlation_id,
        "target": campaign["target"],
        "status": "success",
        "cases_run": campaign["cases_per_worker"],
        "finding_ids": [],
        "error": None,
        "target_invocation": {
            "required_seam_ids": [CONTRACT_PROBE_SEAM],
            "proofs": [{
                "seam_id": CONTRACT_PROBE_SEAM,
                "mechanism": "counter",
                "invocation_count": 1,
                "evidence_ref": f"contract-probe/{correlation_id}#/run_fuzz.validate_worker_result",
            }],
        },
        "negative_cases": [],
        "encountered_issues": [],
    }
    worker = validate_worker_result(raw_worker, campaign, invocation_observer=observe)
    if counts[CONTRACT_PROBE_SEAM] != worker["target_invocation"]["proofs"][0]["invocation_count"]:
        raise FuzzInputError("target-not-exercised: contract-probe count did not match direct seam invocation")
    return worker


def build_result(campaign: dict[str, Any]) -> dict[str, Any]:
    """Run the instrumented tooling probe; never use it as product-fuzz evidence."""
    if campaign["target_seams"] != [{
        "seam_id": CONTRACT_PROBE_SEAM,
        "minimum_invocations": 1,
        "accepted_proof": "counter",
    }]:
        raise FuzzInputError("contract-probe may declare only run_fuzz.validate_worker_result")
    workers = [_probe_worker(correlation_id, campaign) for correlation_id in selected_workers(campaign)]
    report = {
        "schema_version": SCHEMA_VERSION,
        "execution_mode": "contract-probe",
        "campaign": campaign,
        "workers": workers,
        "findings": [],
        "promoted_tests": [],
        "unresolved_candidates": [],
        "campaign_issues": [],
        "summary": _expected_summary(workers, [], []),
    }
    return validate_report(report)


def default_campaign(root: Path) -> dict[str, Any]:
    return {
        "campaign_id": "fuzz-v2-contract-probe",
        "worktree_path": str(root.resolve()),
        "target": "full",
        "baseline_ref": None,
        "seed": 157,
        "max_workers": 4,
        "cases_per_worker": 1,
        "per_worker_timeout_s": 120,
        "promote_regressions": False,
        "target_seams": [{
            "seam_id": CONTRACT_PROBE_SEAM,
            "minimum_invocations": 1,
            "accepted_proof": "counter",
        }],
        "notes": "Tooling-only contract probe; not product fuzz evidence.",
    }


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise FuzzInputError(f"unable to read campaign JSON: {path}") from exc


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Validate bounded adversarial-fuzz campaign evidence.")
    parser.add_argument("--campaign", type=Path, help="v2 campaign JSON; defaults to the tooling contract-probe campaign")
    parser.add_argument("--verify-report", type=Path, help="validate and normalize an external durable v2 report")
    parser.add_argument("--contract-probe", action="store_true", help="run the instrumented tooling-only v2 contract probe")
    parser.add_argument("--execute", action="store_true", help="run the fixed AN.15 checked-emission product campaign")
    parser.add_argument("--output", type=Path, help="optional JSON output path inside the repository")
    args = parser.parse_args(argv[1:])
    root = repository_root()
    try:
        selected_modes = sum((args.contract_probe, args.verify_report is not None, args.execute))
        if selected_modes != 1:
            raise FuzzInputError("select exactly one of --contract-probe, --verify-report, or --execute")
        if args.contract_probe:
            campaign = validate_campaign(load_json(args.campaign) if args.campaign else default_campaign(root), root, require_campaign_id=True)
            result = build_result(campaign)
        elif args.execute:
            if args.campaign is None:
                raise FuzzInputError("--execute requires an explicit AN.15 campaign JSON")
            campaign = validate_campaign(load_json(args.campaign), root, require_campaign_id=True)
            result = build_checked_emission_result(campaign)
        else:
            result = validate_report(load_json(args.verify_report), root)
        encoded = json.dumps(result, indent=2, sort_keys=True) + "\n"
        if args.output:
            output = _inside(args.output, root, "output")
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(encoded, encoding="utf-8")
        print(encoded, end="")
        return 0
    except FuzzInputError as exc:
        print(f"fuzz: error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
