#!/usr/bin/env python3
"""Bounded Jev transport and startup probe for the proposed sanity agent."""
import argparse
import http.client
import json
import math
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

MODEL = "jev-1.13.0"
HOST = "api.typesafe.ai"
MAX_REQUEST_BYTES = 24000  # Conservative pilot bound, not a vendor token count.
MAX_RESPONSE_BYTES = 1048576


class JevError(Exception):
    def __init__(self, code, message, recoverable=False):
        super().__init__(message)
        self.code, self.message, self.recoverable = code, message, recoverable


def failure(exc):
    return {"success": False, "data": None, "error": {
        "code": exc.code, "message": exc.message, "recoverable": exc.recoverable,
        "suggested_action": "set TYPESAFE_API_KEY or keep dev-sanity-llm" if "TYPESAFE_API_KEY is missing" in exc.message else "Keep the LLM directive active; correct Jev access or request data and rerun the startup probe",
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
        raise JevError("SANITY.JEV_INCONCLUSIVE", "Pilot request exceeds 24000 bytes; split evidence without dropping checks")
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


def startup_request():
    # Synthetic state only: no repository source is sent during startup.
    return {"model": MODEL, "state": {"marker": "ready"}, "questions": {
        "startup": {"type": "choice", "instructions": "Which literal value is in state.marker?",
                    "criteria": {"ready": "marker equals ready", "other": "marker differs"}},
    }}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--startup", action="store_true")
    mode.add_argument("--request", type=Path)
    parser.add_argument("--lead", help="ATM lead identity; required with --startup")
    args = parser.parse_args(argv)
    if args.startup and (not args.lead or args.lead.startswith("-")):
        parser.error("--startup requires an explicit --lead identity")
    try:
        request = startup_request() if args.startup else json.loads(args.request.read_text())
        data = evaluate(request)
        if args.startup:
            if data["answers"]["startup"]["choice"] != "ready":
                raise JevError("SANITY.JEV_RESPONSE_INVALID", "Startup probe returned the wrong literal choice")
            data = {"model": MODEL, "status": "ready", "scope": "authenticated synthetic probe only; sanity accuracy unverified"}
        result = {"success": True, "data": data, "error": None}
    except JevError as exc:
        result = failure(exc)
    except (OSError, ValueError, UnicodeError):
        result = failure(JevError("VALIDATION.INPUT", "Request file unavailable or invalid JSON"))
    if args.startup and not result["success"]:
        error = result["error"]
        message = f"dev-sanity Jev startup failed: {error['code']}: {error['message']}. No checks accepted; keep dev-sanity-llm active."
        try:
            sent = subprocess.run(["atm", "send", args.lead, "--stdin"], input=message,
                                  text=True, capture_output=True, timeout=15)
            if sent.returncode:
                print("Lead notification failed; coordinator must report the startup error via ATM.", file=sys.stderr)
        except (OSError, subprocess.TimeoutExpired):
            print("Lead notification unavailable; coordinator must report the startup error via ATM.", file=sys.stderr)
    print(json.dumps(result, allow_nan=False))
    return 0 if result["success"] else 2


if __name__ == "__main__":
    sys.exit(main())
