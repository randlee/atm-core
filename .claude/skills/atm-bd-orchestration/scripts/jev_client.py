#!/usr/bin/env python3
"""Bounded Jev transport and startup probe for the proposed sanity agent."""
import argparse
import copy
from email.utils import parsedate_to_datetime
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

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads/scripts"))
import repo_config
from sprint_index_common import load_phase_config

MODEL = "jev-1.13.0"
HOST = "api.typesafe.ai"
MAX_REQUEST_BYTES = 96000  # Jev documents 32k state tokens of a 64k request budget; ~3 bytes per token of code.
MAX_RESPONSE_BYTES = 1048576
# Lead ruling 2026-10-19: four attempts per part; retries and 413 shortenings share that part's budget.
MAX_JEV_ATTEMPTS = 4


class AttemptBudget:
    def __init__(self, remaining=MAX_JEV_ATTEMPTS):
        self.remaining = min(remaining, MAX_JEV_ATTEMPTS)


class JevError(Exception):
    def __init__(self, code, message, recoverable=False):
        super().__init__(message)
        self.code, self.message, self.recoverable = code, message, recoverable
        self.size: int | None = None
        self.fix: str | None = None


REPORT = "Report this error unchanged; the coordinator records the slot unavailable."
RETRY = "The client retry budget is exhausted. " + REPORT
REBUILD = ("The client owns lossless splitting of evidence, context and inline notes. "
           "Its bounded split/retry budget is exhausted; do not hand-trim the assignment. " + REPORT)
CONTEXT_RANGE = re.compile(r"(?P<path>.+):(?P<start>[0-9]+)-(?P<end>[0-9]+)")  # a `context` entry naming lines
FENCED = re.compile(r"\s*```(?:json)?[ \t]*\n(.*?)\n```\s*", re.S)  # an assignment as received: fenced JSON
FIXES = (  # message prefix -> what the calling agent does next
    ("TYPESAFE_API_KEY is missing", "No Jev call is possible without TYPESAFE_API_KEY. " + REPORT),
    ("TYPESAFE_API_KEY has invalid formatting", "TYPESAFE_API_KEY holds whitespace, quotes or control characters. " + REPORT),
    ("Assignment lacks", "Write the assignment you received unchanged: it needs `deliverable.text`, `worktree_path`, "
     "`commit`, `base_sha`, `changed_files` and `context`. If the received assignment lacks one, " + REPORT[0].lower() + REPORT[1:]),
    ("Committed evidence unreadable", "Run `git -C <worktree_path> fetch origin` so `commit` and `base_sha` exist, then run "
     "the same command again. If it still fails, a `changed_files` or `context` path is missing at `commit`: " + REPORT[0].lower() + REPORT[1:]),
    ("Assignment external_evidence", "Each `external_evidence` entry is one of {\"kind\": \"bead_notes\", \"bead\": <id>, \"text\": <notes>}, "
     "{\"kind\": \"pr_body\", \"repo\": \"owner/name\", \"pr\": <n>} or {\"kind\": \"pr_comment\", \"repo\": "
     "\"owner/name\", \"comment_id\": <n>}. If the received assignment holds another shape, " + REPORT[0].lower() + REPORT[1:]),
    ("External evidence reference is invalid", "Use inline `bead_notes` with `kind`, `bead` and string `text`, or a valid `pr_body`/`pr_comment` reference. "
     + REPORT),
    ("External evidence unreadable", "Run `gh api` for the PR or comment, fix what it reports (`gh auth status`), "
     "then run the same command again. If it still fails, "
     + REPORT[0].lower() + REPORT[1:]),
    ("Context entry", "Fix that `context` entry: `path:start-end` names lines 1 <= start <= end <= the file's line "
     "count at `commit`; a plain `path` sends the whole file. Then run the same command again."),
    ("Request file unavailable", "Write the file as the JSON object you were given, outside the worktree, and run the command again."),
    ("Jev connection failed", RETRY),
    ("Jev retry budget exhausted", RETRY),
)


def suggested_action(exc):
    if exc.message.startswith("Jev request oversized"):
        return getattr(exc, "fix", None) or REPORT
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
        raise oversize(request)
    return body


def retry_delay(retry_after):
    if retry_after is None:
        return 1.0
    try:
        delay = float(retry_after)
    except (TypeError, ValueError):
        try:
            delay = parsedate_to_datetime(retry_after).timestamp() - time.time()
        except (TypeError, ValueError, OverflowError):
            return 30.0
    return min(30.0, max(0.0, delay)) if math.isfinite(delay) else 30.0


def evaluate(request, *, sleep=None, max_attempts=MAX_JEV_ATTEMPTS, _budget=None):
    key = api_key()
    body = validate_request(request)
    sleeper = sleep or time.sleep
    budget = _budget if _budget is not None else AttemptBudget(max_attempts)
    while budget.remaining > 0:
        budget.remaining -= 1
        conn = None
        try:
            conn = http.client.HTTPSConnection(HOST, timeout=20)
            conn.request("POST", "/v1/systemone", body=body, headers={
                "Authorization": "Bearer " + key, "Content-Type": "application/json",
            })
            response = conn.getresponse()
            status = response.status
            raw = response.read(MAX_RESPONSE_BYTES + 1)
            retry_after = response.getheader("Retry-After")
        except (OSError, socket.timeout, http.client.HTTPException):
            if budget.remaining > 0:
                continue
            raise JevError("SANITY.JEV_UNAVAILABLE", "Jev connection failed or timed out", True) from None
        finally:
            if conn is not None:
                conn.close()
        if status == 413:
            raise oversize(request)
        if status in (429, 502, 529) and budget.remaining > 0:
            delay = retry_delay(retry_after)
            if delay:
                sleeper(delay)
            continue
        if status != 200:
            raise JevError("SANITY.JEV_UNAVAILABLE", f"Jev HTTP {status}; response body withheld", status in (429, 502, 529))
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
    r"Jev request oversized; no further split fits the remaining attempt budget",
    r"Jev connection failed or timed out", r"Jev HTTP \d{3}; response body withheld",
    r"Jev response exceeded size limit", r"Jev response was not JSON", r"Jev retry budget exhausted",
    r"Startup probe returned the wrong literal choice", r"Request file unavailable or invalid JSON",
    r"Assignment lacks deliverable text, worktree, commits or file lists",
    r"Committed evidence unreadable at the pinned commits",
    r"Assignment external_evidence entry is not a bead_notes, pr_body or pr_comment reference",
    r"External evidence reference is invalid",
    r"External evidence unreadable: \S+ \S+",
    r"Context entry .+ names lines outside its file at the pinned commit",
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


def written_request(text, evidence, context=None, part=None, external=None):
    state = {"deliverable": text, "evidence": evidence}
    if context:
        state["context"] = context
    if external:
        state["external_evidence"] = external
    instructions = "Does the committed evidence deliver the deliverable?"
    if part:
        state["part"], state["changed_files"] = part
        instructions = ("The evidence is one part of the committed diff of `changed_files`, split to fit the request "
                        "budget. Does this part deliver its share of the deliverable?")
    return {"model": MODEL, "state": state, "questions": {"written": {
        "type": "choice", "instructions": instructions, "criteria": {"yes": "delivered", "no": "not delivered"}}}}


def size(request):
    return len(json.dumps(request, allow_nan=False).encode("utf-8"))


def oversize(request, fix=None):
    exc = JevError("SANITY.JEV_REQUEST_TOO_LARGE", "Jev request oversized; no further split fits the remaining attempt budget", True)
    exc.size = size(request)
    exc.fix = fix and fix.format(size=exc.size)
    return exc


def diff_pieces(diff, budget):
    """`diff` (one file's) as pieces each under `budget` encoded bytes: the file header with one hunk, a hunk too large
    for that cut by lines under its header and @@ line, and a line too large for that cut by characters."""
    def cost(text):
        return len(json.dumps(text).encode("utf-8")) - 2
    head, _, rest = diff.partition("\n@@")
    if not rest:
        head, rest = "", diff
    else:
        head, rest = head + "\n", "@@" + rest
    hunks = [h if i == 0 else "@@" + h for i, h in enumerate(rest.split("\n@@"))] if head else [rest]
    hunks = [h if h.endswith("\n") or i == len(hunks) - 1 else h + "\n" for i, h in enumerate(hunks)]
    pieces = []
    for hunk in hunks:
        if cost(head + hunk) <= budget:
            pieces.append(head + hunk)
            continue
        at, _, body = hunk.partition("\n")
        prefix = head + at + "\n"
        room = budget - cost(prefix)
        lines = []
        for line in body.splitlines(keepends=True):
            step = max(1, room // 6)  # a character encodes to at most 6 JSON bytes
            lines += [line] if cost(line) <= room else [line[i:i + step] for i in range(0, len(line), step)]
        chunk = ""
        for line in lines:
            if chunk and cost(chunk + line) > room:
                pieces.append(prefix + chunk)
                chunk = ""
            chunk += line
        if chunk:
            pieces.append(prefix + chunk)
    return pieces


def split_request(request, target, changed_files):
    """Split evidence, context and external text into labelled requests without dropping content."""
    state = request.get("state")
    if not isinstance(state, dict) or not isinstance(state.get("deliverable"), str):
        raise oversize(request)
    if (not isinstance(state.get("evidence", ""), str)
            or not isinstance(state.get("context", {}), dict)
            or not all(isinstance(key, str) and isinstance(value, str)
                       for key, value in state.get("context", {}).items())
            or not isinstance(state.get("external_evidence", []), list)
            or not all(isinstance(entry, dict) and isinstance(entry.get("text", ""), str)
                       for entry in state.get("external_evidence", []))):
        raise oversize(request)
    base = copy.deepcopy(request)
    base["state"] = {k: v for k, v in state.items() if k not in ("evidence", "context", "external_evidence", "part", "changed_files")}
    base["state"].update(evidence="", part="999999 of 999999", changed_files=changed_files)
    if size(base) >= target:
        raise oversize(request)
    pieces = []

    def chunks(value, budget, is_diff=False):
        if budget < 8:
            raise oversize(request)
        if not value:
            return [""]
        if is_diff:
            proposed = diff_pieces(value, budget)
            if proposed and all(len(json.dumps(p).encode()) - 2 <= budget for p in proposed):
                return proposed
        # JSON escaping can expand a character; binary search each maximal cut.
        result = []
        while value:
            low, high = 1, len(value)
            while low <= high:
                middle = (low + high) // 2
                if len(json.dumps(value[:middle]).encode()) - 2 <= budget:
                    low = middle + 1
                else:
                    high = middle - 1
            if not high:
                raise oversize(request)
            result.append(value[:high])
            value = value[high:]
        return result

    def add_text(field, value, key=None, entry=None):
        sample = copy.deepcopy(base)
        if field == "context":
            sample["state"][field] = {key: ""}
        elif field == "external_evidence":
            if entry is None:
                raise ValueError("external evidence entry is required")
            sample["state"][field] = [{**entry, "text": ""}]
        budget = target - size(sample) - 16
        for piece in chunks(value, budget, is_diff=field == "evidence"):
            part = copy.deepcopy(sample)
            if field == "context":
                part["state"][field][key] = piece
            elif field == "external_evidence":
                part["state"][field][0]["text"] = piece
            else:
                part["state"][field] = piece
            if size(part) > target:
                raise oversize(request)
            pieces.append(part)

    evidence = state.get("evidence") or ""
    if evidence:
        add_text("evidence", evidence)
    for key, value in (state.get("context") or {}).items():
        add_text("context", value, key=key)
    for entry in state.get("external_evidence") or []:
        add_text("external_evidence", entry.get("text", ""), entry=entry)
    if not pieces:
        raise oversize(request)
    for index, part in enumerate(pieces, 1):
        part["state"]["part"] = f"{index} of {len(pieces)}"
    return pieces


def evaluate_parts(requests, key, changed_files=None, *, sleep=None):
    """Initial parts have independent budgets; a 413's smaller children share their parent's remainder."""
    queue = []
    for request in requests:
        try:
            validate_request(request)
            initial = [request]
        except JevError as exc:
            if exc.code != "SANITY.JEV_REQUEST_TOO_LARGE":
                raise
            state = request["state"]
            files = state.get("changed_files", changed_files or []) if isinstance(state, dict) else changed_files or []
            initial = split_request(request, MAX_REQUEST_BYTES, files)
        queue.extend((part, AttemptBudget(MAX_JEV_ATTEMPTS)) for part in initial)
    done = []
    while queue:
        request, budget = queue.pop(0)
        try:
            response = evaluate(request, sleep=sleep, _budget=budget)
        except JevError as exc:
            if exc.code != "SANITY.JEV_REQUEST_TOO_LARGE" or budget.remaining <= 0:
                raise
            files = request.get("state", {}).get("changed_files", changed_files or [])
            smaller = split_request(request, size(request) // 2, files)
            if len(smaller) < 2:
                raise exc
            queue[0:0] = [(part, budget) for part in smaller]
            continue
        done.append((request, response))
    if len(done) == 1 and "part" not in done[0][0].get("state", {}):
        request, response = done[0]
        return {**response, "receipt": receipt(request, response, key)}
    return {"model": MODEL, "parts": [
        {"part": f"{index} of {len(done)}", "changed_files": request["state"]["changed_files"],
         "response": response, "receipt": receipt(request, response, key)}
        for index, (request, response) in enumerate(done, 1)]}


EVIDENCE_REFS = {"pr_body": ("repo", "pr"), "pr_comment": ("repo", "comment_id")}


def evidence_ref(ref):
    """True for one well-formed `external_evidence` reference."""
    if isinstance(ref, dict) and ref.get("kind") == "bead_notes" and set(ref) == {"kind", "bead", "text"}:
        return isinstance(ref["bead"], str) and bool(ref["bead"]) and not ref["bead"].startswith("-") and isinstance(ref["text"], str)
    if isinstance(ref, dict) and ref.get("kind") == "bead_notes":
        return False
    kind = ref.get("kind") if isinstance(ref, dict) else None
    keys = EVIDENCE_REFS.get(kind) if isinstance(kind, str) else None
    return keys is not None and set(ref) == {"kind", *keys} and all(
        type(ref[k]) is int and ref[k] > 0 if k in ("pr", "comment_id")
        else isinstance(ref[k], str) and ref[k] and not ref[k].startswith("-") for k in keys)


def external_evidence(ref, worktree):
    """Inline bead notes or a PR body/comment fetched here, labelled with where it came from."""
    kind = ref["kind"]
    if kind == "bead_notes" and evidence_ref(ref):
        label = ref["bead"]
        return {"kind": kind, "ref": label, "url": None, "updated_at": None, "author": None, "text": ref["text"]}
    if kind not in EVIDENCE_REFS or not evidence_ref(ref):
        raise JevError("VALIDATION.INPUT", "External evidence reference is invalid")
    label = f"{ref['repo']}#{ref.get('pr', ref.get('comment_id'))}"
    argv = (["gh", "api", f"repos/{ref['repo']}/pulls/{ref['pr']}"] if kind == "pr_body" else
            ["gh", "api", f"repos/{ref['repo']}/issues/comments/{ref['comment_id']}"])
    try:
        proc = subprocess.run(argv, cwd=worktree, capture_output=True, text=True, timeout=60)
        value = json.loads(proc.stdout) if proc.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired, ValueError):
        value = None
    if not isinstance(value, dict) or not isinstance(value.get("body"), str):
        raise JevError("VALIDATION.INPUT", f"External evidence unreadable: {kind} {label}")
    text = value["body"]
    user = value.get("user")
    author = user.get("login") if isinstance(user, dict) else None
    return {"kind": kind, "ref": label, "url": value.get("html_url"), "updated_at": value.get("updated_at"),
            "author": author, "text": text}


def assignment_requests(assignment):
    """The sanity requests, built from the assignment so no agent writes them: the deliverable text verbatim, the
    committed diff of the changed files and the context entries at the pinned commit (a `path:start-end` entry only
    those lines, labelled with the entry). One request when it fits under
    MAX_REQUEST_BYTES; each `external_evidence` reference (bead notes, PR body, PR comment) fetched with its source.
    With `context` and `external_evidence` empty and the diff over the cap, `changed_files` split into groups (a file whose
    diff alone is over the cap split by hunks) whose requests each fit, one Jev call per part."""
    try:
        text, worktree = assignment["deliverable"]["text"], assignment["worktree_path"]
        commit, base, changed, context = (assignment[k] for k in ("commit", "base_sha", "changed_files", "context"))
        if not all(isinstance(v, str) and v for v in (text, worktree, commit, base)) or not all(
                isinstance(v, list) and all(isinstance(p, str) and p for p in v) for v in (changed, context)):
            raise TypeError
    except (KeyError, TypeError):
        raise JevError("VALIDATION.INPUT", "Assignment lacks deliverable text, worktree, commits or file lists") from None
    refs = assignment.get("external_evidence", [])
    if not isinstance(refs, list) or not all(evidence_ref(ref) for ref in refs):
        raise JevError("VALIDATION.INPUT", "Assignment external_evidence entry is not a bead_notes, pr_body or pr_comment reference")

    def git(*args):
        proc = subprocess.run(["git", "-C", worktree, *args], capture_output=True, text=True)
        if proc.returncode:
            raise JevError("VALIDATION.INPUT", "Committed evidence unreadable at the pinned commits")
        return proc.stdout

    def diff(*paths):
        return git("diff", "--no-color", f"{base}...{commit}", "--", *paths) if paths else ""

    def excerpt(entry):
        """A plain path's whole file at `commit`, or only lines start..end (1-based, inclusive) of `path:start-end`."""
        lines = CONTEXT_RANGE.fullmatch(entry)
        if not lines:
            return git("show", f"{commit}:{entry}")
        text, start, end = git("show", f"{commit}:{lines['path']}"), int(lines["start"]), int(lines["end"])
        kept = text.splitlines(keepends=True)
        if not 1 <= start <= end <= len(kept):
            raise JevError("VALIDATION.INPUT", f"Context entry {entry} names lines outside its file at the pinned commit")
        return "".join(kept[start - 1:end])

    external = [external_evidence(ref, worktree) for ref in refs]
    request = written_request(text, diff(*changed), {entry: excerpt(entry) for entry in context}, external=external)
    if size(request) <= MAX_REQUEST_BYTES:
        return [request]
    if context or external:
        return split_request(request, MAX_REQUEST_BYTES, changed)
    label = (f"{len(changed) * 1000} of {len(changed) * 1000}", changed)  # the widest part label, so every part fits
    budget = MAX_REQUEST_BYTES - size(written_request(text, "", part=label))
    if budget <= 0:
        raise oversize(request, "The deliverable text alone makes the request {size} bytes, over the 96000-byte Jev "
                       "budget, so no split of `changed_files` fits. Tell the task assigner this deliverable's text "
                       "must be split into smaller numbered deliverables in the plan.")
    groups, evidence = [], ""
    for piece in (p for path in changed for p in diff_pieces(diff(path), budget)):
        if evidence and size(written_request(text, evidence + piece, part=label)) > MAX_REQUEST_BYTES:
            groups.append(evidence)
            evidence = ""
        evidence += piece
    groups.append(evidence)
    return [written_request(text, part, part=(f"{i} of {len(groups)}", changed)) for i, part in enumerate(groups, 1)]


def parts(requests, responses, key):
    """Every part's full result, in order, with its own receipt: no part is reduced into another. Reconciling parts
    that disagree is the sanity teammate's triage, never this client's."""
    return {"model": MODEL, "parts": [
        {"part": request["state"]["part"], "changed_files": request["state"]["changed_files"],
         "response": response, "receipt": receipt(request, response, key)}
        for request, response in zip(requests, responses)]}


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


def evaluation_result(args, *, clock=time.monotonic, sleep=None):
    elapsed_s = None
    requests = []
    assignment = None
    try:
        if args.startup:
            request = startup_request()
        elif args.assignment:
            text = args.assignment.read_text()
            fenced = FENCED.fullmatch(text)
            assignment = json.loads(fenced.group(1) if fenced else text)
            requests = assignment_requests(assignment)
            request = requests[0]
        else:
            request = json.loads(args.request.read_text())
        started = clock()
        try:
            if args.assignment:
                assert isinstance(assignment, dict)
                data = evaluate_parts(requests, api_key(), assignment["changed_files"], sleep=sleep)
            elif args.startup:
                data = evaluate(request, sleep=sleep)
            else:
                data = evaluate_parts([request], api_key(), sleep=sleep)
        finally:
            elapsed_s = max(0.0, clock() - started)
        if not args.startup and len(request["questions"]) == 1 and "parts" not in data and "receipt" not in data:
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
    result["elapsed_s"] = elapsed_s if elapsed_s is not None else 0.0
    return result


def main(argv=None, *, clock=time.monotonic, sleep=None):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--startup", action="store_true")
    mode.add_argument("--request", type=Path)
    mode.add_argument("--assignment", type=Path, help="a sanity assignment (fenced JSON body); the client builds the request")
    mode.add_argument("--error", help="with --announce: announce this JEV child error verbatim; no probe runs")
    parser.add_argument("--announce", action="store_true",
                        help="with --startup or --error: send a failure to ATM's escalation recipients (else the phase lead)")
    parser.add_argument("--phase", help="phase whose tracked settings name the lead; required with --announce")
    args = parser.parse_args(argv)
    if args.announce and (args.request or args.assignment or not args.phase):
        parser.error("--announce requires --startup or --error and an explicit --phase")
    if args.error is not None and (not args.announce or not args.error.strip()):
        parser.error("--error requires --announce and a nonempty error")
    phase_lead = None
    if args.phase:
        try:
            phase_lead = load_phase_config(repo_config.repo_root(), args.phase)["lead"]
        except (repo_config.ConfigError, RuntimeError, OSError, ValueError) as exc:
            parser.error(str(exc))
    if args.error is not None:
        announce(f"dev-sanity Jev child failed: {args.error}. Selection takes the LLM reply for JEV slots until a Jev child succeeds.", phase_lead)
        print(json.dumps({"success": True, "data": {"announced": args.error}, "error": None}))
        return 0
    result = evaluation_result(args, clock=clock, sleep=sleep)
    if args.announce and not result["success"]:
        error = result["error"]
        announce(f"dev-sanity Jev startup failed: {error['code']}: {error['message']}. No sc-sanity-jev checks run; dev-sanity records every JEV slot as unavailable until a startup probe passes.", phase_lead)
    print(json.dumps(result, allow_nan=False))
    return 0 if result["success"] else 2


if __name__ == "__main__":
    sys.exit(main())
