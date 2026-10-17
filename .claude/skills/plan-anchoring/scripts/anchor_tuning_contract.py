#!/usr/bin/env python3
"""Deterministic relational checks for anchor-tuning request/result contracts.

Contract violations are returned as ``Err(ToolError)`` values (see
``result.py``). The CLI prints one ``{success, data, error}`` JSON envelope:
exit 0 with ``data = {"valid": true, "sha256": ...}`` when the object passes,
exit 2 with a typed ``error`` for a violation, a missing field, unreadable or
non-JSON input, or an unexpected failure (``SCRIPT.INTERNAL``).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from result import Err, Ok, Result, ToolError, err  # noqa: E402


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


Check = Result[None, ToolError]
OK = Ok(None)


def canonical_bytes(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def sha256(value: object) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def reject_forbidden(value: object, path: str = "$") -> Check:
    if isinstance(value, dict):
        for key, child in value.items():
            if key in FORBIDDEN_FIELDS:
                return err("VALIDATION.FORBIDDEN_FIELD", f"{path}.{key}")
            checked = reject_forbidden(child, f"{path}.{key}")
            if isinstance(checked, Err):
                return checked
    elif isinstance(value, list):
        for index, child in enumerate(value):
            checked = reject_forbidden(child, f"{path}[{index}]")
            if isinstance(checked, Err):
                return checked
    return OK


def validate_history(history: dict) -> Check:
    events = history.get("events")
    if not isinstance(events, list):
        return err("HISTORY.INVALID", "events must be an array")
    for index, event in enumerate(events, start=1):
        if event.get("event_index") != index:
            return err("HISTORY.INDEX", "event_index is not contiguous")
        expected = None if index == 1 else sha256(events[index - 2])
        if event.get("previous_event_sha256") != expected:
            return err("HISTORY.CHAIN", "previous_event_sha256 mismatch")
        if (index > 1 or event.get("event") == "protocol_re_evaluated") and not isinstance(event.get("supersedes"), dict):
            return err("HISTORY.PREDECESSOR", "migrations and later events require a supersedes object")
        checked = reject_forbidden(event, f"$.events[{index - 1}]")
        if isinstance(checked, Err):
            return checked
    return OK


def validate_request(request: dict) -> Check:
    checked = reject_forbidden(request)
    if isinstance(checked, Err):
        return checked
    runtime = request["runtime"]
    expected_model = {"claude": "opus", "codex": "sol", "cursor": "grok[effort=high]"}.get(runtime["harness"])
    if runtime["model"] != expected_model:
        return err("VALIDATION.MODEL", f"{runtime['harness']} anchor-tuning requires {expected_model}")
    operation = request.get("operation")
    if operation in ("testability", "tune"):
        if request.get("acceptance_threshold") is not None:
            return err("VALIDATION.DEPRECATED", "acceptance_threshold is forbidden")
        if request.get("confidence_reference_threshold") != 0.8:
            return err("VALIDATION.CONFIDENCE_REFERENCE", "expected 0.8")
        records = request["authority"]["anchors"]["records"]
        if operation == "tune" and not records:
            return err("VALIDATION.AUTHORITY", "tune requires at least one locked anchor")
        if operation == "testability":
            if request.get("gate") != "testability":
                return err("VALIDATION.GATE", "testability operation requires testability gate")
            questions = request.get("current_questions")
            if not isinstance(questions, dict) or set(questions) != ROLE_IDS["testability"]:
                return err("TOURNAMENT.ROLE_SET", "testability rubric requires five calibrated questions")
            rendered = json.dumps(questions, sort_keys=True).lower()
            for phrase in FORBIDDEN_QUESTION_META:
                if phrase in rendered:
                    return err("TOURNAMENT.QUESTION_META", f"rubric question contains {phrase!r}")
            corpus = request["calibration_corpus"]
            if not corpus.get("frozen_before_live"):
                return err("TOURNAMENT.ORACLE", "calibration corpus was not frozen before live calls")
        else:
            return validate_history(request["tuning_history"])
    elif operation == "triage":
        return validate_questions(request["gate"], request["sealed_question_set"])
    return OK


def validate_questions(gate: str, sealed: dict) -> Check:
    questions = sealed.get("questions") if isinstance(sealed, dict) else None
    if not isinstance(questions, dict) or set(questions) != ROLE_IDS.get(gate):
        return err("TOURNAMENT.ROLE_SET", f"expected {sorted(ROLE_IDS.get(gate, []))}")
    actual = sha256({"questions": questions})
    if sealed.get("digest") != actual:
        return err("TOURNAMENT.QUESTION_DIGEST", "sealed digest mismatch")
    rendered = json.dumps(questions, sort_keys=True).lower()
    for phrase in FORBIDDEN_QUESTION_META:
        if phrase in rendered:
            return err("TOURNAMENT.QUESTION_META", f"production question contains {phrase!r}")
    return OK


def validate_tune_result(data: dict) -> Check:
    score = data["semantic_score"]
    for numerator, denominator in (
        ("exact_role_cells", "total_role_cells"),
        ("exact_semantic_verdicts", "total_semantic_verdicts"),
        ("stable_cases", "total_cases"),
    ):
        if score[numerator] > score[denominator]:
            return err("TOURNAMENT.SCORE", f"{numerator} exceeds {denominator}")
    exact = (
        score["exact_role_cells"] == score["total_role_cells"]
        and score["exact_semantic_verdicts"] == score["total_semantic_verdicts"]
        and score["stable_cases"] == score["total_cases"]
        and score["regressions"] == 0
    )
    diagnostics = data["confidence_diagnostics"]
    if diagnostics["threshold"] != data["confidence_reference_threshold"]:
        return err("TOURNAMENT.CONFIDENCE_REFERENCE", "diagnostic threshold mismatch")
    if diagnostics["cells_at_or_above_threshold"] > diagnostics["total_cells"]:
        return err("TOURNAMENT.CONFIDENCE", "passing cells exceed total cells")
    if diagnostics["total_cells"] != score["total_role_cells"]:
        return err("TOURNAMENT.CONFIDENCE", "total cells differ from semantic score")

    history = data["tuning_history_after"]
    checked = validate_history(history)
    if isinstance(checked, Err):
        return checked
    if sha256(history) != data["tuning_history_after_sha256"]:
        return err("HISTORY.DIGEST", "history_after digest mismatch")
    before = {"schema": history["schema"], "history_id": history["history_id"], "events": history["events"][:-1]}
    if sha256(before) != data["tuning_history_before_sha256"]:
        return err("HISTORY.DIGEST", "history_before digest mismatch")
    last = history["events"][-1]
    event_score = dict(last["score"])
    event_confidence = event_score.pop("confidence")
    if event_score != score or event_confidence != diagnostics:
        return err("HISTORY.SCORE", "history score differs from result")

    if data["accepted"]:
        if not exact:
            return err("TOURNAMENT.SEMANTIC_REJECTED", "accepted score is not exact and stable")
        sealed = data["sealed_question_set"]
        checked = validate_questions(data["gate"], sealed)
        if isinstance(checked, Err):
            return checked
        if data["sealed_question_set_digest_after"] != sealed["digest"]:
            return err("TOURNAMENT.QUESTION_DIGEST", "result digest mismatch")
        if last["decision"]["status"] != "sealed":
            return err("HISTORY.DECISION", "accepted result is not sealed")
        if last["decision"]["question_set_sha256"] != sealed["digest"]:
            return err("HISTORY.DECISION", "history question digest mismatch")
    else:
        if data["sealed_question_set"] is not None or data["sealed_question_set_digest_after"] is not None:
            return err("TOURNAMENT.UNSEALED", "rejected result contains a seal")
        if last["decision"]["status"] == "sealed":
            return err("HISTORY.DECISION", "rejected result carries a sealed decision")
        if last["decision"]["question_set_sha256"] is not None:
            return err("HISTORY.DECISION", "rejected result carries a question digest")
    return OK


def validate_testability_result(data: dict) -> Check:
    summary = data["confirmation_summary"]
    semantic = data["semantic_score"]
    dedupe = data["dedupe_score"]
    expected_semantic_cells = data["corpus_case_count"] * len(ROLE_IDS["testability"])
    if semantic["total_cells"] != expected_semantic_cells:
        return err("RUBRIC.SCORE", "semantic total does not match corpus cases and roles")
    if dedupe["total_cells"] != data["dedupe_case_count"]:
        return err("RUBRIC.SCORE", "dedupe total does not match dedupe case count")
    for label, score in (("semantic", semantic), ("dedupe", dedupe)):
        for field in ("exact_cells", "confident_cells", "stable_cells"):
            if score[field] > score["total_cells"]:
                return err("RUBRIC.SCORE", f"{label} {field} exceeds total")
    if summary["two_call_cells"] != semantic["total_cells"]:
        return err("RUBRIC.CONFIRMATION", "two-call cells differ from semantic total")
    if not data["repeat_requests_byte_identical"]:
        return err("RUBRIC.REQUEST_DRIFT", "repeated requests were not byte-identical")
    if data["accepted"]:
        if summary["inconclusive_cells"] or summary["unstable_cells"]:
            return err("RUBRIC.UNSEALED", "accepted rubric has inconclusive or unstable cells")
        for label, score in (("semantic", semantic), ("dedupe", dedupe)):
            if any(score[field] != score["total_cells"] for field in ("exact_cells", "confident_cells", "stable_cells")):
                return err("RUBRIC.UNSEALED", f"accepted {label} score is not exact, confident, and stable")
        sealed = data["rubric_question_set"]
        questions = sealed.get("questions") if isinstance(sealed, dict) else None
        if not isinstance(questions, dict) or set(questions) != ROLE_IDS["testability"]:
            return err("RUBRIC.QUESTION_SET", "accepted rubric lacks the five calibrated questions")
        if sealed.get("digest") != sha256({"questions": questions}):
            return err("RUBRIC.QUESTION_DIGEST", "rubric digest mismatch")
        rendered = json.dumps(questions, sort_keys=True).lower()
        for phrase in FORBIDDEN_QUESTION_META:
            if phrase in rendered:
                return err("RUBRIC.QUESTION_META", f"rubric contains {phrase!r}")
    elif data.get("rubric_question_set") is not None:
        return err("RUBRIC.UNSEALED", "rejected calibration contains a sealed rubric")
    return OK


def validate_result(result: dict) -> Check:
    checked = reject_forbidden(result)
    if isinstance(checked, Err):
        return checked
    if not result.get("success"):
        return OK
    data = result.get("data") or {}
    operation = data.get("operation")
    if operation == "testability":
        return validate_testability_result(data)
    elif operation == "tune":
        return validate_tune_result(data)
    elif operation == "triage":
        checked = validate_questions(data.get("gate", "planning"), data["sealed_question_set"])
        if isinstance(checked, Err):
            return checked
        if data["disposition"] == "plan_violation" and (
            data["sealed_question_set_digest_before"]
            != data["sealed_question_set_digest_after"]
        ):
            return err("TRIAGE.DIGEST_CHANGED", "plan_violation changed questions")
        return OK
    else:
        return err("VALIDATION.OPERATION", "unknown result operation")


def render_error(error: ToolError) -> str:
    """One-line ``CODE: detail`` form of a contract violation, for logs and messages."""

    return f"{error.code}: {error.message}"


def _emit(value: dict) -> None:
    print(json.dumps(value, sort_keys=True, separators=(",", ":")))


def _check_file(kind: str, path: pathlib.Path) -> Result[dict, ToolError]:
    """Read, decode and check one contract file. Raises only for unexpected failures."""

    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError):
        return err("SCRIPT.IO", "input could not be read")
    try:
        value = json.loads(text)
    except json.JSONDecodeError as exc:
        return err("VALIDATION.JSON", f"input is not valid JSON: {exc}")
    if not isinstance(value, dict):
        return err("VALIDATION.SHAPE", f"{kind} must be a JSON object")
    try:
        checked = (validate_request if kind == "request" else validate_result)(value)
    except KeyError as exc:
        return err("VALIDATION.MISSING_FIELD", f"required field is missing: {exc.args[0]}")
    if isinstance(checked, Err):
        return checked
    return Ok({"valid": True, "sha256": sha256(value)})


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("request", "result"))
    parser.add_argument("path", type=pathlib.Path)
    args = parser.parse_args(argv)
    try:
        outcome = _check_file(args.kind, args.path)
    except Exception as exc:  # noqa: BLE001 - the CLI boundary: a JSON error object beats a traceback
        outcome = err("SCRIPT.INTERNAL", f"{type(exc).__name__}: {exc}",
                      action="Validate the object against its JSON schema first, then report this with the input.")
    if isinstance(outcome, Err):
        _emit({"success": False, "data": None, "error": outcome.error.to_json()})
        return 2
    _emit({"success": True, "data": outcome.value, "error": None})
    return 0


if __name__ == "__main__":
    sys.exit(main())
