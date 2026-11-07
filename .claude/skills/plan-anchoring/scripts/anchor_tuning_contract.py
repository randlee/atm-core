#!/usr/bin/env python3
"""Deterministic relational checks for anchor-tuning request/result contracts.

Contract violations are returned as ``Err(ToolError)`` values (see
``result.py``). The CLI prints one ``{success, data, error}`` JSON envelope.
A file that was read and checked exits 0, valid or not: ``data = {"valid":
true, "sha256": ...}`` when the object passes, ``data = {"valid": false,
"violation": {"code", "message"}}`` for a contract violation, a missing field,
a non-object or non-JSON content. Exit 2 means the check could not run:
unreadable input (``SCRIPT.IO``), bad arguments (``VALIDATION.INPUT``), or an
unexpected failure (``SCRIPT.INTERNAL``). There is no exit 1.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from result import Err, Ok, Result, ToolError, err, run_main, utf8_stdio  # noqa: E402


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
        checked = validate_questions(request["gate"], request["sealed_question_set"])
        if isinstance(checked, Err):
            return checked
        if request["gate"] == "testability":
            return validate_testability_triage(request)
        return OK
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


def validate_testability_triage(request: dict) -> Check:
    """Bind a disputed rubric response to its approved source and exact seal."""
    authority = request["authority"]
    records = authority["anchors"]["records"]
    if not records:
        return err("VALIDATION.AUTHORITY", "testability triage requires an approved locked anchor")
    for record in records:
        fixtures = record.get("fixtures")
        if record.get("state") != "locked" or not isinstance(fixtures, dict) or any(
            not isinstance(fixtures.get(kind), list) or not fixtures[kind]
            or any(not isinstance(text, str) or not text.strip() for text in fixtures[kind])
            for kind in ("pass", "fail")
        ):
            return err("VALIDATION.FIXTURES", "testability triage requires locked pass/fail fixture pairs")
    intent = authority["intent"]
    if intent["sha256"] != hashlib.sha256(intent["content"].encode("utf-8")).hexdigest():
        return err("TRIAGE.AUTHORITY_DIGEST", "intent text digest mismatch")
    if authority["anchors"]["sha256"] != sha256(records):
        return err("TRIAGE.AUTHORITY_DIGEST", "anchor records digest mismatch")
    review = request["review"]
    if review["role"] not in ROLE_IDS["testability"]:
        return err("VALIDATION.ROLE", "testability triage requires a rubric role")
    canonical = review["canonical_request"]
    if canonical.get("questions") != request["sealed_question_set"]["questions"]:
        return err("TRIAGE.REQUEST_QUESTIONS", "review request differs from sealed rubric")
    state = canonical.get("state")
    source = state.get("anchor") if isinstance(state, dict) else None
    if not isinstance(source, dict) or set(source) != {"statement", "quote", "scope"} or any(
        not isinstance(source.get(key), str) or not source[key].strip()
        for key in ("statement", "quote", "scope")
    ):
        return err("TRIAGE.SOURCE", "canonical request must contain the complete source anchor")
    if review["source_digest"] != sha256(source):
        return err("TRIAGE.SOURCE_DIGEST", "source anchor digest mismatch")
    if not any(all(source[key] == record.get(key) for key in ("statement", "quote", "scope")) for record in records):
        return err("TRIAGE.SOURCE_AUTHORITY", "source anchor is not in the approved authority")
    answers = review["raw_response"].get("answers")
    if not isinstance(answers, dict) or set(answers) != ROLE_IDS["testability"]:
        return err("TRIAGE.RESPONSE", "raw response must contain all five rubric answers")
    for answer in answers.values():
        if not isinstance(answer, dict) or answer.get("type", "choice") != "choice" or answer.get("choice") not in ("yes", "no", "unclear"):
            return err("TRIAGE.RESPONSE", "raw response has an invalid choice answer")
        confidence = answer.get("confidence")
        probabilities = answer.get("probabilities")
        if isinstance(confidence, bool) or not isinstance(confidence, (int, float)) or not 0 <= confidence <= 1:
            return err("TRIAGE.RESPONSE", "raw response confidence must be a probability")
        if not isinstance(probabilities, dict) or set(probabilities) != {"yes", "no", "unclear"} or any(
            isinstance(value, bool) or not isinstance(value, (int, float)) or not 0 <= value <= 1
            for value in probabilities.values()
        ) or abs(sum(probabilities.values()) - 1) > 0.01 * len(probabilities):
            return err("TRIAGE.RESPONSE", "raw response probabilities are invalid")
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
        gate = data.get("gate", "planning")
        allowed = {"anchor_defect", "question_defect"} if gate == "testability" else {"plan_violation", "question_defect"}
        if data["disposition"] not in allowed:
            return err("TRIAGE.DISPOSITION", "disposition does not apply to this gate")
        expected_action = "reopen_affected_tournament" if data["disposition"] == "question_defect" else "unchanged"
        if data["question_action"] != expected_action:
            return err("TRIAGE.QUESTION_ACTION", "question action does not match disposition")
        if gate == "testability":
            if data["sealed_question_set_digest_before"] != data["sealed_question_set"]["digest"]:
                return err("TRIAGE.QUESTION_DIGEST", "triage must cite the actual rubric seal")
            if data["sealed_question_set_digest_after"] != data["sealed_question_set_digest_before"]:
                return err("TRIAGE.DIGEST_CHANGED", "testability triage diagnoses; a separate tournament changes questions")
        if data["disposition"] in ("plan_violation", "anchor_defect") and (
            data["sealed_question_set_digest_before"]
            != data["sealed_question_set_digest_after"]
        ):
            return err("TRIAGE.DIGEST_CHANGED", "accurate failure changed questions")
        return OK
    else:
        return err("VALIDATION.OPERATION", "unknown result operation")


def render_error(error: ToolError) -> str:
    """One-line ``CODE: detail`` form of a contract violation, for logs and messages."""

    return f"{error.code}: {error.message}"


def _emit(value: dict) -> None:
    print(json.dumps(value, sort_keys=True, separators=(",", ":")))


def _invalid(error: ToolError) -> Result[dict, ToolError]:
    """A completed check that found a violation: Ok with data.valid false, exit 0."""

    return Ok({"valid": False, "violation": {"code": error.code, "message": error.message}})


def _check_file(kind: str, path: pathlib.Path) -> Result[dict, ToolError]:
    """Read, decode and check one contract file. Err only when the file cannot be read; raises only for unexpected failures."""

    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError):
        return err("SCRIPT.IO", "input could not be read")
    checked = _check_text(kind, text)
    if isinstance(checked, Err):
        return _invalid(checked.error)
    return checked


def _check_text(kind: str, text: str) -> Result[dict, ToolError]:
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


class _UsageError(Exception):
    pass


class _JsonArgumentParser(argparse.ArgumentParser):
    """Usage errors become a VALIDATION.INPUT envelope (exit 2) instead of argparse's text on stderr."""

    def error(self, message):  # argparse hook; never returns
        raise _UsageError(f"{self.prog}: {message}")


def main(argv: list[str] | None = None) -> int:
    parser = _JsonArgumentParser(prog="anchor_tuning_contract.py")
    parser.add_argument("kind", choices=("request", "result"))
    parser.add_argument("path", type=pathlib.Path)
    try:
        args = parser.parse_args(argv)
    except _UsageError as exc:
        _emit({"success": False, "data": None, "error": ToolError(
            "VALIDATION.INPUT", str(exc), "Run 'anchor_tuning_contract.py --help' for the accepted arguments.").to_json()})
        return 2
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
    sys.exit(run_main(main))
