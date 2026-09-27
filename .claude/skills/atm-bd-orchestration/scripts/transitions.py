"""Pure, bead-shaped transition decisions used by the orchestration fixtures."""
from __future__ import annotations

from typing import Any


def next_transition(sprint_state: dict[str, Any]) -> str:
    """Return the single next action without reading or mutating bead state."""
    sanity = sprint_state.get("sanity") or {}
    qa = sprint_state.get("qa") or {}
    findings = sprint_state.get("findings") or []
    if sanity.get("status") == "closed" and str(sanity.get("close_reason", "")).startswith("PASS") and not qa:
        return "create QA bead"
    open_severities = {
        (finding.get("metadata") or {}).get("severity")
        for finding in findings if finding.get("status") != "closed"
    }
    if qa.get("status") == "closed" and str(qa.get("close_reason", "")).startswith("PASS") and open_severities <= {"minor"}:
        return "PASS with backlog"
    if qa.get("status") == "closed" and str(qa.get("close_reason", "")).startswith("FAIL"):
        if int((qa.get("metadata") or {}).get("round", 1)) >= 2:
            return "ROUND_CAP"
        if open_severities & {"blocking", "important"}:
            return "one fix round"
    return "wait"
