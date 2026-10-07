#!/usr/bin/env python3
"""Check a phase review's completion variables before rendering and closing."""
import json
from pathlib import Path
import re
import sys


def validate(values):
    if not isinstance(values, dict):
        raise ValueError("completion variables must be an object")
    verdict = values.get("verdict")
    outcome = values.get("integration_review")
    if verdict not in ("PASS", "FAIL"):
        raise ValueError("verdict must be PASS or FAIL")
    for key in ("findings_blocking", "findings_important", "findings_minor"):
        if type(values.get(key)) is not int or values[key] < 0:
            raise ValueError(f"{key} must be a nonnegative integer")
    if verdict == "PASS" and (values["findings_blocking"] or values["findings_important"]):
        raise ValueError("PASS requires zero blocking and important findings")
    expected = "integration_review_passed" if verdict == "PASS" else "integration_review_failed"
    if outcome != expected:
        raise ValueError("integration_review must agree with verdict")
    head = values.get("integration_commit", "")
    if not isinstance(head, str) or not re.fullmatch(r"[0-9a-f]{40}", head):
        raise ValueError("integration_commit must be a full commit SHA")
    if head != values.get("commit"):
        raise ValueError("integration_commit must equal the assigned reviewed commit")
    counts = values.get("post_mortem_counts", {})
    if not isinstance(counts, dict):
        raise ValueError("post_mortem_counts must be an object")
    for key in ("total", "verified_fixed", "justified_nonfix", "unresolved"):
        if type(counts.get(key)) is not int or counts[key] < 0:
            raise ValueError(f"post_mortem_counts.{key} must be a nonnegative integer")
    if counts["total"] != sum(counts[key] for key in ("verified_fixed", "justified_nonfix", "unresolved")):
        raise ValueError("post-mortem counts must sum to total")
    if verdict == "PASS" and counts["unresolved"]:
        raise ValueError("PASS requires zero unresolved findings")
    jev = values.get("post_mortem_jev")
    if not isinstance(jev, dict) or jev.get("status") not in ("completed", "not_applicable"):
        raise ValueError("post_mortem_jev.status must be completed or not_applicable; "
                         "unavailable is a REVIEW_PENDING_JEV refusal, not a completion")
    evidence = values.get("post_mortem_md")
    if not isinstance(evidence, str) or not evidence.strip():
        raise ValueError("post_mortem_md must contain the per-finding evidence")


def main():
    try:
        if len(sys.argv) != 2:
            raise ValueError("usage: check-review-completion.py <completion-vars.json>")
        validate(json.loads(Path(sys.argv[1]).read_text()))
    except (ValueError, OSError) as error:
        print(f"review completion: {error}", file=sys.stderr)
        return 1
    print("review completion variables valid; reviewer must still verify the evidence")
    return 0


if __name__ == "__main__":
    sys.exit(main())
