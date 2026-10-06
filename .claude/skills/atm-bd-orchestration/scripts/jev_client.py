#!/usr/bin/env python3
"""Bounded Jev transport and startup probe for the proposed sanity agent."""
import argparse
import hashlib
import hmac
import http.client
import json
import re
import math
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

MODEL = "jev-1.13.0"
HOST = "api.typesafe.ai"
MAX_REQUEST_BYTES = 96000  # Jev documents 32k state tokens of a 64k request budget; ~3 bytes per token of code.
MAX_RESPONSE_BYTES = 1048576


class JevError(Exception):
    def __init__(self, code, message, recoverable=False):
        super().__init__(message)
        self.code, self.message, self.recoverable = code, message, recoverable


REPORT = "Report this error unchanged; the coordinator records the slot unavailable."
RETRY = "Transient: wait 60 seconds and run the same command once more. If it fails again, " + REPORT[0].lower() + REPORT[1:]
FIXES = (  # message prefix -> what the calling agent does next
    ("TYPESAFE_API_KEY is missing", "No Jev call is possible without TYPESAFE_API_KEY. " + REPORT),
    ("TYPESAFE_API_KEY has invalid formatting", "TYPESAFE_API_KEY holds whitespace, quotes or control characters. " + REPORT),
    ("Assignment lacks", "Write the assignment you received unchanged: it needs `deliverable.text`, `worktree_path`, "
     "`commit`, `base_sha`, `changed_files` and `context`. If the received assignment lacks one, " + REPORT[0].lower() + REPORT[1:]),
    ("Committed evidence unreadable", "Run `git -C <worktree_path> fetch origin` so `commit` and `base_sha` exist, then run "
     "the same command again. If it still fails, a `changed_files` or `context` path is missing at `commit`: " + REPORT[0].lower() + REPORT[1:]),
    ("Request file unavailable", "Write the file as the JSON object you were given, outside the worktree, and run the command again."),
    ("Jev connection failed", RETRY),
    ("Jev retry budget exhausted", RETRY),
)


def suggested_action(exc):
    if exc.message.startswith("Request exceeds"):
        return (f"The request is {exc.size} bytes. Reduce context and retry: remove entries from the assignment's "
                "`context` list, largest file first, and rerun `--assignment <file>` until it fits. Never remove the "
                "deliverable text or `changed_files`. If it still exceeds with `context` empty, report this error unchanged.")
    if exc.message.startswith("Jev HTTP"):
        return RETRY if exc.recoverable else "Jev rejected the request or the key. " + REPORT
    return next((fix for prefix, fix in FIXES if exc.message.startswith(prefix)), REPORT)


def failure(exc):
    return {"success": False, "data": None, "error": {
        "code": exc.code, "message": exc.message, "recoverable": exc.recoverable,
        "suggested_action": suggested_action(exc),
    }}


def api_key():
    key = os.environ.get("TYPESAFE_API_KEY", "").strip()
    if not key:
        raise JevError("SANITY.JEV_UNAVAILABLE", "TYPESAFE_API_KEY is missing; no Jev evaluation ran", True)
    if not key.isascii() or any(ord(c) <= 32 or ord(c) == 127 for c in key):
        raise JevError("SANITY.JEV_UNAVAILABLE", "TYPESAFE_API_KEY has invalid formatting")
    return key


def probability(value):
    return type(value) in (int, float) and math.isfinite(value) and 0 <= value <= 1


def validate_response(value, request):
    if not isinstance(value, dict) or value.get("model") != MODEL:
        raise JevError("SANITY.JEV_RESPONSE_INVALID", "Unexpected Jev response model or shape")
    answers = value.get("answers")
    if not isinstance(answers, dict) or set(answers) != set(request["questions"]):
        raise JevError("SANITY.JEV_RESPONSE_INVALID", "Missing or unexpected answer IDs")
    for name, question in request["questions"].items():
        answer = answers[name]
        options = set(question["criteria"])
        if not isinstance(answer, dict):
            raise JevError("SANITY.JEV_RESPONSE_INVALID", "Answer must be an object")
        probs = answer.get("probabilities")
        if (answer.get("type") != "choice" or not isinstance(answer.get("choice"), str)
                or answer.get("choice") not in options
                or not probability(answer.get("confidence"))
                or not isinstance(probs, dict) or set(probs) != options
                or not all(probability(p) for p in probs.values())
                or abs(sum(probs.values()) - 1) > 0.001):
            raise JevError("SANITY.JEV_RESPONSE_INVALID", "Invalid Choice answer")
    return value


def validate_request(request):
    if (not isinstance(request, dict) or request.get("model") != MODEL
            or not isinstance(request.get("state"), (str, dict, list))
            or not isinstance(request.get("questions"), dict) or not request["questions"]):
        raise JevError("VALIDATION.INPUT", "Expected pinned model, state and nonempty questions")
    for name, q in request["questions"].items():
        if (not isinstance(name, str) or not name or not isinstance(q, dict)
                or q.get("type") != "choice" or not isinstance(q.get("instructions"), (str, dict, list))
                or not isinstance(q.get("criteria"), dict) or not 2 <= len(q["criteria"]) <= 255
                or not all(isinstance(k, str) and k for k in q["criteria"])
                or not all(v is None or isinstance(v, (str, dict, list)) for v in q["criteria"].values())):
            raise JevError("VALIDATION.INPUT", "Pilot supports well-formed Choice questions only")
    try:
        body = json.dumps(request, allow_nan=False).encode("utf-8")
    except (TypeError, ValueError):
        raise JevError("VALIDATION.INPUT", "Request must contain finite JSON values") from None
    if len(body) > MAX_REQUEST_BYTES:
        exc = JevError("SANITY.JEV_INCONCLUSIVE", "Request exceeds 96000 bytes, the Jev state budget; no Jev evaluation ran", True)
        exc.size = len(body)
        raise exc
    return body


def evaluate(request):
    key = api_key()
    body = validate_request(request)
    for attempt in range(2):
        conn = http.client.HTTPSConnection(HOST, timeout=20)
        try:
            conn.request("POST", "/v1/systemone", body=body, headers={
                "Authorization": "Bearer " + key, "Content-Type": "application/json",
            })
            response = conn.getresponse()
            status = response.status
            raw = response.read(MAX_RESPONSE_BYTES + 1)
            retry_after = response.getheader("Retry-After")
        except (OSError, socket.timeout, http.client.HTTPException):
            raise JevError("SANITY.JEV_UNAVAILABLE", "Jev connection failed or timed out", True) from None
        finally:
            conn.close()
        if status in (429, 529) and attempt == 0:
            try:
                delay = float(retry_after) if retry_after is not None else 1.0
            except ValueError:
                delay = float("inf")  # Date/unknown format: do not retry too soon.
            if math.isfinite(delay) and 0 <= delay <= 5:
                time.sleep(delay)
                continue
        if status != 200:
            raise JevError("SANITY.JEV_UNAVAILABLE", f"Jev HTTP {status}; response body withheld", status in (429, 529) or status >= 500)
        if len(raw) > MAX_RESPONSE_BYTES:
            raise JevError("SANITY.JEV_RESPONSE_INVALID", "Jev response exceeded size limit")
        try:
            value = json.loads(raw)
        except (ValueError, UnicodeError):
            raise JevError("SANITY.JEV_RESPONSE_INVALID", "Jev response was not JSON") from None
        return validate_response(value, request)
    raise JevError("SANITY.JEV_UNAVAILABLE", "Jev retry budget exhausted", True)


# Every failure message this client writes. A Jev failure envelope must carry one verbatim (sanity-merge checks it).
CLIENT_MESSAGES = re.compile("|".join([
    r"TYPESAFE_API_KEY is missing; no Jev evaluation ran", r"TYPESAFE_API_KEY has invalid formatting",
    r"Unexpected Jev response model or shape", r"Missing or unexpected answer IDs", r"Answer must be an object",
    r"Invalid Choice answer", r"Expected pinned model, state and nonempty questions",
    r"Pilot supports well-formed Choice questions only", r"Request must contain finite JSON values",
    r"Request exceeds 96000 bytes, the Jev state budget; no Jev evaluation ran",
    r"Jev connection failed or timed out", r"Jev HTTP \d{3}; response body withheld",
    r"Jev response exceeded size limit", r"Jev response was not JSON", r"Jev retry budget exhausted",
    r"Startup probe returned the wrong literal choice", r"Request file unavailable or invalid JSON",
    r"Assignment lacks deliverable text, worktree, commits or file lists",
    r"Committed evidence unreadable at the pinned commits",
]))
RECEIPT_KEYS = ("model", "question", "choice", "probabilities", "response_id", "request_sha256", "mac")


def is_client_message(message):
    return isinstance(message, str) and CLIENT_MESSAGES.fullmatch(message) is not None


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")


def receipt_mac(body, key):
    return hmac.new(key.encode("utf-8"), canonical({k: body[k] for k in RECEIPT_KEYS if k != "mac"}), hashlib.sha256).hexdigest()


def receipt(request, response, key):
    """What a caller copies to show this call happened: the one question's answer, bound to the request and keyed with
    the API key, so it cannot be written without the call."""
    (name,) = request["questions"]
    answer = response["answers"][name]
    body = {"model": response["model"], "question": name, "choice": answer["choice"],
            "probabilities": answer["probabilities"],
            "response_id": response.get("id") if isinstance(response.get("id"), str) else None,
            "request_sha256": hashlib.sha256(canonical(request)).hexdigest()}
    return {**body, "mac": receipt_mac({**body, "mac": None}, key)}


def verify_receipt(value, key):
    return (isinstance(value, dict) and set(value) == set(RECEIPT_KEYS) and isinstance(value["mac"], str) and bool(key)
            and hmac.compare_digest(value["mac"], receipt_mac(value, key)))


def startup_request():
    # Synthetic state only: no repository source is sent during startup.
    return {"model": MODEL, "state": {"marker": "ready"}, "questions": {
        "startup": {"type": "choice", "instructions": "Which literal value is in state.marker?",
                    "criteria": {"ready": "marker equals ready", "other": "marker differs"}},
    }}


def assignment_request(assignment):
    """The sanity request, built from the assignment so no agent writes it: the deliverable text verbatim,
    the committed diff of the changed files and the context files at the pinned commit."""
    try:
        text, worktree = assignment["deliverable"]["text"], assignment["worktree_path"]
        commit, base, changed, context = (assignment[k] for k in ("commit", "base_sha", "changed_files", "context"))
        if not all(isinstance(v, str) and v for v in (text, worktree, commit, base)) or not all(
                isinstance(v, list) and all(isinstance(p, str) and p for p in v) for v in (changed, context)):
            raise TypeError
    except (KeyError, TypeError):
        raise JevError("VALIDATION.INPUT", "Assignment lacks deliverable text, worktree, commits or file lists") from None

    def git(*args):
        proc = subprocess.run(["git", "-C", worktree, *args], capture_output=True, text=True)
        if proc.returncode:
            raise JevError("VALIDATION.INPUT", "Committed evidence unreadable at the pinned commits")
        return proc.stdout

    state = {"deliverable": text, "evidence": git("diff", "--no-color", f"{base}...{commit}", "--", *changed) if changed else ""}
    if context:
        state["context"] = {path: git("show", f"{commit}:{path}") for path in context}
    return {"model": MODEL, "state": state, "questions": {"written": {
        "type": "choice", "instructions": "Does the committed evidence deliver the deliverable?",
        "criteria": {"yes": "delivered", "no": "not delivered"}}}}


def escalation_recipients():
    """ATM's escalation recipients: the team's (`ATM_TEAM`), else the daemon default; [] when neither is set or readable."""
    team = os.environ.get("ATM_TEAM", "")
    for argv in ([["atm", "escalation", "list", "--team", team, "--json"]] if team else []) + [["atm", "escalation", "list", "--json"]]:
        try:
            listed = subprocess.run(argv, text=True, capture_output=True, timeout=15)
            value = json.loads(listed.stdout) if listed.returncode == 0 else {}
        except (OSError, subprocess.TimeoutExpired, ValueError):
            value = {}
        found = [r for r in (value.get("recipients") or []) if isinstance(r, str) and r] if isinstance(value, dict) else []
        if found:
            return found
    return []


def announce(message, lead):
    """Send `message` to ATM's escalation recipients, else to `lead` saying none is set."""
    recipients = escalation_recipients()
    if not recipients:
        recipients, message = [lead], message + " No escalation recipient is set."
    for recipient in recipients:
        try:
            sent = subprocess.run(["atm", "send", recipient, "--stdin"], input=message,
                                  text=True, capture_output=True, timeout=15)
            if sent.returncode:
                print(f"Announcement to {recipient} failed; coordinator must report the error via ATM.", file=sys.stderr)
        except (OSError, subprocess.TimeoutExpired):
            print(f"Announcement to {recipient} unavailable; coordinator must report the error via ATM.", file=sys.stderr)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--startup", action="store_true")
    mode.add_argument("--request", type=Path)
    mode.add_argument("--assignment", type=Path, help="a sanity assignment (fenced JSON body); the client builds the request")
    mode.add_argument("--error", help="with --announce: announce this JEV child error verbatim; no probe runs")
    parser.add_argument("--announce", action="store_true",
                        help="with --startup or --error: send a failure to ATM's escalation recipients (else --lead)")
    parser.add_argument("--lead", help="ATM lead identity; required with --announce, the fallback recipient")
    args = parser.parse_args(argv)
    if args.announce and (args.request or args.assignment or not args.lead or args.lead.startswith("-")):
        parser.error("--announce requires --startup or --error and an explicit --lead identity")
    if args.error is not None and (not args.announce or not args.error.strip()):
        parser.error("--error requires --announce and a nonempty error")
    if args.error is not None:
        announce(f"dev-sanity Jev child failed: {args.error}. Selection takes the LLM reply for JEV slots until a Jev child succeeds.", args.lead)
        print(json.dumps({"success": True, "data": {"announced": args.error}, "error": None}))
        return 0
    try:
        if args.startup:
            request = startup_request()
        elif args.assignment:
            request = assignment_request(json.loads(args.assignment.read_text()))
        else:
            request = json.loads(args.request.read_text())
        data = evaluate(request)
        if not args.startup and len(request["questions"]) == 1:
            data = {**data, "receipt": receipt(request, data, api_key())}
        if args.startup:
            if data["answers"]["startup"]["choice"] != "ready":
                raise JevError("SANITY.JEV_RESPONSE_INVALID", "Startup probe returned the wrong literal choice")
            data = {"model": MODEL, "status": "ready", "scope": "authenticated synthetic probe only; sanity accuracy unverified"}
        result = {"success": True, "data": data, "error": None}
    except JevError as exc:
        result = failure(exc)
    except (OSError, ValueError, UnicodeError):
        result = failure(JevError("VALIDATION.INPUT", "Request file unavailable or invalid JSON"))
    if args.announce and not result["success"]:
        error = result["error"]
        announce(f"dev-sanity Jev startup failed: {error['code']}: {error['message']}. No sc-sanity-jev checks run; dev-sanity records every JEV slot as unavailable until a startup probe passes.", args.lead)
    print(json.dumps(result, allow_nan=False))
    return 0 if result["success"] else 2


if __name__ == "__main__":
    sys.exit(main())
