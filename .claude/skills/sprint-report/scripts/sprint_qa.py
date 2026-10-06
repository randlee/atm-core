"""QA state and finding counts shared by table and DAG reports."""

import sys
from typing import Any


def metadata(bead: dict[str, Any]) -> dict[str, Any]:
    value = bead.get("metadata")
    return value if isinstance(value, dict) else {}

def choose_round(candidates: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not candidates:
        return None
    return max(
        candidates,
        key=lambda bead: (
            int(str(metadata(bead).get("round", "0")))
            if str(metadata(bead).get("round", "0")).isdigit() else 0,
            str(bead.get("id", "")),
        ),
    )

def qa_verdict(qa: dict[str, Any]) -> str:
    qa_meta = metadata(qa)
    verdict = str(qa_meta.get("verdict") or "").upper()
    if not verdict:
        close_reason = str(qa.get("close_reason") or "")
        verdict = close_reason.split(":", 1)[0].strip().upper()
    if verdict not in {"PASS", "FAIL"}:
        verdict = "IN PROGRESS" if qa.get("status") != "closed" else "UNKNOWN"
    return verdict

def qa_icon(qa: dict[str, Any] | None, findings: list[dict[str, Any]]) -> str:
    if not qa:
        return ""
    verdict = qa_verdict(qa)
    if verdict == "FAIL" or any(finding.get("status") != "closed" for finding in findings):
        return "🚩"
    if verdict == "PASS":
        return "✅"
    if qa.get("status") == "in_progress":
        return "🌀"
    if qa.get("status") in {"open", "deferred", "blocked"}:
        return "📥"
    return ""

def findings_summary(findings: list[dict[str, Any]]) -> str:
    counts = dict.fromkeys(("blocking", "important", "minor"), 0)
    for finding in findings:
        if finding.get("status") == "closed":
            continue
        severity = str(metadata(finding).get("severity") or next(
            (label.removeprefix("severity:") for label in finding.get("labels") or []
             if label.startswith("severity:")), ""
        )).lower()
        severity = {"critical": "blocking", "low": "minor"}.get(severity, severity)
        if severity not in counts:
            print(f"sprint-report: unknown severity for finding {finding.get('id')}", file=sys.stderr)
            return "?"
        counts[severity] += 1
    return ":".join(str(count) for count in counts.values())
