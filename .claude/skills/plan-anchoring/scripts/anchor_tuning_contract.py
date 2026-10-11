#!/usr/bin/env python3
"""Deterministic relational checks for anchor-tuning request/result contracts."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys


FORBIDDEN_FIELDS = {"desired_verdict", "requested_disposition", "proposed_question_text"}
FORBIDDEN_QUESTION_META = (
    "confidence at least",
    "confidence threshold",
    "final verdict",
    "role-to-verdict",
    "classify the three roles",
    "select that category confidently",
)
ROLE_IDS = {
    "testability": {"single_constraint", "observable", "faithful", "scoped", "self_contained"},
    "planning": {"relevant", "addressed", "contradicted"},
    "development": {"relevant", "implemented", "violated", "evidenced"},
}


class ContractError(ValueError):
    pass


def canonical_bytes(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def sha256(value: object) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def reject_forbidden(value: object, path: str = "$") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if key in FORBIDDEN_FIELDS:
                raise ContractError(f"VALIDATION.FORBIDDEN_FIELD: {path}.{key}")
            reject_forbidden(child, f"{path}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            reject_forbidden(child, f"{path}[{index}]")


def validate_history(history: dict) -> None:
    events = history.get("events")
    if not isinstance(events, list):
        raise ContractError("HISTORY.INVALID: events must be an array")
    for index, event in enumerate(events, start=1):
        if event.get("event_index") != index:
            raise ContractError("HISTORY.INDEX: event_index is not contiguous")
        expected = None if index == 1 else sha256(events[index - 2])
        if event.get("previous_event_sha256") != expected:
            raise ContractError("HISTORY.CHAIN: previous_event_sha256 mismatch")
        reject_forbidden(event, f"$.events[{index - 1}]")


def validate_request(request: dict) -> None:
    reject_forbidden(request)
    runtime = request["runtime"]
    expected_model = {"claude": "opus", "codex": "sol"}.get(runtime["harness"])
    if runtime["model"] != expected_model:
        raise ContractError(
            f"VALIDATION.MODEL: {runtime['harness']} anchor-tuning requires {expected_model}"
        )
    operation = request.get("operation")
    if operation in ("testability", "tune"):
        if request.get("acceptance_threshold") is not None:
            raise ContractError("VALIDATION.DEPRECATED: acceptance_threshold is forbidden")
        if request.get("confidence_reference_threshold") != 0.8:
            raise ContractError("VALIDATION.CONFIDENCE_REFERENCE: expected 0.8")
        records = request["authority"]["anchors"]["records"]
        if operation == "tune" and not records:
            raise ContractError("VALIDATION.AUTHORITY: tune requires at least one locked anchor")
        if operation == "testability":
            if request.get("gate") != "testability":
                raise ContractError("VALIDATION.GATE: testability operation requires testability gate")
            questions = request.get("current_questions")
            if not isinstance(questions, dict) or set(questions) != ROLE_IDS["testability"]:
                raise ContractError("TOURNAMENT.ROLE_SET: testability rubric requires five calibrated questions")
            rendered = json.dumps(questions, sort_keys=True).lower()
            for phrase in FORBIDDEN_QUESTION_META:
                if phrase in rendered:
                    raise ContractError(
                        f"TOURNAMENT.QUESTION_META: rubric question contains {phrase!r}"
                    )
            corpus = request["calibration_corpus"]
            if not corpus.get("frozen_before_live"):
                raise ContractError("TOURNAMENT.ORACLE: calibration corpus was not frozen before live calls")
        else:
            validate_history(request["tuning_history"])
    elif operation == "triage":
        validate_questions(request["gate"], request["sealed_question_set"])


def validate_questions(gate: str, sealed: dict) -> None:
    questions = sealed.get("questions") if isinstance(sealed, dict) else None
    if not isinstance(questions, dict) or set(questions) != ROLE_IDS.get(gate):
        raise ContractError(f"TOURNAMENT.ROLE_SET: expected {sorted(ROLE_IDS.get(gate, []))}")
    actual = sha256({"questions": questions})
    if sealed.get("digest") != actual:
        raise ContractError("TOURNAMENT.QUESTION_DIGEST: sealed digest mismatch")
    rendered = json.dumps(questions, sort_keys=True).lower()
    for phrase in FORBIDDEN_QUESTION_META:
        if phrase in rendered:
            raise ContractError(
                f"TOURNAMENT.QUESTION_META: production question contains {phrase!r}"
            )


def validate_tune_result(data: dict) -> None:
    score = data["semantic_score"]
    for numerator, denominator in (
        ("exact_role_cells", "total_role_cells"),
        ("exact_semantic_verdicts", "total_semantic_verdicts"),
        ("stable_cases", "total_cases"),
    ):
        if score[numerator] > score[denominator]:
            raise ContractError(f"TOURNAMENT.SCORE: {numerator} exceeds {denominator}")
    exact = (
        score["exact_role_cells"] == score["total_role_cells"]
        and score["exact_semantic_verdicts"] == score["total_semantic_verdicts"]
        and score["stable_cases"] == score["total_cases"]
        and score["regressions"] == 0
    )
    diagnostics = data["confidence_diagnostics"]
    if diagnostics["threshold"] != data["confidence_reference_threshold"]:
        raise ContractError("TOURNAMENT.CONFIDENCE_REFERENCE: diagnostic threshold mismatch")
    if diagnostics["cells_at_or_above_threshold"] > diagnostics["total_cells"]:
        raise ContractError("TOURNAMENT.CONFIDENCE: passing cells exceed total cells")
    if diagnostics["total_cells"] != score["total_role_cells"]:
        raise ContractError("TOURNAMENT.CONFIDENCE: total cells differ from semantic score")

    history = data["tuning_history_after"]
    validate_history(history)
    if sha256(history) != data["tuning_history_after_sha256"]:
        raise ContractError("HISTORY.DIGEST: history_after digest mismatch")
    before = {"schema": history["schema"], "history_id": history["history_id"], "events": history["events"][:-1]}
    if sha256(before) != data["tuning_history_before_sha256"]:
        raise ContractError("HISTORY.DIGEST: history_before digest mismatch")
    last = history["events"][-1]
    event_score = dict(last["score"])
    event_confidence = event_score.pop("confidence")
    if event_score != score or event_confidence != diagnostics:
        raise ContractError("HISTORY.SCORE: history score differs from result")

    if data["accepted"]:
        if not exact:
            raise ContractError("TOURNAMENT.SEMANTIC_REJECTED: accepted score is not exact and stable")
        sealed = data["sealed_question_set"]
        validate_questions(data["gate"], sealed)
        if data["sealed_question_set_digest_after"] != sealed["digest"]:
            raise ContractError("TOURNAMENT.QUESTION_DIGEST: result digest mismatch")
        if last["decision"]["status"] != "sealed":
            raise ContractError("HISTORY.DECISION: accepted result is not sealed")
        if last["decision"]["question_set_sha256"] != sealed["digest"]:
            raise ContractError("HISTORY.DECISION: history question digest mismatch")
    else:
        if data["sealed_question_set"] is not None or data["sealed_question_set_digest_after"] is not None:
            raise ContractError("TOURNAMENT.UNSEALED: rejected result contains a seal")
        if last["decision"]["status"] == "sealed":
            raise ContractError("HISTORY.DECISION: rejected result carries a sealed decision")
        if last["decision"]["question_set_sha256"] is not None:
            raise ContractError("HISTORY.DECISION: rejected result carries a question digest")


def validate_testability_result(data: dict) -> None:
    summary = data["confirmation_summary"]
    semantic = data["semantic_score"]
    dedupe = data["dedupe_score"]
    expected_semantic_cells = data["corpus_case_count"] * len(ROLE_IDS["testability"])
    if semantic["total_cells"] != expected_semantic_cells:
        raise ContractError("RUBRIC.SCORE: semantic total does not match corpus cases and roles")
    if dedupe["total_cells"] != data["dedupe_case_count"]:
        raise ContractError("RUBRIC.SCORE: dedupe total does not match dedupe case count")
    for label, score in (("semantic", semantic), ("dedupe", dedupe)):
        for field in ("exact_cells", "confident_cells", "stable_cells"):
            if score[field] > score["total_cells"]:
                raise ContractError(f"RUBRIC.SCORE: {label} {field} exceeds total")
    if summary["two_call_cells"] != semantic["total_cells"]:
        raise ContractError("RUBRIC.CONFIRMATION: two-call cells differ from semantic total")
    if not data["repeat_requests_byte_identical"]:
        raise ContractError("RUBRIC.REQUEST_DRIFT: repeated requests were not byte-identical")
    if data["accepted"]:
        if summary["inconclusive_cells"] or summary["unstable_cells"]:
            raise ContractError("RUBRIC.UNSEALED: accepted rubric has inconclusive or unstable cells")
        for label, score in (("semantic", semantic), ("dedupe", dedupe)):
            if any(score[field] != score["total_cells"] for field in ("exact_cells", "confident_cells", "stable_cells")):
                raise ContractError(f"RUBRIC.UNSEALED: accepted {label} score is not exact, confident, and stable")
        sealed = data["rubric_question_set"]
        questions = sealed.get("questions") if isinstance(sealed, dict) else None
        if not isinstance(questions, dict) or set(questions) != ROLE_IDS["testability"]:
            raise ContractError("RUBRIC.QUESTION_SET: accepted rubric lacks the five calibrated questions")
        if sealed.get("digest") != sha256({"questions": questions}):
            raise ContractError("RUBRIC.QUESTION_DIGEST: rubric digest mismatch")
        rendered = json.dumps(questions, sort_keys=True).lower()
        for phrase in FORBIDDEN_QUESTION_META:
            if phrase in rendered:
                raise ContractError(f"RUBRIC.QUESTION_META: rubric contains {phrase!r}")
    elif data.get("rubric_question_set") is not None:
        raise ContractError("RUBRIC.UNSEALED: rejected calibration contains a sealed rubric")


def validate_result(result: dict) -> None:
    reject_forbidden(result)
    if not result.get("success"):
        return
    data = result.get("data") or {}
    operation = data.get("operation")
    if operation == "testability":
        validate_testability_result(data)
    elif operation == "tune":
        validate_tune_result(data)
    elif operation == "triage":
        validate_questions(data.get("gate", "planning"), data["sealed_question_set"])
        if data["disposition"] == "plan_violation" and (
            data["sealed_question_set_digest_before"]
            != data["sealed_question_set_digest_after"]
        ):
            raise ContractError("TRIAGE.DIGEST_CHANGED: plan_violation changed questions")
    else:
        raise ContractError("VALIDATION.OPERATION: unknown result operation")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("request", "result"))
    parser.add_argument("path", type=pathlib.Path)
    args = parser.parse_args()
    try:
        value = json.loads(args.path.read_text(encoding="utf-8"))
        (validate_request if args.kind == "request" else validate_result)(value)
    except (OSError, json.JSONDecodeError, KeyError, ContractError) as exc:
        print(json.dumps({"valid": False, "error": str(exc)}, sort_keys=True))
        return 1
    print(json.dumps({"valid": True, "sha256": sha256(value)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
