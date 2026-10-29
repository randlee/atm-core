#!/usr/bin/env python3
# pyright: strict
"""Check a phase review's completion variables before rendering and closing."""
import json
from pathlib import Path
import re
import sys
from typing import NotRequired, TypedDict
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads" / "scripts"))

from operation_models import matches_typeddict


class PostMortemCounts(TypedDict, total=False):
    total: int
    verified_fixed: int
    justified_nonfix: int
    unresolved: int


class PostMortemJev(TypedDict, total=False):
    status: str


class CompletionVariables(TypedDict, total=False):
    verdict: str
    integration_review: str
    findings_blocking: int
    findings_important: int
    findings_minor: int
    integration_commit: str
    commit: str
    post_mortem_counts: PostMortemCounts
    post_mortem_jev: PostMortemJev
    post_mortem_md: str
    stack_view: NotRequired[str]


def nonnegative_count(value: int | None, field: str, context: str) -> int:
    if type(value) is not int or value < 0:
        raise ValueError(f"{context}.{field} must be a nonnegative integer")
    return value


def validate(value: object) -> None:
    if not matches_typeddict(value, CompletionVariables):
        raise ValueError("completion variables must be an object with correctly typed fields")
    values = value
    verdict = values.get("verdict")
    outcome = values.get("integration_review")
    if verdict not in ("PASS", "FAIL"):
        raise ValueError("verdict must be PASS or FAIL")
    findings_blocking = nonnegative_count(values.get("findings_blocking"), "findings_blocking", "completion variables")
    findings_important = nonnegative_count(values.get("findings_important"), "findings_important", "completion variables")
    nonnegative_count(values.get("findings_minor"), "findings_minor", "completion variables")
    if verdict == "PASS" and (findings_blocking or findings_important):
        raise ValueError("PASS requires zero blocking and important findings")
    expected = "integration_review_passed" if verdict == "PASS" else "integration_review_failed"
    if outcome != expected:
        raise ValueError("integration_review must agree with verdict")
    head = values.get("integration_commit", "")
    if not re.fullmatch(r"[0-9a-f]{40}", head):
        raise ValueError("integration_commit must be a full commit SHA")
    if head != values.get("commit"):
        raise ValueError("integration_commit must equal the assigned reviewed commit")
    raw_counts = values.get("post_mortem_counts", {})
    total = nonnegative_count(raw_counts.get("total"), "total", "post_mortem_counts")
    verified_fixed = nonnegative_count(raw_counts.get("verified_fixed"), "verified_fixed", "post_mortem_counts")
    justified_nonfix = nonnegative_count(raw_counts.get("justified_nonfix"), "justified_nonfix", "post_mortem_counts")
    unresolved = nonnegative_count(raw_counts.get("unresolved"), "unresolved", "post_mortem_counts")
    if total != verified_fixed + justified_nonfix + unresolved:
        raise ValueError("post-mortem counts must sum to total")
    if verdict == "PASS" and unresolved:
        raise ValueError("PASS requires zero unresolved findings")
    jev = values.get("post_mortem_jev")
    if jev is None or jev.get("status") not in ("completed", "not_applicable"):
        raise ValueError("post_mortem_jev.status must be completed or not_applicable; "
                         "unavailable is a REVIEW_PENDING_JEV refusal, not a completion")
    evidence = values.get("post_mortem_md")
    if not evidence or not evidence.strip():
        raise ValueError("post_mortem_md must contain the per-finding evidence")


def main() -> int:
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
