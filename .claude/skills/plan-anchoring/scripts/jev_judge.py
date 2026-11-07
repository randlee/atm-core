#!/usr/bin/env python3
"""jev_judge.py - the deterministic half of plan anchoring.

Everything that must not be left to a language model lives here: anchor ID
allocation, the testability lint and duplicate check on new anchors, the exact
Jev request, turning Jev's independent answers into one verdict per (anchor,
item) pair, coverage counting across items, run-to-run diffs, and the human
report that shows the anchor's testable statement next to the user's verbatim
quote.

An anchor has two texts. `statement` is the testable requirement written by the
orchestrator and approved by the user; it is what Jev judges. `quote` is the
user's exact words, kept verbatim as provenance and shown in reports; it is
required for ANC anchors and defaults to the statement for REQ/ADR anchors
(register text). A record whose anchor lacks `quote` is read as quote =
statement; `validate` reports it.

Standard library only. Python 3.9+. One exception: a live Jev call reads the
committed ``allow_remote_judge`` answer through ``plan_anchor_settings.py``
(PyYAML when the settings file exists), imported lazily inside that check, so
--dry-run, --answers-file and every offline subcommand stay stdlib-only.

Expected failures travel as values: a function that can fail returns
``Ok(value)`` or ``Err(JudgeError)`` (see ``result.py``) and its caller
propagates the ``Err`` explicitly. ``main`` is the only place an error becomes
JSON on stdout and an exit code; anything unexpected that still raises is
reported there as SCRIPT.INTERNAL.

Subcommands
  add             append an anchor record (allocates the next ANC-NNN); lints the statement and refuses likely duplicates (data.added)
  update          edit statement / scope / summary / deterministic_check / fixtures (never the quote; data.updated)
  retire          mark an anchor retired, optionally naming its replacement
  exclude         record (or --remove) that an anchor does not apply to one plan item; no version bump
  validate        check anchors.json for structural problems (plus non-fatal warnings); data.valid
  lint            testability warnings for one or every anchor statement; data.clean
  dedupe          token-Jaccard pairs of similar anchors, optionally confirmed by Jev; data.likely_duplicates
  rubric          judge anchor statements against the testability rubric (lint + Jev); data.pass
  render          write anchors.md from anchors.json
  set-beads       record the anchor / plan-questions / dev-questions bead IDs
  promote         mark an anchor as promoted to an ADR (keeps its ID)
  compose-item    build the labelled item text from `bd show --json` (and `bd children --json`)
  agent-preflight validate and canonicalise agent paths and identifiers before any read or upload
  shape           size and shape metrics of plan beads (sections, code vs prose, references, repeated paragraphs); never gates
  judge           ask Jev about one item against one or more anchors, write a run; data.summary.pass
  check-fixtures  judge each anchor's fixtures.pass / fixtures.fail sentences as items; data.pass and data.mismatches
  report          render a run as markdown in the provenance-quoting format; data.pass
  coverage        anchors x items verdict matrix over one or more runs; data.uncovered_anchors
  diff            what changed between two runs of the same gate and item; data.pass_after and data.regressions

Every subcommand prints JSON to stdout (or markdown with --format md where
supported).

Exit codes
  A tool failure is an exit code; a negative answer from a command that ran
  properly is not. There is no exit 1.
  0  the command ran: {"success": true, "data": {...}, "error": null}. `data`
     carries the outcome, negative ones included: a failing gate (`judge`
     data.summary.pass false), a fixture mismatch (`check-fixtures` data.pass
     false; data.judged 0 when nothing was judged), problems in the record
     (`validate` data.valid false), lint warnings (`lint` data.clean false),
     likely duplicates (`dedupe` data.likely_duplicates non-empty), a
     statement that is not testable (`rubric` data.pass false), an anchor
     `add` refused as a duplicate, a likely duplicate, the verbatim quote,
     untestable or lint-failing (data.added false with data.refused {code,
     message, recoverable, suggested_action}; `update --strict`: data.updated
     false), and a no-op `update`, `retire` of a retired anchor or
     `exclude --remove` with nothing to remove (data.changed false)
  2  the command could not do its job: {"success": false, "data": null|{...},
     "error": {code, message, recoverable, suggested_action}} (bad input or
     arguments as VALIDATION.INPUT, a missing or unreadable file, a malformed
     record or override, bad credentials, remote judging not allowed by
     allow_remote_judge in .sc/plan-anchor/settings.yaml as
     POLICY.REMOTE_JUDGE_DISABLED, a plan bead that does not fit one Jev
     request as PLAN.TOO_LARGE, a missing dependency, an internal error as
     SCRIPT.INTERNAL, and any Jev call that produced no answer: a Jev HTTP
     error, a network or transport failure, an invalid response, or a missing
     or malformed answers-file entry, in `judge`, `check-fixtures`,
     `dedupe --jev`, `rubric` and `add --rubric`. A failed call is never
     folded into a negative result: the error names the first failure, and
     data holds the partial work (`judge`: data.run_path, the run file written
     with the failing cells as verdict `error`; data.failures lists each one)
  130 interrupted: the error envelope with code CANCELED (recoverable true)

Plan-gate size handling
  The plan gate never shortens prose. A bead that fits (--max-item-chars, and
  --max-request-bytes for every anchor) is sent whole. Otherwise fenced code
  blocks longer than --max-code-lines are reduced to their signature lines plus
  a redaction marker, longest first and one at a time, until it fits; a bead
  that does not fit with all of them reduced is refused before anything is
  sent (PLAN.TOO_LARGE), and
  a context overflow from Jev fails the run as PLAN.TOO_LARGE (exit 2; the
  partial run file is kept). The
  development gate keeps the truncate-and-halve behaviour.
"""
from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import http.client
import json
import math
import os
import pathlib
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from typing import Any

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from result import Err, Ok, Result, utf8_stdio  # noqa: E402


JEV_URL = "https://api.typesafe.ai/v1/systemone"  # tests redirect by patching this constant in-process
TYPESAFE_KEY_ENV = "TYPESAFE_API_KEY"
CREDENTIAL_SERVICE = "plan-anchor"
CREDENTIAL_ACCOUNT = "typesafe-api-key"
WINDOWS_CREDENTIAL_TARGET = f"{CREDENTIAL_SERVICE}:{CREDENTIAL_ACCOUNT}"
DEFAULT_MODEL = "jev-latest"
DEFAULT_THRESHOLD = 0.6
DEFAULT_MAX_ITEM_CHARS = 24000
DEFAULT_MAX_CODE_LINES = 8  # plan gate: shortest fenced block that may go to Jev as signatures when a bead does not fit
CODE_REDACTION_MARKER = "<redacted: {dropped} of {total} code lines are in the plan but were not sent to Jev>"
DEFAULT_MAX_REQUEST_BYTES = 24000  # plan gate: what Jev reads reliably (a planted contradiction 28k deep scored 0.46 vs 0.79 near the top)
DEFAULT_MAX_REQUEST_BYTES_DEV = 64000  # dev gate: evidence is long; Jev accepted 107k bytes in one attempt, halving never fired in 245 requests
NEAR_THRESHOLD_BAND = 0.10         # a confidence this close to the threshold triggers one confirming re-judge (15/50 identical cells moved > 0.10)
DEFAULT_MIN_ITEM_CHARS = 2000      # below this the item is not shortened further on a context overflow
MAX_RESPONSE_BYTES = 1048576
MAX_ATTEMPTS = 3                   # per anchor, for transport-level failures (429, 529, 5xx, network, bad response)
RETRY_DELAYS = (1.0, 2.0)          # seconds before the second and third attempt; Retry-After overrides the first
MAX_RETRY_AFTER = 30.0
PROBABILITY_ROUNDING = 0.01        # Jev rounds each probability; the sum may be off by this per option
TRUNCATION_MARKER = "\n[... truncated by jev_judge; keep the state small ...]"
HERE = pathlib.Path(__file__).resolve().parent
QUESTIONS_DIR = HERE.parent / "assets" / "questions"
TRIAGE_ASSETS = HERE.parent / "assets" / "triage"
TRIAGE_RELDIR = ".sc/plan-anchor/triage"

SOURCE_TYPES = ("ANC", "REQ", "ADR")
REGISTER_NAMES = {"REQ": "requirements register", "ADR": "ADR register"}
STATUSES = ("proposed", "locked", "promoted", "retired")
JUDGED_STATUSES = ("locked", "promoted")
KINDS = ("directive", "prohibition", "structural")  # how decide() gates: see decide(); structural anchors skip Jev at the plan gate
VERDICTS = (
    "contradicted",
    "not_addressed",
    "asserted_only",
    "underspecified",
    "satisfied",
    "not_applicable",
    "deterministic_only",
    "excluded",          # the anchor's `exclude` list names this item: Jev is not asked, the cell passes
    "error",
)
FAILING = {"contradicted", "not_addressed", "asserted_only", "underspecified"}
EXCLUSION_KEYS = ("item", "reason")
SEVERITY = {v: i for i, v in enumerate(VERDICTS)}
REQUIRED_ROLES = ("relevance", "compliance", "conflict")
ABORT_CODES = ("AUTH.CREDENTIALS", "POLICY.REMOTE_JUDGE_DISABLED")
TRANSIENT_CODES = ("JEV.RATE_LIMITED", "JEV.OVERLOADED", "JEV.NETWORK", "JEV.RESPONSE_INVALID")
FIXTURE_EXPECTED = {"pass": ("satisfied",), "fail": ("not_addressed", "contradicted")}
ITEM_SECTIONS = (("description", "Description"), ("design", "Design"), ("acceptance_criteria", "Acceptance criteria"), ("notes", "Notes"))

# testability lint and duplicate detection (all deterministic; see lint_statement and jaccard)
MAX_STATEMENT_CHARS = 240
DUPLICATE_JACCARD = 0.6            # `add` refuses a new anchor this similar to an existing one unless --allow-similar
DEDUPE_DEFAULT_THRESHOLD = 0.5     # `dedupe` reports pairs at or above this token Jaccard
LIKELY_DUPLICATE_JACCARD = 0.8     # without Jev, `dedupe` calls a pair a likely duplicate at or above this
SIMILAR_REPORT_FLOOR = DUPLICATE_JACCARD / 2  # `add` lists candidates at or above this in data.similar
DEDUPE_QUESTION = "same_constraint"
REQUIREMENT_VERBS = ("must", "never", "always", "only", "shall", "is", "are", "requires", "uses", "does not")
HEDGES = ("should", "could", "maybe", "probably", "might", "perhaps", "ideally", "try to", "as needed", "if possible",
          "where possible", "when possible", "where appropriate", "as appropriate", "etc", "and so on")
SPEECH_PRONOUNS = ("i", "we", "you", "my", "our", "me", "us", "your")
VAGUE_TERMS = ("appropriate", "proper", "properly", "clean", "good", "nice", "reasonable", "robust", "simple")
STOPWORDS = frozenset("the a an and or of to in on for by from with as at be is are this that it its into".split())
LINT_VERBATIM = "statement is the verbatim quote"
LINT_VERBATIM_PROBLEM = LINT_VERBATIM + "; write a testable anchor (see references/anchor-capture.md)"
QUOTE_MISSING_PROBLEM = "quote missing; record the user's verbatim words"
REQUIREMENT_FORM_WARNINGS = (
    "hedged wording; state the requirement",
    "written as speech, not as a requirement",
    "not phrased as a testable requirement",
)

_sleep = time.sleep  # replaced in tests

SAFE_AGENT_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$")
BEARER_RE = re.compile(r"(?i)(bearer\s+)[^\s\"']+")


# --------------------------------------------------------------------------- #
# plumbing
# --------------------------------------------------------------------------- #
class JudgeError:
    """An expected failure, carried in Err. Never raised: main() turns it into the error envelope and exit 2."""

    def __init__(self, code: str, message: str, recoverable: bool = False, action: str = "",
                 status=None, retry_after=None):
        self.code = code
        self.message = message
        self.recoverable = recoverable
        self.action = action
        self.status = status            # HTTP status when the error came from Jev
        self.retry_after = retry_after  # parsed Retry-After header, seconds, or None
        self.meta = {}                  # attempt / shortening bookkeeping, filled by judge_with_policy
        self.data = None                # optional facts that explain the error (PLAN.TOO_LARGE carries the bead's shape)

    def __str__(self) -> str:
        return self.message

    def __repr__(self) -> str:
        return f"JudgeError({self.code!r}, {self.message!r})"

    def to_json(self) -> dict:
        return redact_sensitive({
            "success": False,
            "data": self.data,
            "error": {
                "code": self.code,
                "message": str(self),
                "recoverable": self.recoverable,
                "suggested_action": self.action,
            },
        })


def fail(code: str, message: str, **fields) -> Err[JudgeError]:
    """An expected failure as a value: ``return fail(...)`` where the old code raised."""
    return Err(JudgeError(code, message, **fields))


def jev_failure(what: str, failures: list, total: int, data: dict) -> Err[JudgeError]:
    """Some Jev calls produced no answer: the command could not do its job (exit 2), never a negative result.

    ``failures`` is ``[(label, JudgeError)]`` in report order; the envelope takes the first failure's code and
    action, and ``data`` (the partial work, e.g. the run path) gains ``failures`` so nothing judged is lost.
    """
    first = failures[0][1]
    shown = "; ".join(f"{label}: {e.code} ({e.message})" for label, e in failures[:5])
    if len(failures) > 5:
        shown += f"; +{len(failures) - 5} more"
    out = JudgeError(first.code, f"{len(failures)} of {total} {what} got no answer from Jev: {shown}",
                     recoverable=all(e.recoverable for _, e in failures),
                     action=first.action or "Fix the cause named in the message and rerun the command.")
    out.data = dict(data, failures=[{"id": label, "code": e.code, "message": e.message} for label, e in failures])
    return Err(out)


def redact_sensitive(value):
    """Remove the live TypeSafe credential and bearer tokens from every emitted value."""
    if isinstance(value, dict):
        return {key: redact_sensitive(item) for key, item in value.items()}
    if isinstance(value, list):
        return [redact_sensitive(item) for item in value]
    if isinstance(value, tuple):
        return tuple(redact_sensitive(item) for item in value)
    if not isinstance(value, str):
        return value
    text = value
    api_key = os.environ.get("TYPESAFE_API_KEY", "")
    if api_key:
        text = text.replace(api_key, "<redacted>")
    return BEARER_RE.sub(r"\1<redacted>", text)


def emit(obj, fmt: str = "json") -> None:
    obj = redact_sensitive(obj)
    if fmt == "json":
        print(json.dumps(obj, indent=2, ensure_ascii=False))
    else:
        print(obj)


def now_utc() -> _dt.datetime:
    return _dt.datetime.now(_dt.timezone.utc)


def stamp() -> str:
    return now_utc().strftime("%Y%m%dT%H%M%SZ")


def history_event(event: str, **fields) -> dict:
    ev = {"at": now_utc().isoformat(timespec="seconds"), "event": event}
    ev.update(fields)
    return ev


def load_json(path: str, what: str) -> Result[Any, JudgeError]:
    if path == "-":
        raw = sys.stdin.read()
        try:
            return Ok(json.loads(raw))
        except json.JSONDecodeError as exc:
            return fail("INPUT.MALFORMED", f"{what} on stdin is not valid JSON: {exc}")
    p = pathlib.Path(path) if path else None
    if p is None or not path.strip() or not p.is_file():
        return fail("INPUT.MISSING", f"{what} not found: {path!r}" + (" (not a regular file)" if p is not None and path.strip() and p.exists() else ""),
                    action=f"Pass the path of an existing file for the {what}.")
    try:
        return Ok(json.loads(p.read_text(encoding="utf-8")))
    except json.JSONDecodeError as exc:
        return fail("INPUT.MALFORMED", f"{what} is not valid JSON: {exc}", action=f"Fix the JSON in {path}.")
    except (OSError, UnicodeDecodeError) as exc:
        return fail("INPUT.MALFORMED", f"{what} could not be read: {exc}", action=f"Check {path} is a readable UTF-8 file.")


def load_record(path: str) -> Result[dict, JudgeError]:
    loaded = load_json(path, "anchors.json")
    if isinstance(loaded, Err):
        return loaded
    rec = loaded.value
    if not isinstance(rec, dict):
        return fail("INPUT.MALFORMED", "anchors.json must be a JSON object", action="See references/anchor-document.md for the schema.")
    return Ok(rec)


def load_run(path: str, what: str) -> Result[dict, JudgeError]:
    loaded = load_json(path, what)
    if isinstance(loaded, Err):
        return loaded
    run = loaded.value
    if not isinstance(run, dict) or run.get("schema") != "plan-anchoring/run/1" or not isinstance(run.get("results"), list) \
            or not isinstance(run.get("item"), dict) or not isinstance(run.get("summary"), dict):
        return fail("INPUT.MALFORMED", f"{what} {path} is not a plan-anchoring run file", action="Pass a file written by `judge`.")
    return Ok(run)


def write_text_atomic(path: str, text: str) -> None:
    p = pathlib.Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    tmp = p.with_name(p.name + ".tmp")
    tmp.write_text(text, encoding="utf-8")
    tmp.replace(p)


def save_json(path: str, obj: dict) -> None:
    """Write ``obj`` as JSON, redacted as ``emit`` redacts stdout (REQ-017): no key or bearer token reaches a file."""
    write_text_atomic(path, json.dumps(redact_sensitive(obj), indent=2, ensure_ascii=False) + "\n")


def short_date(iso: str) -> str:
    try:
        d = _dt.date.fromisoformat(iso[:10])
        return d.strftime("%b %-d") if os.name != "nt" else d.strftime("%b %d").replace(" 0", " ")
    except (ValueError, TypeError):
        return iso or "undated"


def sanitize_id(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]", "_", text)


def _inside(path: pathlib.Path, root: pathlib.Path) -> bool:
    return path == root or root in path.parents


def canonical_workspace_path(raw: str, root: pathlib.Path, what: str, *, must_exist: bool,
                             regular_file: bool = False) -> Result[pathlib.Path, JudgeError]:
    if not raw or not raw.strip():
        return fail("VALIDATION.PATH", f"{what} is empty", action=f"Pass a {what} inside the repository.")
    candidate = pathlib.Path(raw).expanduser()
    if not candidate.is_absolute():
        candidate = root / candidate
    try:
        resolved = candidate.resolve(strict=must_exist)
    except (OSError, RuntimeError) as exc:
        return fail("VALIDATION.PATH", f"{what} could not be resolved: {exc}",
                    action=f"Pass a readable {what} inside {root}.")
    if not _inside(resolved, root):
        return fail("VALIDATION.PATH", f"{what} is outside the repository: {resolved}",
                    action=f"Use a path under {root}; external files are never read or sent to Jev.")
    if regular_file and not resolved.is_file():
        return fail("VALIDATION.PATH", f"{what} is not a regular file: {resolved}",
                    action=f"Pass an existing regular file for {what}.")
    return Ok(resolved)


def cmd_agent_preflight(args) -> Result[dict, JudgeError]:
    """Fail closed before an agent reads local evidence or sends it to Jev."""
    try:
        root = pathlib.Path(args.workspace_root).expanduser().resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        return fail("VALIDATION.PATH", f"workspace root could not be resolved: {exc}",
                    action="Run from a Git worktree and pass `git rev-parse --show-toplevel`.")
    if not root.is_dir():
        return fail("VALIDATION.PATH", f"workspace root is not a directory: {root}")

    try:
        script = pathlib.Path(args.script).expanduser().resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        return fail("VALIDATION.PATH", f"script could not be resolved: {exc}",
                    action="Use the installed plan-anchoring script selected from the harness skill directory.")
    running_script = pathlib.Path(__file__).resolve()
    if script != running_script:
        return fail("VALIDATION.PATH", f"script does not match the running plan-anchoring script: {script}",
                    action="Use the installed plan-anchoring script selected from the harness skill directory.")
    # Trust rule: the script must be a copy in the nearest per-folder install at or above the working
    # directory (walking up, stopping before the home directory), never one in a global location.
    import plan_anchor_paths  # stdlib-only sibling
    cwd = pathlib.Path.cwd().resolve()
    start = cwd if _inside(cwd, root) else root
    trusted, why = plan_anchor_paths.trusted_script(script, start)
    if not trusted:
        return fail("VALIDATION.PATH", f"untrusted plan-anchoring script {script}: {why}",
                    action="Install plan-anchor into the repository or a parent folder with the plan-anchor-installer "
                           "skill (install.py --dest <folder>) and pass that copy's jev_judge.py.")

    resolved = canonical_workspace_path(args.anchors, root, "anchors file", must_exist=True, regular_file=True)
    if isinstance(resolved, Err):
        return resolved
    anchors = resolved.value
    raw_run_dir = args.run_dir or str(anchors.parent / "anchor-runs")
    resolved = canonical_workspace_path(raw_run_dir, root, "run directory", must_exist=False)
    if isinstance(resolved, Err):
        return resolved
    run_dir = resolved.value
    if run_dir.exists() and not run_dir.is_dir():
        return fail("VALIDATION.PATH", f"run directory is not a directory: {run_dir}",
                    action="Pass a directory inside the repository for run output.")
    evidence = []
    for path in args.evidence:
        resolved = canonical_workspace_path(path, root, "evidence file", must_exist=True, regular_file=True)
        if isinstance(resolved, Err):
            return resolved
        evidence.append(str(resolved.value))

    for identifier in args.identifier:
        if not SAFE_AGENT_ID_RE.fullmatch(identifier):
            return fail("VALIDATION.ID", f"unsafe agent identifier: {identifier!r}",
                        action="Use 1-128 letters, digits, dots, underscores, colons or hyphens; do not use paths or whitespace.")
    if args.pr_number and (not args.pr_number.isascii() or not args.pr_number.isdigit() or int(args.pr_number) < 1):
        return fail("VALIDATION.ID", f"invalid PR number: {args.pr_number!r}",
                    action="Pass a positive decimal pull-request number.")

    work_source = args.correlation_id or (args.identifier[0] if args.identifier else "run")
    return Ok({"success": True, "data": {
        "workspace_root": str(root),
        "script": str(script),
        "anchors_path": str(anchors),
        "run_dir": str(run_dir),
        "evidence": evidence,
        "identifiers": list(args.identifier),
        "pr_number": args.pr_number or None,
        "work_key": sanitize_id(work_source)[:128],
    }, "error": None})


# --------------------------------------------------------------------------- #
# anchor records
# --------------------------------------------------------------------------- #
def empty_record(plan: str, intent_ref: str) -> dict:
    return {
        "schema": "plan-anchoring/anchors/1",
        "plan": plan,
        "intent_ref": intent_ref,
        "anchors": [],
    }


def anchor_list(rec: dict) -> Result[list, JudgeError]:
    anchors = rec.get("anchors")
    if anchors is None:
        anchors = rec["anchors"] = []
    if not isinstance(anchors, list):
        return fail("INPUT.MALFORMED", "anchors.json: 'anchors' must be a list", action="Fix the record by hand, then run validate.")
    return Ok(anchors)


def find_anchor(rec: dict, anchor_id: str) -> Result[dict, JudgeError]:
    listed = anchor_list(rec)
    if isinstance(listed, Err):
        return listed
    for a in listed.value:
        if isinstance(a, dict) and a.get("id") == anchor_id:
            return Ok(a)
    return fail("ANCHOR.UNKNOWN", f"No anchor with id {anchor_id}", action="Check anchors.json for the exact ID.")


def next_anc_id(rec: dict) -> Result[str, JudgeError]:
    listed = anchor_list(rec)
    if isinstance(listed, Err):
        return listed
    nums = []
    for a in listed.value:
        aid = a.get("id") if isinstance(a, dict) else None
        if isinstance(aid, str) and re.fullmatch(r"ANC-\d{3,}", aid):
            nums.append(int(aid[4:]))
    return Ok(f"ANC-{(max(nums) + 1 if nums else 1):03d}")


def require_valid(rec: dict) -> Result[None, JudgeError]:
    problems = validate_record(rec)
    if problems:
        return fail("VALIDATION.ANCHORS", "anchors.json has problems: " + "; ".join(problems[:5]), recoverable=True,
                    action="Run `jev_judge.py validate` and fix the record first.")
    return Ok(None)


# --------------------------------------------------------------------------- #
# statement text: normalisation, testability lint, similarity
# --------------------------------------------------------------------------- #
def normalise_text(text) -> str:
    """Lowercase, collapse whitespace, drop trailing punctuation: the comparison form of a statement or quote."""
    s = re.sub(r"\s+", " ", str(text or "")).strip().lower()
    return s.rstrip(" .!?;:,")


def same_text(a, b) -> bool:
    return normalise_text(a) == normalise_text(b)


def anchor_quote(a: dict) -> str:
    """The verbatim quote; a record written before the quote field existed is read as quote = statement."""
    q = a.get("quote")
    if isinstance(q, str) and q.strip():
        return q
    return str(a.get("statement") or "")


def anchor_kind(a: dict) -> str:
    """directive (default), prohibition or structural; a record without `kind` is a directive."""
    k = a.get("kind")
    return k if k in KINDS else "directive"


def _has_word(text: str, phrase: str) -> bool:
    return re.search(r"(?<![A-Za-z0-9])" + re.escape(phrase) + r"(?![A-Za-z0-9])", text, re.IGNORECASE) is not None


def lint_statement(statement: str, quote: str, source_type: str = "ANC") -> list:
    """Deterministic testability warnings for an authored statement. Order is fixed; each rule fires at most once.

    The first rule (statement is the verbatim quote) is an error in `add` and `validate` for ANC anchors; it never
    fires for REQ/ADR anchors, whose quote is the register text and legitimately equals the statement.
    """
    warnings = []
    text = str(statement or "").strip()
    if source_type == "ANC" and quote and same_text(text, quote):
        warnings.append(LINT_VERBATIM)
    if len(text) > MAX_STATEMENT_CHARS:
        warnings.append("too long; split into one constraint per anchor")
    if any(_has_word(text, h) for h in HEDGES):
        warnings.append("hedged wording; state the requirement")
    if any(_has_word(text, p) for p in SPEECH_PRONOUNS):
        warnings.append("written as speech, not as a requirement")
    if not any(_has_word(text, v) for v in REQUIREMENT_VERBS):
        warnings.append("not phrased as a testable requirement")
    if any(_has_word(text, v) for v in VAGUE_TERMS):
        warnings.append("vague term; name the observable outcome")
    return warnings


def tokens(text) -> set:
    """Lowercase alphanumeric tokens minus a small stopword list and stray single letters (possessive 's', 'i.e.')."""
    out = set()
    for tok in re.findall(r"[a-z0-9]+", str(text or "").lower()):
        if tok in STOPWORDS or (len(tok) == 1 and tok.isalpha()):
            continue
        out.add(tok)
    return out


def jaccard(a: set, b: set) -> float:
    if not a and not b:
        return 0.0
    return len(a & b) / len(a | b)


def best_similarity(statement_a: str, quote_a: str, statement_b: str, quote_b: str) -> tuple:
    """Token Jaccard over testable statements only.

    Quotes are immutable provenance, not constraints. One multi-directive quote
    may legitimately produce several distinct atomic anchors, so quote overlap
    must never make those anchors duplicates.
    """
    del quote_a, quote_b
    return (jaccard(tokens(statement_a), tokens(statement_b)), "statement", "statement")


def similar_anchors(anchors: list, statement: str, quote: str, floor: float = SIMILAR_REPORT_FLOOR) -> list:
    """Existing proposed/locked/promoted anchors whose statement or quote is similar to the new text, best first."""
    found = []
    for a in anchors:
        if not isinstance(a, dict) or a.get("status") not in ("proposed", "locked", "promoted"):
            continue
        score, new_field, old_field = best_similarity(statement, quote, a.get("statement") or "", anchor_quote(a))
        if score >= floor:
            found.append({"id": a.get("id"), "jaccard": round(score, 3), "field": f"{new_field}/{old_field}"})
    found.sort(key=lambda s: (-s["jaccard"], str(s["id"])))
    return found


def lint_or_refuse(statement: str, quote: str, source_type: str, strict: bool) -> Result[list, JudgeError]:
    """Run the lint; the verbatim rule is always a refusal for ANC, every other warning only under --strict.

    Both are refusals (``is_refusal``): the caller reports them in data.refused with exit 0.
    """
    warnings = lint_statement(statement, quote, source_type)
    if LINT_VERBATIM in warnings:
        refused = JudgeError("VALIDATION.INPUT", LINT_VERBATIM_PROBLEM, recoverable=True,
                             action="Write the statement as a testable requirement in the orchestrator's words; keep the user's words in --quote.")
        refused.refusal = True
        return Err(refused)
    if strict and warnings:
        return fail("ANCHOR.LINT", "statement has testability warnings: " + "; ".join(warnings), recoverable=True,
                    action="Reword the statement (see references/anchor-capture.md), or drop --strict to record it anyway.")
    return Ok(warnings)


REFUSAL_CODES = ("ANCHOR.LINT", "ANCHOR.DUPLICATE_CANDIDATE", "ANCHOR.NOT_TESTABLE", "ANCHOR.DUPLICATE")


def is_refusal(error: JudgeError) -> bool:
    """A quality gate's no (lint, duplicate, verbatim quote, rubric): a negative answer, not a tool failure."""
    return error.code in REFUSAL_CODES or getattr(error, "refusal", False)


def refusal(error: JudgeError, verb: str, **data) -> Ok:
    """A quality gate said no (`add`/`update`): a completed negative answer, so success with data.<verb> false and exit 0.

    data.refused keeps the code, message, recoverable flag and suggested action the refusal used to carry as an error.
    """
    out = {verb: False, "refused": {"code": error.code, "message": error.message, "recoverable": error.recoverable,
                                    "suggested_action": error.action}}
    out.update(data)
    return Ok({"success": True, "data": out, "error": None})


def cmd_add(args) -> Result[dict, JudgeError]:
    p = pathlib.Path(args.anchors)
    if p.exists():
        loaded = load_record(args.anchors)
        if isinstance(loaded, Err):
            return loaded
        rec = loaded.value
    else:
        # Defaults follow --anchors: docs/plans/phase-b/anchors.json is plan "phase-b", intent docs/plans/phase-b/intent.md.
        rec = empty_record(args.plan or p.resolve().parent.name,
                           args.intent_ref or (pathlib.PurePath(args.anchors).parent / "intent.md").as_posix())
    listed = anchor_list(rec)
    if isinstance(listed, Err):
        return listed
    anchors = listed.value
    statement = args.statement.strip()
    if not statement:
        return fail("VALIDATION.INPUT", "statement cannot be empty; write the testable anchor text",
                    action="Pass --statement with the authored requirement and --quote with the user's verbatim words.")
    if args.status == "retired":
        return fail("VALIDATION.INPUT", "a new anchor cannot be added as retired", action="Add it as proposed or locked; retire it later with `retire`.")
    if args.source_type == "ANC":
        allocated = next_anc_id(rec)
        if isinstance(allocated, Err):
            return allocated
        anchor_id = allocated.value
        source_ref = None
    else:
        if not args.source_ref:
            return fail("VALIDATION.INPUT", f"--source-ref is required for source type {args.source_type}",
                        action="Pass the register ID, e.g. --source-ref REQ-014.")
        anchor_id = args.source_ref
        source_ref = args.source_ref
        if any(isinstance(a, dict) and a.get("id") == anchor_id for a in anchors):
            refused = JudgeError("ANCHOR.DUPLICATE", f"{anchor_id} already recorded", recoverable=True,
                                 action=f"Update {anchor_id} instead (`update --id {anchor_id}`), or retire it first.")
            return refusal(refused, "added", path=args.anchors)
    quote = (args.quote or "").strip()
    if args.source_type == "ANC":
        if not quote:
            return fail("VALIDATION.INPUT", "--quote is required for ANC anchors: record the user's verbatim words",
                        action='Pass --quote "<the user\'s exact words>" alongside the authored --statement.')
    elif not quote:
        quote = statement  # register text: the quote is the statement
    linted = lint_or_refuse(statement, quote, args.source_type, bool(args.strict))
    if isinstance(linted, Err):
        if is_refusal(linted.error):
            return refusal(linted.error, "added", path=args.anchors, lint={"warnings": lint_statement(statement, quote, args.source_type)})
        return linted
    warnings = linted.value
    similar = similar_anchors(anchors, statement, quote)
    candidates = [s for s in similar if s["jaccard"] >= DUPLICATE_JACCARD]
    if candidates and not args.allow_similar:
        names = ", ".join(f"{s['id']} (jaccard {s['jaccard']}, {s['field']})" for s in candidates)
        refused = JudgeError("ANCHOR.DUPLICATE_CANDIDATE", f"the new anchor looks like an existing one: {names}", recoverable=True,
                             action=f"Update {candidates[0]['id']} instead (or retire it with --replaced-by), or pass --allow-similar if it is really a different constraint.")
        return refusal(refused, "added", path=args.anchors, lint={"warnings": warnings}, similar=similar)
    rubric = None
    if getattr(args, "rubric", False):
        gated = add_rubric_gate(args, statement, quote, args.source_type)
        if isinstance(gated, Err):
            return gated
        rubric, refused = gated.value
        if refused is not None:
            return refusal(refused, "added", path=args.anchors, lint={"warnings": warnings}, similar=similar, rubric=rubric)
    record = {
        "id": anchor_id,
        "source_type": args.source_type,
        "source_ref": source_ref,
        "statement": statement,
        "quote": quote,
        "kind": args.kind,
        "stated_by": args.stated_by,
        "stated_at": args.stated_at or _dt.date.today().isoformat(),
        "context": args.context,
        "summary": args.summary or "",
        "scope": args.scope or "",
        "deterministic_check": args.deterministic_check or "",
        "fixtures": {"pass": [], "fail": []},
        "question_overrides": {},
        "version": 1,
        "status": args.status,
        "promoted_to": None,
        "beads": {"anchor": None, "plan_questions": None, "dev_questions": None},
        "history": [history_event("created", status=args.status)],
    }
    anchors.append(record)
    save_json(args.anchors, rec)
    data = {"added": True, "anchor": record, "path": args.anchors, "lint": {"warnings": warnings}, "similar": similar}
    if rubric is not None:
        data["rubric"] = rubric
    return Ok({"success": True, "data": data, "error": None})


def _record_and_anchor(path: str, anchor_id: str) -> Result[tuple, JudgeError]:
    """Load anchors.json and find one anchor in it: (record, anchor)."""
    loaded = load_record(path)
    if isinstance(loaded, Err):
        return loaded
    rec = loaded.value
    found = find_anchor(rec, anchor_id)
    if isinstance(found, Err):
        return found
    return Ok((rec, found.value))


def cmd_update(args) -> Result[dict, JudgeError]:
    if getattr(args, "quote", None) is not None:
        return fail("VALIDATION.INPUT", "quote is verbatim provenance and immutable",
                    action="If the user said something different, `add` a new anchor with the new quote and `retire --replaced-by` this one.")
    loaded = _record_and_anchor(args.anchors, args.id)
    if isinstance(loaded, Err):
        return loaded
    rec, a = loaded.value
    changed = []
    previous_statement = None
    if args.statement is not None:
        new_statement = args.statement.strip()
        if not new_statement:
            return fail("VALIDATION.INPUT", "statement cannot be empty; write the testable anchor text")
        if new_statement != (a.get("statement") or ""):
            previous_statement = a.get("statement")
            a["statement"] = new_statement
            changed.append("statement")
    for key, val in (("scope", args.scope), ("summary", args.summary), ("deterministic_check", args.deterministic_check)):
        if val is not None and (a.get(key) or "") != val:
            a[key] = val
            changed.append(key)
    new_kind = getattr(args, "kind", None)
    if new_kind is not None and new_kind != anchor_kind(a):
        a["kind"] = new_kind
        changed.append("kind")
    fixtures = a.get("fixtures") if isinstance(a.get("fixtures"), dict) else {}
    before = json.dumps(fixtures, sort_keys=True)
    if args.clear_fixtures:
        fixtures = {"pass": [], "fail": []}
    fixtures.setdefault("pass", [])
    fixtures.setdefault("fail", [])
    for s in args.fixture_pass or []:
        if s.strip():
            fixtures["pass"].append(s.strip())
    for s in args.fixture_fail or []:
        if s.strip():
            fixtures["fail"].append(s.strip())
    if json.dumps(fixtures, sort_keys=True) != before:
        a["fixtures"] = fixtures
        changed.append("fixtures")
    if not changed:  # every value given already holds: a no-op, not a failure; nothing is saved
        # data.changed is the list of changed fields on an update, false on a no-op (ADR-007)
        return Ok({"success": True, "data": {"updated": False, "changed": False, "id": a["id"],
                                             "version": a.get("version", 1), "anchor": a}, "error": None})
    linted = lint_or_refuse(a.get("statement") or "", anchor_quote(a), a.get("source_type") or "ANC", bool(getattr(args, "strict", False)))
    if isinstance(linted, Err):
        if is_refusal(linted.error):  # nothing is saved
            return refusal(linted.error, "updated", id=args.id, changed=changed,
                           lint={"warnings": lint_statement(a.get("statement") or "", anchor_quote(a), a.get("source_type") or "ANC")})
        return linted
    warnings = linted.value
    a["version"] = int(a.get("version", 1)) + 1
    event = history_event("updated", fields=changed)
    if "statement" in changed:
        event["previous_statement"] = previous_statement
    a.setdefault("history", []).append(event)
    save_json(args.anchors, rec)
    return Ok({"success": True, "data": {"updated": True, "id": a["id"], "changed": changed, "version": a["version"], "anchor": a,
                                         "lint": {"warnings": warnings}}, "error": None})


def cmd_retire(args) -> Result[dict, JudgeError]:
    loaded = _record_and_anchor(args.anchors, args.id)
    if isinstance(loaded, Err):
        return loaded
    rec, a = loaded.value
    if a.get("status") == "retired":  # already done: a no-op, not a failure; nothing is saved
        return Ok({"success": True, "data": {"id": a["id"], "status": "retired", "changed": False,
                                             "replaced_by": args.replaced_by or None, "version": a.get("version", 1)}, "error": None})
    if args.replaced_by:
        replacement = find_anchor(rec, args.replaced_by)  # the replacement must exist
        if isinstance(replacement, Err):
            return replacement
    a["status"] = "retired"
    a["version"] = int(a.get("version", 1)) + 1
    a.setdefault("history", []).append(history_event("retired", replaced_by=args.replaced_by or None, reason=args.reason or ""))
    save_json(args.anchors, rec)
    return Ok({"success": True, "data": {"id": a["id"], "status": "retired", "changed": True,
                                         "replaced_by": args.replaced_by or None, "version": a["version"]}, "error": None})


def apply_exclusion(a: dict, item: str, reason: str, remove: bool) -> Result[dict, JudgeError]:
    """Add, replace or remove one exclusion on an anchor in memory. Ok(data); data.changed says whether the anchor changed."""
    exclude = a.get("exclude")
    if exclude is not None and not isinstance(exclude, list):
        return fail("VALIDATION.ANCHORS", f"{a['id']}: exclude must be a list", action="Fix the record by hand, then run validate.")
    kept = [x for x in (exclude or []) if not (isinstance(x, dict) and x.get("item") == item)]
    removed = len(kept) != len(exclude or [])
    if remove:
        if not removed:  # nothing to remove: a no-op, not a failure
            return Ok({"id": a["id"], "item": item, "removed": True, "changed": False,
                       "exclude": a.get("exclude", []), "version": a.get("version", 1)})
        a["exclude"] = kept
        if not kept:
            del a["exclude"]
        event = history_event("exclusion_removed", item=item)
    else:
        a["exclude"] = kept + [{"item": item, "reason": reason}]
        event = history_event("excluded", item=item, reason=reason, replaced=removed)
    a.setdefault("history", []).append(event)
    return Ok({"id": a["id"], "item": item, "removed": bool(remove), "changed": True,
               "exclude": a.get("exclude", []), "version": a.get("version", 1)})


def read_exclusion_lines(path: str) -> Result[list, JudgeError]:
    """Parse an `exclude --from` JSONL file: one {id, item, reason} or {id, item, remove: true} per non-blank line."""
    p = pathlib.Path(path)
    if not path.strip() or not p.is_file():
        return fail("INPUT.MISSING", f"exclusions file not found: {path!r}", action="Pass an existing JSONL file to --from.")
    try:
        text = p.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        return fail("INPUT.MALFORMED", f"exclusions file could not be read: {exc}", action=f"Check {path} is a readable UTF-8 file.")
    entries, seen = [], {}
    for n, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        where = f"{path}:{n}"
        try:
            e = json.loads(line)
        except json.JSONDecodeError as exc:
            return fail("INPUT.MALFORMED", f"{where}: not a JSON object ({exc.msg})", action="Write one exclusion object per line.")
        if not isinstance(e, dict):
            return fail("INPUT.MALFORMED", f"{where}: not a JSON object", action="Write one exclusion object per line.")
        unknown = sorted(set(e) - {"id", "item", "reason", "remove"})
        if unknown:
            return fail("INPUT.MALFORMED", f"{where}: unknown field(s) {unknown}", action="Use only id, item, reason and remove.")
        aid, item = e.get("id"), e.get("item")
        reason, remove = e.get("reason", ""), e.get("remove", False)
        if not isinstance(aid, str) or not aid.strip() or not isinstance(item, str) or not item.strip():
            return fail("VALIDATION.INPUT", f"{where}: 'id' and 'item' must be non-empty strings",
                        action="Pass the anchor id and the plan item id exactly as judge --item-id gets it.")
        if not isinstance(remove, bool) or not isinstance(reason, str):
            return fail("INPUT.MALFORMED", f"{where}: 'remove' must be true or false and 'reason' a string")
        if not remove and not reason.strip():
            return fail("VALIDATION.INPUT", f"{where}: 'reason' cannot be empty",
                        action="Say in one sentence why the anchor does not apply to this item.")
        key = (aid, item.strip())
        if key in seen:
            return fail("VALIDATION.INPUT", f"{where}: {aid} x {item.strip()} is already on line {seen[key]}",
                        action="List each (anchor, item) pair once.")
        seen[key] = n
        entries.append({"id": aid, "item": item.strip(), "reason": reason.strip(), "remove": remove, "line": n})
    if not entries:
        return fail("INPUT.EMPTY", f"exclusions file {path} has no entries", action="Write one exclusion object per line.")
    return Ok(entries)


def cmd_exclude(args) -> Result[dict, JudgeError]:
    """Add, replace or remove exclusions. Not a requirement change: no version bump, earlier runs stay valid, any status.

    --from applies every line of a JSONL file atomically: any bad line or unknown anchor saves nothing.
    """
    if args.from_file:
        if args.id or args.item or args.reason or args.remove:
            return fail("VALIDATION.INPUT", "--from cannot be combined with --id, --item, --reason or --remove",
                        action="Put every exclusion in the file, or use the single-cell flags without --from.")
        parsed = read_exclusion_lines(args.from_file)
        if isinstance(parsed, Err):
            return parsed
        entries = parsed.value
    else:
        if not args.id or args.item is None:
            return fail("VALIDATION.INPUT", "pass --id and --item, or --from <file.jsonl>")
        item = (args.item or "").strip()
        if not item:
            return fail("VALIDATION.INPUT", "--item cannot be empty; pass the plan item id exactly as judge --item-id gets it")
        reason = (args.reason or "").strip()
        if not args.remove and not reason:
            return fail("VALIDATION.INPUT", "--reason cannot be empty",
                        action="Say in one sentence why the anchor does not apply to this item.")
        entries = [{"id": args.id, "item": item, "reason": reason, "remove": bool(args.remove), "line": None}]
    loaded = load_record(args.anchors)
    if isinstance(loaded, Err):
        return loaded
    rec = loaded.value
    results = []
    for e in entries:
        found = find_anchor(rec, e["id"])
        if isinstance(found, Err):
            if e["line"] is not None:
                return fail(found.error.code, f"{args.from_file}:{e['line']}: {found.error.message}", action=found.error.action)
            return found
        applied = apply_exclusion(found.value, e["item"], e["reason"], e["remove"])
        if isinstance(applied, Err):
            return applied
        results.append(applied.value)
    if any(r["changed"] for r in results):
        save_json(args.anchors, rec)
    if not args.from_file:
        return Ok({"success": True, "data": results[0], "error": None})
    return Ok({"success": True, "data": {"applied": results, "count": len(results),
                                         "changed": sum(1 for r in results if r["changed"])}, "error": None})


def validate_record(rec: dict) -> list:
    problems = []
    if not isinstance(rec, dict):
        return ["anchors.json must be a JSON object"]
    if rec.get("schema") != "plan-anchoring/anchors/1":
        problems.append("schema must be 'plan-anchoring/anchors/1'")
    anchors = rec.get("anchors", [])
    if not isinstance(anchors, list):
        problems.append("'anchors' must be a list")
        return problems
    seen = set()
    for n, a in enumerate(anchors):
        if not isinstance(a, dict):
            problems.append(f"anchors[{n}]: must be an object")
            continue
        aid = a.get("id", "<missing>")
        if not isinstance(aid, str) or not aid:
            problems.append(f"anchors[{n}]: id must be a non-empty string")
            aid = str(aid)
        if aid in seen:
            problems.append(f"{aid}: duplicate id")
        seen.add(aid)
        if a.get("source_type") not in SOURCE_TYPES:
            problems.append(f"{aid}: source_type must be one of {SOURCE_TYPES}")
        if a.get("source_type") == "ANC" and not re.fullmatch(r"ANC-\d{3,}", aid):
            problems.append(f"{aid}: ANC ids look like ANC-001")
        if a.get("source_type") in ("REQ", "ADR") and not a.get("source_ref"):
            problems.append(f"{aid}: REQ/ADR anchors need source_ref")
        if not isinstance(a.get("statement"), str) or not a["statement"].strip():
            problems.append(f"{aid}: statement is empty")
        quote = a.get("quote")
        if quote is not None and not isinstance(quote, str):
            problems.append(f"{aid}: quote must be a string")
        elif a.get("source_type") == "ANC":
            if not (isinstance(quote, str) and quote.strip()):
                problems.append(f"{aid}: {QUOTE_MISSING_PROBLEM}")
            if isinstance(a.get("statement"), str) and a["statement"].strip() and same_text(a["statement"], anchor_quote(a)):
                problems.append(f"{aid}: {LINT_VERBATIM_PROBLEM}")
        if a.get("status") not in STATUSES:
            problems.append(f"{aid}: status must be one of {STATUSES}")
        if a.get("kind") is not None and a.get("kind") not in KINDS:
            problems.append(f"{aid}: kind must be one of {KINDS}")
        if a.get("status") == "promoted" and not a.get("promoted_to"):
            problems.append(f"{aid}: promoted anchors need promoted_to")
        if not a.get("stated_at"):
            problems.append(f"{aid}: stated_at missing")
        fx = a.get("fixtures")
        if fx is not None and (not isinstance(fx, dict) or not all(isinstance(fx.get(k, []), list) for k in ("pass", "fail"))):
            problems.append(f"{aid}: fixtures must be an object with 'pass' and 'fail' lists")
        overrides = a.get("question_overrides") or {}
        if not isinstance(overrides, dict):
            problems.append(f"{aid}: question_overrides must be an object")
            overrides = {}
        for gate, q in overrides.items():
            if gate not in ("plan", "dev"):
                problems.append(f"{aid}: question_overrides key must be plan or dev, got {gate}")
            elif not isinstance(q, dict) or "questions" not in q:
                problems.append(f"{aid}: question_overrides.{gate} must have a 'questions' object")
            elif not isinstance(q["questions"], dict):
                problems.append(f"{aid}: question_overrides.{gate}.questions must be an object")
        problems.extend(exclusion_problems(aid, a.get("exclude")))
    return problems


def exclusion_problems(aid: str, exclude) -> list:
    """Schema of the optional `exclude` list: [{item: <plan item id>, reason: <non-empty sentence>}], one entry per item."""
    if exclude is None:
        return []
    if not isinstance(exclude, list):
        return [f"{aid}: exclude must be a list of " "{item, reason} objects"]
    problems, seen = [], set()
    for n, x in enumerate(exclude):
        where = f"{aid}: exclude[{n}]"
        if not isinstance(x, dict):
            problems.append(f"{where} must be an object with item and reason")
            continue
        extra = sorted(set(x) - set(EXCLUSION_KEYS))
        if extra:
            problems.append(f"{where} has unknown keys {extra}; only item and reason are allowed")
        item = x.get("item")
        if not isinstance(item, str) or not item.strip():
            problems.append(f"{where}.item must be a non-empty plan item id")
        elif item in seen:
            problems.append(f"{where}: duplicate exclusion for item {item}")
        else:
            seen.add(item)
        reason = x.get("reason")
        if not isinstance(reason, str) or not reason.strip():
            problems.append(f"{where}.reason is empty; say in one sentence why the anchor does not apply to this item")
    return problems


def exclusion_for(anchor: dict, item_id: str):
    """The anchor's exclusion entry for this item id (exact match), or None."""
    for x in anchor.get("exclude") or []:
        if isinstance(x, dict) and x.get("item") == item_id:
            return x
    return None


def record_warnings(rec: dict, anchors_path: str) -> list:
    warnings = []
    intent = rec.get("intent_ref")
    if intent:
        candidates = [pathlib.Path(intent), pathlib.Path(anchors_path).parent / pathlib.Path(intent).name]
        if not any(c.is_file() for c in candidates):
            warnings.append(f"intent_ref file not found: {intent}")
    anchors = rec.get("anchors") if isinstance(rec.get("anchors"), list) else []
    if not any(isinstance(a, dict) and a.get("status") in JUDGED_STATUSES for a in anchors):
        warnings.append("no locked anchors")
    return warnings


def cmd_validate(args) -> Result[dict, JudgeError]:
    loaded = load_record(args.anchors)
    if isinstance(loaded, Err):
        return loaded
    rec = loaded.value
    problems = validate_record(rec)
    warnings = record_warnings(rec, args.anchors)
    if problems:
        # A completed negative answer, not a failure of the command: success with data.valid false, exit 0.
        return Ok({"success": True, "data": {"valid": False, "path": args.anchors, "problems": problems, "warnings": warnings,
                                             "suggested_action": "Fix the listed problems and re-run validate."}, "error": None})
    return Ok({"success": True, "data": {"valid": True, "anchors": len(rec["anchors"]), "path": args.anchors, "problems": [],
                                         "warnings": warnings}, "error": None})


def render_md(rec: dict) -> str:
    lines = [f"# Anchors - {rec.get('plan', 'plan')}", ""]
    lines.append("Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.")
    lines.append("")
    if rec.get("intent_ref"):
        lines.append(f"Intent: `{rec['intent_ref']}`")
        lines.append("")
    lines.append("| ID | Status | Summary | Stated |")
    lines.append("| --- | --- | --- | --- |")
    for a in rec.get("anchors", []):
        lines.append(f"| {a['id']} | {a['status']} | {a.get('summary') or ''} | {a.get('stated_at', '')} |")
    lines.append("")
    for a in rec.get("anchors", []):
        lines.append(f"## {a['id']}" + (f" ({a['source_ref']})" if a.get("source_ref") and a['source_ref'] != a['id'] else ""))
        lines.append("")
        lines.append(f"Anchor: {a['statement']}")
        lines.append("")
        lines.append(f"> {anchor_quote(a)}")
        lines.append("")
        who = a.get("stated_by") or "user"
        lines.append(f"Said by {who}, {a.get('context') or 'planning'}, {a.get('stated_at', '')}. Status: {a['status']}" +
                     (f", promoted to {a['promoted_to']}" if a.get("promoted_to") else "") + f". Version {a.get('version', 1)}.")
        if a.get("summary"):
            lines.append("")
            lines.append(f"Summary: {a['summary']}")
        if a.get("scope"):
            lines.append("")
            lines.append(f"Scope: {a['scope']}")
        if a.get("deterministic_check"):
            lines.append("")
            lines.append(f"Deterministic check to pair with the development gate: {a['deterministic_check']}")
        if a.get("exclude"):
            lines.append("")
            lines.append("Excluded from (not judged; the cell passes as `excluded`):")
            for x in a["exclude"]:
                lines.append(f"- `{x['item']}`: {x['reason']}")
        b = a.get("beads") or {}
        if any(b.values()):
            lines.append("")
            lines.append(f"Beads: anchor `{b.get('anchor')}`, plan questions `{b.get('plan_questions')}`, dev questions `{b.get('dev_questions')}`")
        fx = a.get("fixtures") or {}
        if fx.get("pass") or fx.get("fail"):
            lines.append("")
            lines.append("Fixtures:")
            for s in fx.get("pass", []):
                lines.append(f"- pass: {s}")
            for s in fx.get("fail", []):
                lines.append(f"- fail: {s}")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def cmd_render(args) -> Result[dict, JudgeError]:
    loaded = load_record(args.anchors)
    if isinstance(loaded, Err):
        return loaded
    rec = loaded.value
    checked = require_valid(rec)
    if isinstance(checked, Err):
        return checked
    md = render_md(rec)
    out = args.out or str(pathlib.Path(args.anchors).with_name("anchors.md"))
    write_text_atomic(out, md)
    return Ok({"success": True, "data": {"path": out, "anchors": len(rec["anchors"])}, "error": None})


def cmd_set_beads(args) -> Result[dict, JudgeError]:
    loaded = _record_and_anchor(args.anchors, args.id)
    if isinstance(loaded, Err):
        return loaded
    rec, a = loaded.value
    beads = a.setdefault("beads", {})
    if not isinstance(beads, dict):
        beads = a["beads"] = {}
    for key, val in (("anchor", args.anchor_bead), ("plan_questions", args.plan_bead), ("dev_questions", args.dev_bead)):
        if val:
            beads[key] = val
    a.setdefault("history", []).append(history_event("beads_set", beads=beads))
    save_json(args.anchors, rec)
    return Ok({"success": True, "data": {"id": a["id"], "beads": beads}, "error": None})


def cmd_promote(args) -> Result[dict, JudgeError]:
    loaded = _record_and_anchor(args.anchors, args.id)
    if isinstance(loaded, Err):
        return loaded
    rec, a = loaded.value
    a["status"] = "promoted"
    a["promoted_to"] = args.adr
    a["version"] = int(a.get("version", 1)) + 1
    a.setdefault("history", []).append(history_event("promoted", to=args.adr))
    save_json(args.anchors, rec)
    return Ok({"success": True, "data": {"id": a["id"], "promoted_to": args.adr, "version": a["version"]}, "error": None})


# --------------------------------------------------------------------------- #
# item composition (from bd JSON; the script never runs bd)
# --------------------------------------------------------------------------- #
SECTION_KEYS = tuple(k for k, _ in ITEM_SECTIONS)
COMPOSE_FOR = {"plan": ("description", "acceptance_criteria"), "dev": SECTION_KEYS}  # plan: what Jev reads; verdicts matched the full text on 6/6 pairs


def demote_headings(body: str) -> str:
    """Push a field's own markdown headings down one level (outside code fences) so that the '## ' headings
    compose-item writes are the only level-2 headings; size_breakdown and shape key on them."""
    out, in_code = [], False
    for line in body.split("\n"):
        if FENCE_RE.match(line):
            in_code = not in_code
        elif not in_code:
            m = re.match(r"^(\s{0,3})(#{1,5})(\s)", line)
            if m:
                level = max(len(m.group(2)) + 1, 3)   # never level 1 or 2: those are compose-item's own headings
                line = m.group(1) + "#" * level + line[m.end(2):]
        out.append(line)
    return "\n".join(out)


def compose_item_text(show, children=None, sections=None) -> Result[dict, JudgeError]:
    """Markdown item from bd JSON. `sections` (keys from ITEM_SECTIONS) limits the body; None means every non-empty section."""
    if sections is not None:
        bad = [s for s in sections if s not in SECTION_KEYS]
        if bad:
            return fail("VALIDATION.INPUT", f"unknown section(s) {bad}; choose from {list(SECTION_KEYS)}")
    if isinstance(show, list):
        if len(show) != 1 or not isinstance(show[0], dict):
            return fail("INPUT.MALFORMED", "bd show --json output must be a one-element array holding the bead object",
                        action="Pass the unmodified output of `bd show <id> --json`.")
        show = show[0]
    if not isinstance(show, dict):
        return fail("INPUT.MALFORMED", "bead JSON must be an object or a one-element array")
    item_id = str(show.get("id") or "")
    title = str(show.get("title") or item_id or "untitled").strip()
    parts = [f"# {title}"]
    sections_used = []
    for key, heading in ITEM_SECTIONS:
        if sections is not None and key not in sections:
            continue
        val = show.get(key)
        if isinstance(val, str) and val.strip():
            parts.append(f"## {heading}\n\n{demote_headings(val.strip())}")
            sections_used.append(heading)
    if children is not None:
        if not isinstance(children, list):
            return fail("INPUT.MALFORMED", "bd children --json output must be an array", action="Pass the unmodified output of `bd children <id> --json`.")
        closed = [c for c in children if isinstance(c, dict) and c.get("status") == "closed"]
        if closed:
            rows = [f"- {c.get('id', '?')} {str(c.get('title') or '').strip()}: {str(c.get('close_reason') or '').strip()}".rstrip() for c in closed]
            parts.append("## Closed beads\n\n" + "\n".join(rows))
            sections_used.append("Closed beads")
    text = "\n\n".join(parts) + "\n"
    return Ok({"item_id": item_id, "title": title, "text": text, "sections": sections_used, "chars": len(text)})


def cmd_compose_item(args) -> Result[dict, JudgeError]:
    loaded = load_json(args.show_json, "bd show output")
    if isinstance(loaded, Err):
        return loaded
    show = loaded.value
    children = None
    if args.children_json:
        loaded = load_json(args.children_json, "bd children output")
        if isinstance(loaded, Err):
            return loaded
        children = loaded.value
    gate_for = getattr(args, "for_gate", "") or ""
    sections = None
    if getattr(args, "sections", ""):
        if gate_for:
            return fail("VALIDATION.INPUT", "pass --sections or --for, not both")
        sections = [s.strip() for s in args.sections.split(",") if s.strip()]
        if not sections:
            return fail("VALIDATION.INPUT", f"--sections needs at least one of {list(SECTION_KEYS)}")
    elif gate_for:
        sections = list(COMPOSE_FOR[gate_for])
        if gate_for == "plan":
            children = None  # closed children are development-gate evidence
    built = compose_item_text(show, children, sections)
    if isinstance(built, Err):
        return built
    composed = built.value
    data = {"path": None, "item_id": composed["item_id"], "title": composed["title"], "sections": composed["sections"], "chars": composed["chars"],
            "composition": {"for": gate_for or None, "sections_requested": sections if sections is not None else "all",
                            "children": children is not None}}
    if args.out:
        write_text_atomic(args.out, composed["text"])
        data["path"] = args.out
    else:
        data["text"] = composed["text"]
    return Ok({"success": True, "data": data, "error": None})


# --------------------------------------------------------------------------- #
# plan size and shape (plan gate reduction; the `shape` subcommand)
# --------------------------------------------------------------------------- #
FENCE_RE = re.compile(r"^(\s*)(`{3,}|~{3,})")
SIGNATURE_RES = tuple(re.compile(p) for p in (
    r"^\s*(pub(\([^)]*\))?\s+)?((async|const|unsafe|default|extern(\s+\"[^\"]*\")?)\s+)*fn\s+\w+",   # Rust fn
    r"^\s*(pub(\([^)]*\))?\s+)?(struct|enum|union|trait|type|mod)\s+\w+",                       # Rust items
    r"^\s*(unsafe\s+)?impl\b",
    r"^\s*(pub(\([^)]*\))?\s+)?(const|static)\s+(mut\s+)?[A-Z_][A-Z0-9_]*\s*:",
    r"^\s*macro_rules!\s*\w+",
    r"^\s*(async\s+)?def\s+\w+\s*\(",                                                           # Python
    r"^\s*class\s+\w+",
    r"^\s*(export\s+)?(default\s+)?(async\s+)?function\b",                                      # JS / TS
    r"^\s*(export\s+)?(interface|type)\s+\w+",
    r"^\s*(function\s+)?[\w-]+\s*\(\)\s*\{",                                                    # shell
    r"^\s*\[\[?[^\]=]+\]\]?\s*$",                                                                  # TOML / INI table
    r"^[@\w][\w-]*(\s+[^:=]*)?:(\s|$)(?!=)",                                                      # just recipe header
))
BULLET_RE = re.compile(r"^\s*(?:[-*+]|\d+[.)])\s+\S")
TABLE_ROW_RE = re.compile(r"^\s*\|.*\|\s*$")
TABLE_RULE_RE = re.compile(r"^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$")
REFERENCE_RES = {
    "github": re.compile(r"(?:(?<![\w/#])#(\d+)\b)|(?:github\.com/[\w.-]+/[\w.-]+/(?:issues|pull)/(\d+))"),
    "req": re.compile(r"\bREQ-\d+\b"),
    "adr": re.compile(r"\bADR-\d+\b"),
    "anc": re.compile(r"\bANC-\d{3,}\b"),
}
DUPLICATE_PARAGRAPH_MIN_CHARS = 120


def signature_lines(body: list) -> list:
    """Declaration lines of a code block; a signature whose parentheses are still open continues onto following lines."""
    keep, i = [], 0
    while i < len(body):
        line = body[i]
        if any(r.match(line) for r in SIGNATURE_RES):
            keep.append(line)
            depth = line.count("(") - line.count(")")
            while depth > 0 and i + 1 < len(body) and len(keep) < len(body):
                i += 1
                keep.append(body[i])
                depth += body[i].count("(") - body[i].count(")")
        i += 1
    return keep


def fenced_blocks(lines: list):
    """Yield (open, close, indent, fence) line indexes for each fenced code block in order; close == len(lines) when unterminated."""
    i = 0
    while i < len(lines):
        m = FENCE_RE.match(lines[i])
        if not m:
            i += 1
            continue
        indent, fence = m.group(1), m.group(2)
        j = i + 1
        while j < len(lines) and not (lines[j].strip().startswith(fence[0] * len(fence)) and not lines[j].strip().strip(fence[0])):
            j += 1
        yield i, j, indent, fence
        i = j + 1


def long_code_blocks(text: str, max_lines: int) -> list:
    """Ordinals of the fenced blocks longer than max_lines, longest first (ties in document order)."""
    lines = text.split("\n")
    sizes = [(n, j - i - 1) for n, (i, j, _, _) in enumerate(fenced_blocks(lines))]
    return [n for n, size in sorted(sizes, key=lambda s: (-s[1], s[0])) if size > max_lines]


def reduce_code(text: str, max_lines: int, only=None) -> tuple:
    """Replace each fenced code block longer than max_lines with its signature lines and a redaction marker.

    `only`, a set of fenced-block ordinals (0-based, in document order), limits the reduction to those blocks; None
    reduces every long block. The plan keeps the code; only the text sent to Jev is reduced, and the marker says so.
    Returns (text, stats).
    """
    lines = text.split("\n")
    out, stats, prev = [], {"blocks": 0, "lines": 0}, 0
    for n, (i, j, indent, fence) in enumerate(fenced_blocks(lines)):
        out.extend(lines[prev:i])
        body = lines[i + 1:j]
        if len(body) <= max_lines or (only is not None and n not in only):
            out.extend(lines[i:j + 1])
        else:
            sigs = signature_lines(body)
            dropped = len(body) - len(sigs)
            out.append(lines[i])
            out.extend(sigs)
            if dropped > 0:
                out.append(indent + CODE_REDACTION_MARKER.format(dropped=dropped, total=len(body)))
            out.append(lines[j] if j < len(lines) else indent + fence)
            stats["blocks"] += 1
            stats["lines"] += dropped
        prev = j + 1
    out.extend(lines[prev:])
    return "\n".join(out), stats


def section_key(heading: str) -> str:
    return re.sub(r"[^a-z0-9]+", "_", heading.strip().lower()).strip("_") or "untitled"


def find_references(text: str) -> dict:
    refs = {}
    for kind, rx in REFERENCE_RES.items():
        found = set()
        for m in rx.finditer(text):
            if kind == "github":
                found.add("#" + (m.group(1) or m.group(2)))
            else:
                found.add(m.group(0))
        refs[kind] = sorted(found)
    return refs


def size_breakdown(text: str) -> dict:
    """Where an item's characters are: per '## ' section, and prose versus fenced code. Facts for a judgement, not a verdict."""
    sections, current, code, in_code = {}, "(title)", 0, False
    for line in text.split("\n"):
        if FENCE_RE.match(line):
            in_code = not in_code
            code += len(line) + 1
            continue
        if not in_code and line.startswith("## "):
            current = line[3:].strip()
        sections[current] = sections.get(current, 0) + len(line) + 1
        if in_code:
            code += len(line) + 1
    return {"sections": sections, "code_chars": code, "prose_chars": len(text) - code}


def shape_text(text: str, max_code_lines: int = DEFAULT_MAX_CODE_LINES) -> dict:
    """Facts about one composed item text, on top of size_breakdown (so `shape` and PLAN.TOO_LARGE report the same numbers):
    chars per section, code versus prose (chars and lines), fenced blocks, bullets, table rows, references, and the prose
    paragraphs (for cross-bead repetition). Facts for a judgement, never a verdict."""
    size = size_breakdown(text)
    code_lines = prose_lines = blocks = bullets = table_rows = 0
    paragraphs, buf, in_code = [], [], False

    def flush():
        if buf:
            paragraphs.append(" ".join(buf))
            buf.clear()

    for line in text.split("\n"):
        if FENCE_RE.match(line):
            in_code = not in_code
            if in_code:
                blocks += 1
            flush()
            continue
        if in_code:
            code_lines += 1
            continue
        if line.startswith("## ") or not line.strip():
            flush()
            continue
        prose_lines += 1
        if BULLET_RE.match(line):
            bullets += 1
        if TABLE_ROW_RE.match(line) and not TABLE_RULE_RE.match(line):
            table_rows += 1
        buf.append(line.strip())
    flush()
    reduced, code_reduced = reduce_code(text, max_code_lines)
    refs = find_references(text)
    return {
        "chars": len(text), "chars_after_reduction": len(reduced), "code_reduced": code_reduced,
        "sections": {section_key(k): v for k, v in size["sections"].items()},
        "code_chars": size["code_chars"], "prose_chars": size["prose_chars"],
        "code_lines": code_lines, "prose_lines": prose_lines, "fenced_blocks": blocks,
        "bullets": bullets, "table_rows": table_rows, "references": refs,
        "no_issue_reference": not any(refs.values()), "paragraphs": paragraphs,
    }


def plan_too_large(item: dict, detail: str, max_code_lines: int = DEFAULT_MAX_CODE_LINES) -> JudgeError:
    """The plan-gate refusal. Oversize is a symptom: the error names where the characters are and carries the bead's shape."""
    size = size_breakdown(item["text"])
    where = ", ".join(f"{k} {v}" for k, v in sorted(size["sections"].items(), key=lambda kv: -kv[1]))
    exc = JudgeError("PLAN.TOO_LARGE",
                     f"plan bead {item['id']} is {len(item['text'])} characters after long code was reduced to signatures "
                     f"({size['prose_chars']} prose, {size['code_chars']} code; by section: {where}); {detail}. "
                     "A reasonable plan bead fits in one Jev request; this one is asking too much",
                     action="Oversize is a symptom; judge its cause before changing the bead. First remove work that no anchor, "
                            "requirement or user-approved issue asks for and the change does not need. Then cut prose and process "
                            "ceremony that does not serve a deliverable. Split only approved work that is still too large after "
                            "that. Never split, trim or raise a limit just to pass the gate. See references/plan-size-and-shape.md")
    shape = shape_text(item["text"], max_code_lines)
    shape.pop("paragraphs", None)
    shape["chars_before_reduction"] = item.get("chars_before_reduction", len(item["text"]))
    if "code_reduced" in item:
        shape["code_reduced"] = item["code_reduced"]
    exc.data = {"item_id": item["id"], "size": size, "shape": shape}
    exc.meta = {"size": size}
    return exc


def fit_plan_item(item: dict, misfit, max_code_lines: int) -> Result[dict, JudgeError]:
    """Plan gate: send the bead whole when it fits; otherwise reduce code as little as possible.

    `misfit(text)` returns None when the text fits one Jev request, else why not. Fenced blocks longer than
    max_code_lines are reduced to signatures one more at a time, longest first, until the bead fits. Prose is never
    cut; a bead that does not fit with every long block reduced is PLAN.TOO_LARGE. Records chars_before_reduction and
    code_reduced (what was actually reduced).
    """
    text = item["text"]
    item["chars_before_reduction"] = len(text)
    item["code_reduced"] = {"blocks": 0, "lines": 0}
    detail, chosen = misfit(text), set()
    for n in long_code_blocks(text, max_code_lines):
        if detail is None:
            break
        chosen.add(n)
        item["text"], item["code_reduced"] = reduce_code(text, max_code_lines, chosen)
        detail = misfit(item["text"])
    if detail is not None:
        return Err(plan_too_large(item, detail, max_code_lines))
    return Ok(item)


def beads_from_json(obj, what: str) -> Result[list, JudgeError]:
    """bd JSON in any of its shapes: one bead object, `bd show --json` (one-element array) or `bd children --json` (array)."""
    if isinstance(obj, dict):
        return Ok([obj])
    if isinstance(obj, list) and all(isinstance(b, dict) for b in obj):
        return Ok(obj)
    return fail("INPUT.MALFORMED", f"{what} must be a bead object or an array of bead objects", action="Pass unmodified `bd show --json` / `bd children --json` output.")


def load_beads(path: str, what: str) -> Result[list, JudgeError]:
    """Read one bd JSON file and return its beads."""
    loaded = load_json(path, what)
    if isinstance(loaded, Err):
        return loaded
    return beads_from_json(loaded.value, what)


SPRINT_TITLE_RE = re.compile(r"^[A-Za-z]+-\d+\b")   # "h-3: ..." style sprint titles
SPRINT_ID_RE = re.compile(r"-[a-z]+-\d+$")          # bead ids such as proj-h-3


def is_sprint_bead(show: dict, item_id: str, title: str) -> bool:
    kind = str(show.get("issue_type") or show.get("type") or "").lower()
    labels = show.get("labels") if isinstance(show.get("labels"), list) else []
    return (kind == "sprint" or any(str(lb).lower() == "stage:sprint" for lb in labels)
            or SPRINT_TITLE_RE.match(title) is not None or SPRINT_ID_RE.search(item_id) is not None
            or re.search(r"\bsprint\b", title, re.IGNORECASE) is not None)


def bead_shape(show: dict, max_code_lines: int) -> Result[dict, JudgeError]:
    built = compose_item_text(show)
    if isinstance(built, Err):
        return built
    composed = built.value
    shape = shape_text(composed["text"], max_code_lines)
    kind = str(show.get("issue_type") or show.get("type") or "")
    sections = shape["sections"]
    return Ok({"id": composed["item_id"], "title": composed["title"], "type": kind or None,
               "is_sprint": is_sprint_bead(show, composed["item_id"], composed["title"]),
               "description_chars": sections.get("description", 0), "acceptance_chars": sections.get("acceptance_criteria", 0),
               "design_ratio": round(sections.get("design", 0) / shape["chars"], 2) if shape["chars"] else 0.0, **shape})


def shape_beads(beads: list, max_code_lines: int, rec=None) -> Result[dict, JudgeError]:
    rows = []
    for b in beads:
        row = bead_shape(b, max_code_lines)
        if isinstance(row, Err):
            return row
        rows.append(row.value)
    seen = {}
    for r in rows:
        for p in r.pop("paragraphs"):
            norm = re.sub(r"\s+", " ", p).strip().lower()
            if len(norm) >= DUPLICATE_PARAGRAPH_MIN_CHARS:
                seen.setdefault(norm, [])
                if r["id"] not in seen[norm]:
                    seen[norm].append(r["id"])
    duplicated = [{"paragraph": norm[:80], "chars": len(norm), "beads": ids} for norm, ids in seen.items() if len(ids) >= 2]
    duplicated.sort(key=lambda d: (-len(d["beads"]), -d["chars"], d["paragraph"]))
    totals = {"beads": len(rows), "sprints": sum(1 for r in rows if r["is_sprint"]),
              "chars": sum(r["chars"] for r in rows), "chars_after_reduction": sum(r["chars_after_reduction"] for r in rows),
              "code_lines": sum(r["code_lines"] for r in rows), "prose_lines": sum(r["prose_lines"] for r in rows),
              "fenced_blocks": sum(r["fenced_blocks"] for r in rows), "bullets": sum(r["bullets"] for r in rows),
              "table_rows": sum(r["table_rows"] for r in rows), "no_issue_reference": sum(1 for r in rows if r["no_issue_reference"]),
              "duplicated_paragraphs": len(duplicated)}
    data = {"max_code_lines": max_code_lines, "beads": rows, "duplicated_paragraphs": duplicated, "totals": totals}
    if rec is not None:
        listed = anchor_list(rec)
        if isinstance(listed, Err):
            return listed
        known = {a.get("id") for a in listed.value if isinstance(a, dict)}
        active = {a.get("id") for a in listed.value if isinstance(a, dict) and a.get("status") != "retired"}
        referenced = set()
        for r in rows:
            r["unknown_anchor_references"] = [x for x in r["references"]["anc"] if x not in known]
            referenced.update(r["references"]["anc"])
        data["anchors_not_referenced"] = sorted(a for a in active if a not in referenced)
    return Ok(data)


def shape_md(data: dict) -> str:
    lines = ["## Plan shape", "", f"{data['totals']['beads']} bead(s), {data['totals']['sprints']} sprint(s), {data['totals']['chars']} chars "
             f"({data['totals']['chars_after_reduction']} after code reduction at {data['max_code_lines']} lines), "
             f"{data['totals']['no_issue_reference']} without an issue reference, {data['totals']['duplicated_paragraphs']} repeated paragraph(s).", "",
             "| Bead | Title | Chars | Desc chars | AC chars | Design ratio | After reduction | Code lines | Prose lines | Blocks | Bullets | Table rows | References |",
             "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    for r in data["beads"]:
        refs = ", ".join(x for kind in ("github", "req", "adr", "anc") for x in r["references"][kind]) or "none"
        lines.append(f"| {r['id']} | {r['title']} | {r['chars']} | {r['description_chars']} | {r['acceptance_chars']} | {r['design_ratio']:.2f} | "
                     f"{r['chars_after_reduction']} | {r['code_lines']} | {r['prose_lines']} | {r['fenced_blocks']} | {r['bullets']} | {r['table_rows']} | {refs} |")
    lines.append("")
    if data["duplicated_paragraphs"]:
        lines += ["### Paragraphs repeated across beads", "", "| Paragraph (first 80 chars) | Chars | Beads |", "| --- | --- | --- |"]
        for d in data["duplicated_paragraphs"]:
            lines.append(f"| {d['paragraph']} | {d['chars']} | {', '.join(d['beads'])} |")
        lines.append("")
    if data.get("anchors_not_referenced"):
        lines.append("Anchors no bead references: " + ", ".join(data["anchors_not_referenced"]))
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def cmd_shape(args) -> Result[Any, JudgeError]:
    if args.max_code_lines < 1:
        return fail("VALIDATION.INPUT", f"--max-code-lines must be a positive integer, got {args.max_code_lines}")
    sources = []
    if args.beads:
        d = pathlib.Path(args.beads)
        if not d.is_dir():
            return fail("INPUT.MISSING", f"--beads directory not found: {args.beads}", action="Pass a directory of `bd show --json` files.")
        sources += [(str(f), f"bead file {f.name}") for f in sorted(d.glob("*.json"))]
    if args.children_json:
        sources.append((args.children_json, "bd children output"))
    sources += [(path, "bd show output") for path in args.show_json or []]
    beads = []
    for path, what in sources:
        loaded = load_beads(path, what)
        if isinstance(loaded, Err):
            return loaded
        beads += loaded.value
    if not beads:
        return fail("INPUT.EMPTY", "no beads to shape", action="Pass --beads <dir>, --children-json <file> or --show-json <file>.")
    rec = None
    if args.anchors:
        loaded = load_record(args.anchors)
        if isinstance(loaded, Err):
            return loaded
        rec = loaded.value
    shaped = shape_beads(beads, args.max_code_lines, rec)
    if isinstance(shaped, Err):
        return shaped
    data = shaped.value
    if args.format == "md":
        return Ok(shape_md(data))
    return Ok({"success": True, "data": data, "error": None})


# --------------------------------------------------------------------------- #
# judging
# --------------------------------------------------------------------------- #
def check_roles(qs: dict, where: str) -> Result[None, JudgeError]:
    """Every required role, and the optional evidence role if present, must name a question in the set."""
    questions = qs.get("questions")
    if not isinstance(questions, dict) or not questions:
        return fail("INPUT.MALFORMED", f"{where}: 'questions' must be a non-empty object keyed by question id",
                    action="See assets/questions/plan.json for the shape.")
    roles = qs.get("roles")
    if not isinstance(roles, dict):
        return fail("QUESTIONS.ROLES", f"{where}: 'roles' must map relevance, compliance and conflict to question ids",
                    action="See references/verdict-gating.md for the role contract.")
    for required in REQUIRED_ROLES:
        if roles.get(required) not in questions:
            return fail("QUESTIONS.ROLES", f"{where} must map role '{required}' to a question id in the set",
                        action="See references/verdict-gating.md for the role contract.")
    evidence = roles.get("evidence")
    if evidence is not None and evidence not in questions:
        return fail("QUESTIONS.ROLES", f"{where} maps role 'evidence' to '{evidence}', which is not a question in the set",
                    action="Keep the default question ids, or update roles to match the renamed question.")
    return Ok(None)


def load_question_set(gate: str, override_path: str = "") -> Result[dict, JudgeError]:
    path = override_path or str(QUESTIONS_DIR / f"{gate}.json")
    loaded = load_json(path, f"question set for gate '{gate}'")
    if isinstance(loaded, Err):
        return loaded
    qs = loaded.value
    if not isinstance(qs, dict):
        return fail("INPUT.MALFORMED", f"question set {path} must be a JSON object")
    checked = check_roles(qs, f"question set {path}")
    if isinstance(checked, Err):
        return checked
    return Ok(qs)


def load_extra_questions(paths: list, base: dict) -> Result[tuple, JudgeError]:
    """Add consumer questions from JSONL files to the base set. Ok((merged_set, sources)).

    Each non-blank line is one question object with an `id` plus the shape of a question in the base set.
    Extra questions are asked and recorded with every cell but never gate: no role may map to one.
    """
    merged = json.loads(json.dumps(base))
    sources = []
    for path in paths or []:
        p = pathlib.Path(path)
        if not p.is_file():
            return fail("INPUT.MISSING", f"extra questions not found: {path!r}", action="Pass an existing JSONL file to --extra-questions.")
        try:
            raw = p.read_bytes()
            text = raw.decode("utf-8")
        except (OSError, UnicodeDecodeError) as exc:
            return fail("INPUT.MALFORMED", f"extra questions could not be read: {exc}", action=f"Check {path} is a readable UTF-8 file.")
        ids = []
        for n, line in enumerate(text.splitlines(), 1):
            if not line.strip():
                continue
            where = f"{path}:{n}"
            try:
                q = json.loads(line)
            except json.JSONDecodeError as e:
                return fail("INPUT.MALFORMED", f"{where}: not a JSON object ({e.msg})", action="Write one question object per line.")
            if not isinstance(q, dict):
                return fail("INPUT.MALFORMED", f"{where}: not a JSON object", action="Write one question object per line.")
            qid = q.pop("id", None)
            if not isinstance(qid, str) or not qid.strip():
                return fail("INPUT.MALFORMED", f"{where}: missing a non-empty string 'id'", action="Give every extra question a unique id.")
            if qid in merged["questions"]:
                return fail("INPUT.MALFORMED", f"{where}: question id '{qid}' is already in the question set",
                            action="Extra questions add to the set; to replace a question use an override instead.")
            if not isinstance(q.get("instructions"), str) or not q.get("type"):
                return fail("INPUT.MALFORMED", f"{where}: '{qid}' needs 'type' and string 'instructions'",
                            action="Copy the shape of a question in assets/questions/<gate>.json.")
            merged["questions"][qid] = q
            ids.append(qid)
        sources.append({"path": str(path), "sha256": hashlib.sha256(raw).hexdigest(), "ids": ids})
    return Ok((merged, sources))


def triage_questions_path(args, gate: str) -> str:
    """The triage questions judge adds by default: the repository's .sc/plan-anchor/triage/<gate>-questions.jsonl
    (nearest at or above --repository-root, stopping before home), else the shipped default; "" when there are none."""
    import plan_anchor_paths  # stdlib-only sibling
    name = f"{gate}-questions.jsonl"
    start = pathlib.Path(getattr(args, "repository_root", "") or ".").resolve()
    settings = plan_anchor_paths.find_settings(start)
    if settings is not None:
        local = settings.parent / "triage" / name
        if local.is_file():
            return str(local)
    shipped = TRIAGE_ASSETS / name
    return str(shipped) if shipped.is_file() else ""


def check_extra_not_gating(qs: dict, extra_ids: set, where: str) -> Result[None, JudgeError]:
    """An extra question never gates: refuse a role that maps to one (for example through an override)."""
    for role, qid in (qs.get("roles") or {}).items():
        if qid in extra_ids:
            return fail("QUESTIONS.ROLES", f"{where} maps role '{role}' to extra question '{qid}'; extra questions never gate",
                        action="Map the role to a question in the gate's set, or replace that question with an override.")
    return Ok(None)


def merge_overrides(base: dict, override: dict) -> Result[dict, JudgeError]:
    """Per-anchor overrides replace question text by id; roles stay as in base unless overridden."""
    merged = json.loads(json.dumps(base))
    if not isinstance(override, dict):
        return fail("INPUT.MALFORMED", "a question override must be an object with 'questions' and optional 'roles'")
    questions = override.get("questions") or {}
    if not isinstance(questions, dict):
        return fail("INPUT.MALFORMED", "override 'questions' must be an object keyed by question id")
    for qid, q in questions.items():
        merged["questions"][qid] = q
    roles = override.get("roles")
    if roles:
        if not isinstance(roles, dict):
            return fail("INPUT.MALFORMED", "override 'roles' must be an object")
        merged.setdefault("roles", {}).update(roles)
    return Ok(merged)


def questions_digest(questions: dict) -> str:
    canonical = json.dumps(questions, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return hashlib.sha256(canonical).hexdigest()


def build_state(anchor: dict, item: dict) -> dict:
    return {
        "anchor": {
            "id": anchor["id"],
            "statement": anchor["statement"],
            "scope": anchor.get("scope") or "",
        },
        "item": {
            "id": item["id"],
            "title": item.get("title") or "",
            "text": item["text"],
        },
    }


def encode_body(body: dict) -> bytes:
    return json.dumps(body, allow_nan=False).encode("utf-8")


def parse_retry_after(value):
    if value is None:
        return None
    try:
        delay = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(delay) or delay < 0:
        return None
    return min(delay, MAX_RETRY_AFTER)


def http_error(status: int, text: str, retry_after=None) -> JudgeError:
    """Map an HTTP failure to a JudgeError. Bodies are withheld except the first 300 chars of a 422."""
    low = text.lower()
    if 400 <= status < 500 and ("max_tokens" in low or ("token" in low and "exceed" in low)):
        return JudgeError("JEV.CONTEXT_EXCEEDED", f"Jev HTTP {status}: request exceeds the model's context window", recoverable=True,
                          action="Shorten the evidence for this item.", status=status)
    if status == 401:
        return JudgeError("AUTH.CREDENTIALS", "Jev HTTP 401; response body withheld", action="Check TYPESAFE_API_KEY.", status=status)
    if status == 422:
        return JudgeError("JEV.VALIDATION", f"Jev HTTP 422: {text[:300]}", action="Fix the named field; usually a malformed question override.", status=status)
    code = {429: "JEV.RATE_LIMITED", 529: "JEV.OVERLOADED"}.get(status, "JEV.HTTP")
    transient = status in (429, 529) or status >= 500
    return JudgeError(code, f"Jev HTTP {status}; response body withheld", recoverable=transient,
                      action="Retry with backoff" if transient else "Check the request body.",
                      status=status, retry_after=retry_after)


def call_jev(body: dict, api_key: str, timeout: float) -> Result[dict, JudgeError]:
    """One POST to Jev. Returns Err(JudgeError) on failure; never retries or shortens (judge_with_policy does both)."""
    data = encode_body(body)
    req = urllib.request.Request(
        JEV_URL, data=data, method="POST",
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json", "User-Agent": "plan-anchoring/jev_judge"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read(MAX_RESPONSE_BYTES + 1)
    except urllib.error.HTTPError as exc:
        text = exc.read(4096).decode("utf-8", "replace")
        retry_after = parse_retry_after(exc.headers.get("Retry-After") if exc.headers is not None else None)
        return Err(http_error(exc.code, text, retry_after))
    except urllib.error.URLError as exc:
        return fail("JEV.NETWORK", f"Could not reach Jev: {exc.reason}", recoverable=True, action="Check network access and retry.")
    except (OSError, http.client.HTTPException) as exc:
        return fail("JEV.NETWORK", f"Jev connection failed: {type(exc).__name__}: {exc}", recoverable=True, action="Check network access and retry.")
    if len(raw) > MAX_RESPONSE_BYTES:
        return fail("JEV.RESPONSE_INVALID", "Jev response exceeded the size limit", recoverable=True, action="Retry once; if it repeats, check for a proxy rewriting the response.")
    try:
        parsed = json.loads(raw.decode("utf-8"))
    except (ValueError, UnicodeError):
        return fail("JEV.RESPONSE_INVALID", "Jev response was not JSON", recoverable=True, action="Retry once; if it repeats, check for a proxy rewriting the response.")
    if not isinstance(parsed, dict):
        return fail("JEV.RESPONSE_INVALID", "Jev response was not a JSON object", recoverable=True, action="Check for a proxy rewriting the response.")
    return Ok(parsed)


def is_transient(exc: JudgeError) -> bool:
    if exc.code in TRANSIENT_CODES:
        return True
    return exc.code == "JEV.HTTP" and exc.status is not None and exc.status >= 500


def item_core(text: str) -> str:
    return text[: -len(TRUNCATION_MARKER)] if text.endswith(TRUNCATION_MARKER) else text


def halve_item(item: dict, min_item_chars: int, meta: dict) -> bool:
    """Keep the head of the item text and re-append the truncation marker. False when already at the floor."""
    core = item_core(item["text"])
    if len(core) <= min_item_chars:
        return False
    item["text"] = core[: max(len(core) // 2, min_item_chars)] + TRUNCATION_MARKER
    meta["item_shortened"] = True
    meta["item_chars_sent"] = len(item["text"])
    return True


def fit_request(body: dict, max_request_bytes: int, min_item_chars: int, meta: dict) -> Result[None, JudgeError]:
    """Pre-flight cap: shorten the item until the serialised request (state + questions) is under max_request_bytes.

    A state without an item (dedupe, rubric) has nothing to shorten: over the cap it is refused outright.
    """
    item = body["state"].get("item")
    size = len(encode_body(body))
    while size > max_request_bytes:
        if item is None:
            exc = JudgeError("JEV.CONTEXT_EXCEEDED", f"request is {size} bytes, over --max-request-bytes {max_request_bytes}, and has no item to shorten",
                             action="Shorten the anchor statement, quote or scope, or raise --max-request-bytes.")
            exc.meta = meta
            return Err(exc)
        if not halve_item(item, min_item_chars, meta):
            exc = JudgeError("JEV.CONTEXT_EXCEEDED",
                             f"request is {size} bytes with the item at {len(item['text'])} chars, over --max-request-bytes {max_request_bytes}; "
                             f"the item is already at --min-item-chars {min_item_chars}",
                             action="Shorten the evidence for this item (names, counts and summaries only), shorten the question overrides, or raise --max-request-bytes.")
            exc.meta = meta
            return Err(exc)
        size = len(encode_body(body))
    meta["request_bytes"] = size
    return Ok(None)


def judge_with_policy(body: dict, api_key: str, timeout: float, max_request_bytes: int, min_item_chars: int, questions: dict,
                      shorten: bool = True, optional=frozenset()) -> Result[tuple, JudgeError]:
    """call_jev with the attempt policy and context handling. Returns Ok((validated response, meta)) or Err(JudgeError carrying .meta).

    - Transport-level failures (429, 529, 5xx, network, invalid response): up to MAX_ATTEMPTS attempts; the first
      retry waits Retry-After when given, else RETRY_DELAYS[0]; the next waits RETRY_DELAYS[1].
    - Context overflow (4xx mentioning max_tokens / token ... exceed): halve the item and retry until it fits or
      the item is at min_item_chars, then give up with JEV.CONTEXT_EXCEEDED. With shorten=False (the plan gate)
      the item is never cut: the overflow is returned at once as PLAN.TOO_LARGE.
    - Everything else (401, 422) is returned at once.
    """
    item = body["state"].get("item")
    meta = {"attempts": 0, "item_chars_sent": len(item["text"]) if item else 0, "item_shortened": False}
    fitted = fit_request(body, max_request_bytes, min_item_chars, meta)
    if isinstance(fitted, Err):
        return fitted
    transient_failures = 0
    while True:
        meta["attempts"] += 1
        meta["item_chars_sent"] = len(item["text"]) if item else 0
        answered = call_jev(body, api_key, timeout)
        if isinstance(answered, Ok):
            answered = validate_response(answered.value, questions, optional)
            if isinstance(answered, Ok):
                return Ok((answered.value, meta))
        exc = answered.error
        exc.meta = meta
        if exc.code == "JEV.CONTEXT_EXCEEDED" and item is not None and not shorten:
            final = plan_too_large(item, f"Jev rejected it for context size ({exc.message})")
            final.meta = dict(meta, **final.meta)
            return Err(final)
        if exc.code == "JEV.CONTEXT_EXCEEDED":
            if item is not None and halve_item(item, min_item_chars, meta):
                meta["request_bytes"] = len(encode_body(body))
                continue
            final = JudgeError("JEV.CONTEXT_EXCEEDED",
                               f"Jev rejected the request for context size with the item at {len(item['text']) if item else 0} chars "
                               f"(floor --min-item-chars {min_item_chars}): {exc.message}",
                               action="Shorten the evidence for this item: sprint report and PR body summaries, file names and counts only; or lower --min-item-chars.",
                               status=exc.status)
            final.meta = meta
            return Err(final)
        if is_transient(exc) and transient_failures < MAX_ATTEMPTS - 1:
            delay = RETRY_DELAYS[min(transient_failures, len(RETRY_DELAYS) - 1)]
            if transient_failures == 0 and exc.retry_after is not None:
                delay = exc.retry_after
            transient_failures += 1
            _sleep(delay)
            continue
        return answered


def is_probability(value) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and 0 <= value <= 1


def noul_probability(ans: dict):
    for key in ("noul", "probability", "p"):
        if key in ans:
            return ans[key]
    return None


def answer_problem(ans, question) -> str:
    """Why an answer cannot be gated, or None when it can (modelled on process-porn's validator, minus the model check)."""
    if not isinstance(ans, dict):
        return "answer is not an object"
    kind = ans.get("type", "choice")
    if kind == "noul":
        if not is_probability(noul_probability(ans)):
            return "noul probability is missing or not in [0, 1]"
        return None
    if kind != "choice":
        return f"answer type {kind!r} is not choice or noul"
    criteria = question.get("criteria") if isinstance(question, dict) else None
    options = set(criteria) if isinstance(criteria, dict) else set()
    if "choice" not in ans:
        return "choice answer has no 'choice'"
    if options and ans["choice"] not in options:
        return f"choice {ans['choice']!r} is not one of {sorted(options)}"
    if not is_probability(ans.get("confidence")):
        return "confidence is missing or not in [0, 1]"
    probs = ans.get("probabilities")
    if not isinstance(probs, dict):
        return "probabilities missing"
    if options and set(probs) != options:
        return "probability keys differ from the options"
    if not all(is_probability(p) for p in probs.values()):
        return "a probability is out of range"
    total = sum(probs.values())
    if abs(total - 1) > PROBABILITY_ROUNDING * max(len(probs), 1):
        return f"probabilities sum to {total:.4f}"
    return None


def validate_response(resp: dict, questions: dict, optional=frozenset()) -> Result[dict, JudgeError]:
    """Every question must be answered except the ``optional`` (extra, never-gating) ones; no unknown ids."""
    if not isinstance(resp, dict) or not isinstance(resp.get("answers"), dict):
        return fail("JEV.RESPONSE_INVALID", "Jev response has no 'answers' object", recoverable=True,
                    action="Retry; if it repeats, check the --model name.")
    answers = resp["answers"]
    required = set(questions) - set(optional)
    if not required <= set(answers) or not set(answers) <= set(questions):
        missing, extra = sorted(required - set(answers)), sorted(set(answers) - set(questions))
        return fail("JEV.RESPONSE_INVALID", f"Jev answer ids differ from the questions (missing {missing}, unexpected {extra})", recoverable=True,
                    action="Retry; if it repeats, check the question ids in the overrides.")
    for qid, q in questions.items():
        if qid not in answers:
            continue  # an optional question left unanswered
        problem = answer_problem(answers[qid], q)
        if problem:
            return fail("JEV.RESPONSE_INVALID", f"invalid answer for {qid}: {problem}", recoverable=True,
                        action="Retry; if it repeats, the question set or the model changed shape.")
    return Ok(resp)


def answer_choice(ans: dict) -> tuple:
    """Return (choice, confidence) for a choice answer; tolerate noul answers."""
    if ans.get("type") == "noul" or "noul" in ans:
        p = float(noul_probability(ans) or 0.0)
        return ("yes" if p >= 0.5 else "no", abs(p - 0.5) * 2)
    return (ans.get("choice", "unclear"), float(ans.get("confidence", 0.0)))


def role_answers(answers: dict, roles: dict) -> dict:
    """{role: (choice, confidence)} for every role that maps to an answered question."""
    out = {}
    for role, qid in (roles or {}).items():
        if qid and qid in answers:
            out[role] = answer_choice(answers[qid])
    return out


def decide(answers: dict, roles: dict, threshold: float, kind: str = "directive") -> tuple:
    """Turn independent Jev answers into one verdict, gating in code (Jev cannot see its own answers).

    directive (and structural at the dev gate): relevance, compliance and conflict all gate, as in the table in
    references/verdict-gating.md. prohibition: the compliance role is ignored, because the default `addressed`
    question answers no for every bead that simply refrains (10 of 10 beads in the measured experiments): not_applicable
    when relevance is no, contradicted when conflict is yes, underspecified when relevance is unclear/low or the
    conflict answer is not a confident no, else satisfied (the dev gate's evidence role still yields asserted_only).
    With threshold 0 the result is the verdict by choices alone: the `leaning`.
    """
    got = role_answers(answers, roles)

    def get(role):
        return got.get(role, (None, 0.0))

    rel, rel_c = get("relevance")
    comp, comp_c = get("compliance")
    conf, conf_c = get("conflict")
    evid, evid_c = get("evidence")

    if kind == "prohibition":
        min_conf = min(rel_c, conf_c)
        if rel == "no" and rel_c >= threshold:
            return "not_applicable", min_conf
        if conf == "yes" and conf_c >= threshold:
            return "contradicted", min_conf
        if rel == "unclear" or rel_c < threshold or conf != "no" or conf_c < threshold:
            return "underspecified", min_conf
        if roles.get("evidence") and evid == "no" and evid_c >= threshold:
            return "asserted_only", min_conf
        return "satisfied", min_conf

    min_conf = min(rel_c, comp_c, conf_c)
    if rel == "no" and rel_c >= threshold:
        return "not_applicable", min_conf
    if conf == "yes" and conf_c >= threshold:
        return "contradicted", min_conf
    if rel == "unclear" or rel_c < threshold or comp == "unclear" or comp_c < threshold:
        return "underspecified", min_conf
    if comp == "no":
        return "not_addressed", min_conf
    if comp == "yes" and conf == "no" and conf_c >= threshold:  # an unsure conflict answer never passes
        if roles.get("evidence") and evid == "no" and evid_c >= threshold:
            return "asserted_only", min_conf
        return "satisfied", min_conf
    return "underspecified", min_conf


def low_questions_of(entry: dict, roles: dict, threshold: float) -> list:
    """The gating answers that made a cell underspecified: an `unclear` choice or a confidence under the threshold.

    One entry per low answer, tagged with the ask ("second" is an unstable cell's re-ask). They tell the planner
    which question Jev could not answer firmly, so the bead is sharpened where it is weak.
    """
    out = []
    for ask, key in (("first", "answers"), ("second", "answers_second")):
        answers = entry.get(key)
        if not isinstance(answers, dict):
            continue
        for role in ("relevance", "compliance", "conflict"):
            if role == "compliance" and entry.get("kind") == "prohibition":
                continue
            qid = (roles or {}).get(role)
            if not qid or qid not in answers:
                continue
            choice, conf = answer_choice(answers[qid])
            if choice == "unclear" or conf < threshold:
                out.append({"ask": ask, "role": role, "question_id": qid, "choice": choice, "confidence": round(conf, 3)})
    return out


def leaning_of(answers: dict, roles: dict, kind: str) -> str:
    """The verdict by choices alone (threshold 0), so an underspecified cell still shows which way it leans."""
    return decide(answers, roles, 0.0, kind)[0]


def near_threshold(answers: dict, roles: dict, threshold: float, kind: str) -> bool:
    """True when any gating role's confidence is within NEAR_THRESHOLD_BAND of the threshold."""
    got = role_answers(answers, roles)
    gating = [r for r in ("relevance", "compliance", "conflict", "evidence") if r in got and not (kind == "prohibition" and r == "compliance")]
    return any(abs(got[r][1] - threshold) <= NEAR_THRESHOLD_BAND + 1e-9 for r in gating)


def explain(verdict: str, min_conf: float, gate: str) -> str:
    noun = "bead" if gate == "plan" else "work"
    return {
        "deterministic_only": "Structural anchor: verified by its deterministic_check, not by Jev",
        "contradicted": f"This {noun} proposes something the anchor rules out." if gate == "plan" else "The delivered work went against this anchor.",
        "not_addressed": f"This {noun} does not carry the directive forward." if gate == "plan" else "The delivered work does not show the directive was followed.",
        "asserted_only": "Compliance is asserted but no concrete evidence (paths, diffs, commands, tests) is cited.",
        "underspecified": f"Jev was not confident (min confidence {min_conf:.2f}); the anchor or the {noun} may be underspecified.",
        "satisfied": "The directive is carried forward." if gate == "plan" else "The directive was followed and evidenced.",
        "not_applicable": f"The anchor does not govern this {noun}.",
        "error": "Judging failed; see error.",
    }[verdict]


def read_item(args) -> Result[dict, JudgeError]:
    if args.item_file:
        try:
            text = pathlib.Path(args.item_file).read_text(encoding="utf-8")
        except FileNotFoundError:
            return fail("INPUT.MISSING", f"item file not found: {args.item_file}", action="Write the bead or work description to a file and pass it with --item-file.")
        except (OSError, UnicodeDecodeError) as exc:
            return fail("INPUT.MALFORMED", f"item file could not be read: {exc}", action="Pass a readable UTF-8 text file with --item-file.")
    else:
        text = sys.stdin.read()
    if not text.strip():
        return fail("INPUT.EMPTY", "item text is empty", action="Pass the bead or work description via --item-file.")
    item = {"id": args.item_id, "title": args.item_title or "", "truncated": False, "source": args.item_file or "stdin"}
    if getattr(args, "gate", "") == "plan":
        # A plan bead is never truncated; cmd_judge fits it (fit_plan_item) once the anchors and questions are known.
        item["text"] = text
        return Ok(item)
    if len(text) > args.max_item_chars:
        text = text[: args.max_item_chars] + TRUNCATION_MARKER
        item["truncated"] = True
    item["text"] = text
    return Ok(item)


def _command_credential(argv: list[str]) -> str:
    """Read a credential from a platform command without exposing its output on failure."""
    try:
        proc = subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                              text=True, encoding="utf-8", timeout=5, check=False)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return proc.stdout.strip() if proc.returncode == 0 else ""


def _windows_credential() -> str:
    """Read the documented generic credential target from Windows Credential Manager."""
    try:
        import ctypes
        from ctypes import wintypes

        class Credential(ctypes.Structure):
            _fields_ = [
                ("Flags", wintypes.DWORD), ("Type", wintypes.DWORD), ("TargetName", wintypes.LPWSTR),
                ("Comment", wintypes.LPWSTR), ("LastWritten", wintypes.FILETIME),
                ("CredentialBlobSize", wintypes.DWORD), ("CredentialBlob", ctypes.c_void_p),
                ("Persist", wintypes.DWORD), ("AttributeCount", wintypes.DWORD), ("Attributes", ctypes.c_void_p),
                ("TargetAlias", wintypes.LPWSTR), ("UserName", wintypes.LPWSTR),
            ]

        credential = ctypes.POINTER(Credential)()
        advapi = ctypes.windll.advapi32
        advapi.CredReadW.argtypes = (wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                                     ctypes.POINTER(ctypes.POINTER(Credential)))
        advapi.CredReadW.restype = wintypes.BOOL
        advapi.CredFree.argtypes = (ctypes.c_void_p,)
        if not advapi.CredReadW(WINDOWS_CREDENTIAL_TARGET, 1, 0, ctypes.byref(credential)):
            return ""
        try:
            size = int(credential.contents.CredentialBlobSize)
            if not size or not credential.contents.CredentialBlob:
                return ""
            return ctypes.string_at(credential.contents.CredentialBlob, size).decode("utf-16-le").strip()
        finally:
            advapi.CredFree(credential)
    except (AttributeError, OSError, UnicodeDecodeError):
        return ""


def credential_store_api_key() -> str:
    """Read the documented OS credential only when the environment has no key."""
    if sys.platform == "darwin":
        return _command_credential(["security", "find-generic-password", "-s", CREDENTIAL_SERVICE,
                                    "-a", CREDENTIAL_ACCOUNT, "-w"])
    if sys.platform == "win32":
        return _windows_credential()
    if sys.platform.startswith("linux"):
        return _command_credential(["secret-tool", "lookup", "service", CREDENTIAL_SERVICE,
                                    "account", CREDENTIAL_ACCOUNT])
    return ""


def resolved_api_key() -> str:
    """Resolve the TypeSafe key from the environment first, then the platform credential store."""
    environment_key = os.environ.get(TYPESAFE_KEY_ENV)
    if environment_key is not None:
        return environment_key.strip()
    return credential_store_api_key()


def check_api_key(api_key: str) -> Result[None, JudgeError]:
    if not api_key:
        return fail("AUTH.CREDENTIALS", "no TypeSafe API key was found", action="Export TYPESAFE_API_KEY or add the documented platform credential, or use --dry-run / --answers-file.")
    if not api_key.isascii() or any(ord(c) <= 32 or ord(c) == 127 for c in api_key):
        return fail("AUTH.CREDENTIALS", "the TypeSafe API key has invalid formatting (whitespace, control or non-ASCII characters)",
                    action="Replace TYPESAFE_API_KEY or the platform credential with the key exactly as issued.")
    return Ok(None)


def judge_context(args, rec: dict, remote: bool = True, triage: bool = False) -> Result[dict, JudgeError]:
    """Shared pre-flight for judge and check-fixtures: options, question set, overrides, fixture answers, key and consent.

    remote=False (every selected anchor is excluded from the item) skips the key and consent check: nothing is sent.
    """
    threshold = getattr(args, "threshold", DEFAULT_THRESHOLD)
    if not (0.0 <= threshold <= 1.0):
        return fail("VALIDATION.INPUT", f"--threshold must be between 0 and 1, got {threshold}")
    ctx = {
        "gate": args.gate, "model": getattr(args, "model", DEFAULT_MODEL), "threshold": threshold,
        "timeout": getattr(args, "timeout", 30.0),
        "max_item_chars": getattr(args, "max_item_chars", DEFAULT_MAX_ITEM_CHARS),
        "max_request_bytes": getattr(args, "max_request_bytes", None),
        "min_item_chars": getattr(args, "min_item_chars", DEFAULT_MIN_ITEM_CHARS),
        "dry_run": bool(getattr(args, "dry_run", False)),
        "max_code_lines": getattr(args, "max_code_lines", DEFAULT_MAX_CODE_LINES),
        "confirm": confirm_enabled(args),
    }
    if ctx["max_request_bytes"] is None:
        ctx["max_request_bytes"] = DEFAULT_MAX_REQUEST_BYTES if args.gate == "plan" else DEFAULT_MAX_REQUEST_BYTES_DEV
    for name in ("max_item_chars", "max_request_bytes", "min_item_chars", "max_code_lines"):
        if ctx[name] < 1:
            return fail("VALIDATION.INPUT", f"--{name.replace('_', '-')} must be a positive integer, got {ctx[name]}")
    loaded = load_question_set(args.gate, getattr(args, "questions", ""))
    if isinstance(loaded, Err):
        return loaded
    extra_paths = list(getattr(args, "extra_questions", None) or [])
    triage_default = ""
    if triage and not getattr(args, "no_triage_questions", False):
        triage_default = triage_questions_path(args, args.gate)
        if triage_default:
            extra_paths.append(triage_default)
    extra = load_extra_questions(extra_paths, loaded.value)
    if isinstance(extra, Err):
        return extra
    ctx["base_qs"], ctx["extra_questions"] = extra.value
    # The triage questions are asked in a second request, and only for a cell that failed, so the gate request (and so every
    # verdict) is exactly what it is without them. Questions from --extra-questions stay in the gate request.
    ctx["deferred_ids"] = set(ctx["extra_questions"][-1]["ids"]) if triage_default else set()
    loaded = load_answers_fixture(getattr(args, "answers_file", ""), "anchor id")
    if isinstance(loaded, Err):
        return loaded
    ctx["answers_fixture"] = loaded.value
    overrides_file = getattr(args, "overrides_file", "")
    ctx["bead_overrides"] = {}
    if overrides_file:
        loaded = load_json(overrides_file, "overrides file")
        if isinstance(loaded, Err):
            return loaded
        ctx["bead_overrides"] = loaded.value
    if not isinstance(ctx["bead_overrides"], dict):
        return fail("INPUT.MALFORMED", "overrides file must be an object keyed by anchor id")
    ctx["live"] = not ctx["dry_run"] and ctx["answers_fixture"] is None
    if ctx["live"] and remote:
        cleared = remote_preflight(args, "The anchor statement and the item text")
        if isinstance(cleared, Err):
            return cleared
        ctx["api_key"] = cleared.value
    else:
        ctx["api_key"] = os.environ.get(TYPESAFE_KEY_ENV, "").strip()
    return Ok(ctx)


def confirm_enabled(args) -> bool:
    """--no-confirm wins; otherwise on (the default; --confirm-near-threshold states it explicitly)."""
    return not getattr(args, "no_confirm", False)


REMOTE_JUDGE_SETTING = ".sc/plan-anchor/settings.yaml"
REPO_ROOT_HELP = ("any path inside the repository; the nearest .sc/plan-anchor/settings.yaml at or above it holds "
                  "allow_remote_judge (read for live Jev calls only; default .)")
REMOTE_JUDGE_SET = "python3 scripts/plan_anchor_settings.py set allow_remote_judge true|false"


def remote_judge_consent(args, what: str) -> Result[None, JudgeError]:
    """The repository's committed answer to whether plan text may go to TypeSafe: ``allow_remote_judge`` in
    .sc/plan-anchor/settings.yaml. Only ``true`` passes; ``false`` and an absent answer refuse.

    The settings module (and PyYAML behind it) is imported only here, so only a live call needs it.
    """
    import plan_anchor_settings as settings  # lazy: dry runs and fixture answers stay stdlib-only

    start = pathlib.Path(getattr(args, "repository_root", "") or ".")
    root = settings.find_repo_root(start)  # consent is a repository's answer: outside one there is none
    if not root.ok:
        return Err(_from_tool_error(root.error))
    # The nearest settings file at or above the given path wins (REQ-070), not the one at the git top level:
    # an install in a folder inside the repository writes its settings there.
    answer = settings.remote_judge_consent(start.resolve())
    if answer.ok and answer.value is True:
        return Ok(None)
    disabled = f"Sending plan text to TypeSafe is disabled. {what} would leave this machine."
    if answer.ok:  # False: the user said no
        return fail("POLICY.REMOTE_JUDGE_DISABLED",
                    f"{disabled} {REMOTE_JUDGE_SETTING} sets allow_remote_judge: false.",
                    action="The user declined remote judging; only the user changes it (`python3 scripts/plan_anchor_settings.py "
                           "set allow_remote_judge true`, then commit). Use --dry-run to inspect the request instead.")
    if answer.error.code == "SETTINGS.REMOTE_JUDGE_UNSET":
        return fail("POLICY.REMOTE_JUDGE_DISABLED",
                    f"{disabled} {REMOTE_JUDGE_SETTING} does not set allow_remote_judge.",
                    action="Ask the user once whether plan text may be sent to TypeSafe (Jev) for judging, record the answer "
                           f"with `{REMOTE_JUDGE_SET}`, and commit {REMOTE_JUDGE_SETTING}. Never infer consent from "
                           "existing run files or past runs. Use --dry-run to inspect the request instead.")
    return Err(_from_tool_error(answer.error))  # DEPENDENCY.PYYAML, SETTINGS.*: never a silent default


def _from_tool_error(error) -> JudgeError:
    out = JudgeError(error.code, error.message, action=error.action or "")
    out.data = dict(error.data) if error.data is not None else None
    return out


def remote_preflight(args, what: str) -> Result[str, JudgeError]:
    """Key and consent checks shared by every live Jev call; returns the API key. Nothing leaves the machine before this passes."""
    api_key = resolved_api_key()
    checked = check_api_key(api_key)
    if isinstance(checked, Err):
        return checked
    consent = remote_judge_consent(args, what)
    if isinstance(consent, Err):
        return consent
    return Ok(api_key)


def load_answers_fixture(path: str, keyed_by: str) -> Result[Any, JudgeError]:
    """A fixture of Jev answers keyed by id (or '*'); Ok(None) when no path is given."""
    if not path:
        return Ok(None)
    loaded = load_json(path, "answers fixture")
    if isinstance(loaded, Err):
        return loaded
    fixture = loaded.value
    if not isinstance(fixture, dict):
        return fail("INPUT.MALFORMED", f"answers fixture must be an object keyed by {keyed_by} (or '*')")
    return Ok(fixture)


def load_simple_question_set(name: str, required: tuple) -> Result[dict, JudgeError]:
    """A question set without roles (dedupe, testability): every named question must be present."""
    path = str(QUESTIONS_DIR / f"{name}.json")
    loaded = load_json(path, f"question set '{name}'")
    if isinstance(loaded, Err):
        return loaded
    qs = loaded.value
    questions = qs.get("questions") if isinstance(qs, dict) else None
    if not isinstance(questions, dict) or any(q not in questions for q in required):
        return fail("INPUT.MALFORMED", f"question set {path} must hold the questions {list(required)}",
                    action=f"Restore assets/questions/{name}.json from the skill.")
    return Ok(qs)


def ask_jev_once(args, api_key: str, body: dict, questions: dict, fixture, fixture_keys: tuple) -> Result[tuple, JudgeError]:
    """One request for the item-less subcommands (dedupe, rubric): fixture answers when given, else the live policy.

    Returns Ok((validated response, meta)), or Err(JudgeError) (FIXTURE.MISSING when the fixture lacks every key).
    """
    if fixture is not None:
        fx = None
        for key in fixture_keys:
            fx = fixture.get(key)
            if fx is not None:
                break
        if fx is None:
            return fail("FIXTURE.MISSING", f"answers fixture has no entry for {fixture_keys[0]} (or '*')",
                        action=f"Add an entry for {fixture_keys[0]} (or '*') to the answers file.")
        checked = validate_response({"model": "fixture", "answers": fx}, questions)
        if isinstance(checked, Err):
            return checked
        return Ok((checked.value, {"attempts": 1}))
    return judge_with_policy(body, api_key, getattr(args, "timeout", 30.0), DEFAULT_MAX_REQUEST_BYTES, DEFAULT_MIN_ITEM_CHARS, questions)


def prepare_question_sets(ctx: dict, anchors: list) -> Result[list, JudgeError]:
    """Resolve every anchor's question set before any call, so a bad override aborts before anything leaves the machine."""
    prepared = []
    for a in anchors:
        record_override = (a.get("question_overrides") or {}).get(ctx["gate"]) or {}
        bead_override = ctx["bead_overrides"].get(a["id"]) or {}
        merged = merge_overrides(ctx["base_qs"], record_override)
        if isinstance(merged, Err):
            return merged
        merged = merge_overrides(merged.value, bead_override)
        if isinstance(merged, Err):
            return merged
        qs = merged.value
        checked = check_roles(qs, f"question set for {a['id']} after overrides")
        if isinstance(checked, Err):
            return checked
        extra_ids = {i for s in ctx.get("extra_questions") or [] for i in s["ids"]}
        checked = check_extra_not_gating(qs, extra_ids, f"question set for {a['id']} after overrides")
        if isinstance(checked, Err):
            return checked
        overridden = any(o.get(k) for o in (record_override, bead_override) for k in ("questions", "roles"))
        qs["optional"] = sorted(extra_ids & set(qs["questions"]))
        qs["deferred"] = sorted(set(qs["optional"]) & set(ctx.get("deferred_ids") or ()))
        prepared.append((a, qs, bool(overridden)))
    return Ok(prepared)


def _first_answer(ctx: dict, a: dict, qs: dict, body: dict, meta: dict, fixture_key) -> Result[tuple, JudgeError]:
    """The first answer for one anchor: the fixture entry when a fixture is given, else the live policy. Ok((response, meta))."""
    if ctx["answers_fixture"] is not None:
        fx = ctx["answers_fixture"].get(fixture_key(a)) or ctx["answers_fixture"].get("*")
        if fx is None:
            return fail("FIXTURE.MISSING", f"answers fixture has no entry for {fixture_key(a)} (or '*')",
                        action=f"Add an entry for {fixture_key(a)} (or '*') to the answers file.")
        meta["attempts"] = 1
        sent = body["questions"]
        left_out = set(qs["questions"]) - set(sent)  # extra questions dropped from this request for size
        checked = validate_response({"model": "fixture", "answers": {k: v for k, v in fx.items() if k not in left_out}}, sent,
                                    qs.get("optional") or ())
        if isinstance(checked, Err):
            return checked
        return Ok((checked.value, meta))
    return judge_with_policy(body, ctx["api_key"], ctx["timeout"], ctx["max_request_bytes"], ctx["min_item_chars"], body["questions"],
                             shorten=ctx["gate"] != "plan", optional=frozenset(qs.get("optional") or ()))


def entry_base(a: dict) -> dict:
    """The anchor facts every result row carries, judged or excluded."""
    return {
        "anchor_id": a["id"], "source_type": a["source_type"], "source_ref": a.get("source_ref"),
        "statement": a["statement"], "quote": anchor_quote(a), "stated_by": a.get("stated_by"), "stated_at": a.get("stated_at"),
        "context": a.get("context"), "anchor_version": a.get("version", 1),
        "deterministic_check": a.get("deterministic_check") or "",
        "kind": anchor_kind(a), "leaning": None,
    }


def excluded_entry(a: dict, exclusion: dict, gate: str) -> dict:
    """A result row for a cell the anchor's `exclude` list names: no question set, no request, no Jev call."""
    entry = entry_base(a)
    reason = exclusion["reason"].strip()
    noun = "bead" if gate == "plan" else "work"
    entry.update({"verdict": "excluded", "answers": None, "min_confidence": None,
                  "exclusion": {"item": exclusion["item"], "reason": reason},
                  "explanation": f"Excluded from this {noun} by the anchor's exclude list: {sentence(reason)}",
                  "attempts": 0})
    return entry


def ask_triage(ctx: dict, a: dict, qs: dict, deferred: set, item: dict, fixture_key, entry: dict) -> Result[int, JudgeError]:
    """Ask the triage questions of one failed cell in their own request and record the answers on the cell; never touches the
    verdict. Ok(attempts made); Err only for an ABORT_CODES error. A request over the size cap, or a call that gets no answer, is
    recorded on the cell (extra_questions_dropped, triage_error) and costs nothing else."""
    asked = {k: v for k, v in qs["questions"].items() if k in deferred}
    if ctx["answers_fixture"] is not None:
        fx = ctx["answers_fixture"].get(fixture_key(a)) or ctx["answers_fixture"].get("*") or {}
        entry["answers"].update({k: v for k, v in fx.items() if k in asked})
        entry["triage_questions"] = [k for k in asked if k in entry["answers"]]
        return Ok(0)
    body = {"model": ctx["model"], "state": build_state(a, item), "questions": asked}
    if len(encode_body(body)) > ctx["max_request_bytes"]:
        entry["extra_questions_dropped"] = sorted(set(entry.get("extra_questions_dropped") or ()) | deferred)
        return Ok(0)
    got = judge_with_policy(body, ctx["api_key"], ctx["timeout"], ctx["max_request_bytes"], ctx["min_item_chars"], asked,
                            shorten=False, optional=frozenset(asked))
    if isinstance(got, Err):
        if got.error.code in ABORT_CODES:
            return got
        entry["triage_error"] = got.error.to_json()["error"]
        return Ok((got.error.meta or {}).get("attempts", 0))
    resp, meta = got.value
    entry["answers"].update(resp["answers"])
    entry["triage_questions"] = [k for k in asked if k in entry["answers"]]  # which answers are triage, in question order, for the report
    return Ok(meta.get("attempts", 0))


def judge_entries(ctx: dict, prepared: list, item: dict, fixture_key) -> Result[tuple, JudgeError]:
    """One Jev request per anchor. Returns Ok((results, requests_out, model_answered)); Err only for an ABORT_CODES error.

    A cell whose Jev call produced no answer gets verdict ``error`` (a failed near-threshold confirmation keeps
    the first verdict and records ``confirm_error``); ``cell_failures`` lists both, and the caller exits 2.
    """
    results, requests_out, model_answered = [], [], None
    for a, qs, overridden in prepared:
        deferred = set(qs.get("deferred") or ())
        body = {"model": ctx["model"], "state": build_state(a, item),
                "questions": {k: v for k, v in qs["questions"].items() if k not in deferred}}
        dropped = []
        if (set(qs.get("optional") or ()) - deferred) and len(encode_body(body)) > ctx["max_request_bytes"]:
            # extra questions never cost a verdict: over the size cap they are left out of this request, not the item
            dropped = sorted(set(qs["optional"]) - deferred)
            body["questions"] = {k: v for k, v in body["questions"].items() if k not in dropped}
        entry = entry_base(a)
        entry.update({
            "question_set": {"gate": qs.get("gate", ctx["gate"]), "version": qs.get("version"), "overridden": overridden},
            # the gate questions' digest: extra questions never change it (they get their own)
            "questions_sha256": questions_digest({k: v for k, v in qs["questions"].items() if k not in (qs.get("optional") or ())}),
        })
        if qs.get("optional"):
            entry["extra_questions_sha256"] = questions_digest({k: qs["questions"][k] for k in qs["optional"]})
        if overridden:
            entry["questions"] = qs["questions"]
        if dropped:
            entry["extra_questions_dropped"] = dropped
        meta = {"attempts": 0, "item_chars_sent": len(item["text"]), "item_shortened": False}
        if entry["kind"] == "structural" and ctx["gate"] == "plan":
            # Jev is not asked: the default compliance question answered no on 9/10 beads for a structural anchor.
            entry.update({"verdict": "deterministic_only", "answers": None, "min_confidence": None,
                          "explanation": explain("deterministic_only", 0.0, ctx["gate"])})
            entry.update(meta)
            results.append(entry)
            continue
        if ctx["dry_run"]:
            fitted = fit_request(body, ctx["max_request_bytes"], ctx["min_item_chars"], meta)
            if isinstance(fitted, Ok):
                entry.update({"verdict": None, "answers": None, "min_confidence": None, "explanation": "dry run"})
            else:
                entry.update({"verdict": None, "answers": None, "min_confidence": None, "explanation": "dry run; request does not fit",
                              "error": fitted.error.to_json()["error"]})
            requests_out.append(body)
            entry.update(meta)
            results.append(entry)
            continue
        first = _first_answer(ctx, a, qs, body, meta, fixture_key)
        if isinstance(first, Ok):
            resp, meta = first.value
            model_answered = resp.get("model", model_answered)
            verdict, min_conf = decide(resp["answers"], qs["roles"], ctx["threshold"], entry["kind"])
            entry["leaning"] = leaning_of(resp["answers"], qs["roles"], entry["kind"])
            if ctx["answers_fixture"] is None and ctx.get("confirm", False) and near_threshold(resp["answers"], qs["roles"], ctx["threshold"], entry["kind"]):
                # Near the threshold a single answer is noise (8/50 identical re-runs flipped): ask once more, require agreement.
                entry["near_threshold"] = True
                second = judge_with_policy(body, ctx["api_key"], ctx["timeout"], ctx["max_request_bytes"], ctx["min_item_chars"], body["questions"],
                                           shorten=ctx["gate"] != "plan", optional=frozenset(qs.get("optional") or ()))
                if isinstance(second, Ok):
                    resp2, meta2 = second.value
                    meta["attempts"] += meta2.get("attempts", 0)
                    verdict2, min_conf2 = decide(resp2["answers"], qs["roles"], ctx["threshold"], entry["kind"])
                    if verdict2 == verdict:
                        entry["confirmed"] = True
                    else:
                        entry.update({"unstable": True, "answers_second": resp2["answers"], "verdict_first": verdict, "verdict_second": verdict2})
                        verdict, min_conf = "underspecified", min(min_conf, min_conf2)
                else:
                    exc2 = second.error
                    if exc2.code in ABORT_CODES:
                        return second
                    meta["attempts"] += (exc2.meta or {}).get("attempts", 0)
                    entry["confirm_error"] = exc2.to_json()["error"]
            entry.update({"verdict": verdict, "answers": resp["answers"], "min_confidence": round(min_conf, 3),
                          "explanation": explain(verdict, min_conf, ctx["gate"]), "usage": resp.get("usage")})
            if verdict == "underspecified":
                entry["low_questions"] = low_questions_of(entry, qs["roles"], ctx["threshold"])
            if deferred and verdict in FAILING:
                triaged = ask_triage(ctx, a, qs, deferred, item, fixture_key, entry)
                if isinstance(triaged, Err):
                    return triaged
                entry["triage_attempts"] = triaged.value  # kept apart from the gate request's attempts
        else:
            exc = first.error
            if exc.code in ABORT_CODES:
                return first
            meta = exc.meta or meta
            error = exc.to_json()["error"]
            if exc.data:
                error["data"] = exc.data
            entry.update({"verdict": "error", "answers": None, "min_confidence": None, "explanation": explain("error", 0.0, ctx["gate"]),
                          "error": error})
        entry.update({k: v for k, v in meta.items() if k != "size"})
        results.append(entry)
    results.sort(key=lambda r: (SEVERITY.get(r["verdict"] or "error", 99), r["anchor_id"]))
    return Ok((results, requests_out, model_answered))


def cell_failures(results: list, label=lambda r: r["anchor_id"]) -> list:
    """[(label, JudgeError)] for every judged cell whose Jev call produced no answer (first or confirming call)."""
    out = []
    for r in results:
        for key, suffix in (("error", ""), ("confirm_error", " (confirmation)")):
            e = r.get(key)
            if not e or (key == "error" and r.get("verdict") != "error"):
                continue  # a dry run's "does not fit" note is not a call
            out.append((label(r) + suffix, JudgeError(e["code"], e["message"], recoverable=bool(e.get("recoverable")),
                                                      action=e.get("suggested_action") or "")))
    return out


def assemble_run(ctx: dict, rec: dict, anchors_path: str, item: dict, results: list, requests_out: list, model_answered) -> dict:
    counts = {v: 0 for v in VERDICTS}
    for r in results:
        if r["verdict"]:
            counts[r["verdict"]] += 1
    run = {
        "schema": "plan-anchoring/run/1",
        "run_id": f"{ctx['gate']}-{stamp()}-{sanitize_id(item['id'])}",
        "gate": ctx["gate"],
        "created_at": now_utc().isoformat(timespec="seconds"),
        "anchors_path": anchors_path,
        "plan": rec.get("plan"),
        "item": {"id": item["id"], "title": item["title"], "source": item["source"], "chars": len(item["text"]), "truncated": item["truncated"],
                 **{k: item[k] for k in ("chars_before_reduction", "code_reduced") if k in item}},
        "model_requested": ctx["model"],
        "model_answered": model_answered,
        "threshold": ctx["threshold"],
        "max_request_bytes": ctx["max_request_bytes"],
        "min_item_chars": ctx["min_item_chars"],
        "dry_run": ctx["dry_run"],
        "fixture": ctx["answers_fixture"] is not None,
        "extra_questions": ctx.get("extra_questions") or [],
        "results": results,
        "summary": {
            "counts": {k: v for k, v in counts.items() if v},
            "failing": [r["anchor_id"] for r in results if r["verdict"] in FAILING],
            "leaning_counts": dict(sorted(((k, sum(1 for r in results if r.get("leaning") == k)) for k in {r.get("leaning") for r in results if r.get("leaning")}))),
            "unstable": [r["anchor_id"] for r in results if r.get("unstable")],
            "errors": [r["anchor_id"] for r in results if r["verdict"] == "error"],
            "excluded": [{"anchor_id": r["anchor_id"], "reason": r["exclusion"]["reason"]} for r in results if r["verdict"] == "excluded"],
            "shortened": [r["anchor_id"] for r in results if r.get("item_shortened")],
            "pass": not any(r["verdict"] in FAILING or r["verdict"] == "error" for r in results) and not ctx["dry_run"],
        },
    }
    if ctx["dry_run"]:
        run["requests"] = requests_out
        run["totals"] = {"requests": len(requests_out), "bytes": sum(len(encode_body(b)) for b in requests_out),
                         "max_request_bytes": ctx["max_request_bytes"]}
    return run


def write_run(run: dict, out_path: str, run_dir: pathlib.Path) -> str:
    """Write the run file; without --out, two runs of one item in the same second get distinct names."""
    if not out_path:
        base_id, n = run["run_id"], 1
        while (run_dir / f"{run['run_id']}.json").exists():
            n += 1
            run["run_id"] = f"{base_id}-{n}"
        out_path = str(run_dir / f"{run['run_id']}.json")
    save_json(out_path, run)
    run["path"] = out_path
    run["run_path"] = out_path
    return out_path


def default_run_dir(args) -> pathlib.Path:
    return pathlib.Path(args.run_dir or str(pathlib.Path(args.anchors).parent / "anchor-runs"))


def _valid_record(path: str) -> Result[dict, JudgeError]:
    """Load anchors.json and refuse it when validate would report problems."""
    loaded = load_record(path)
    if isinstance(loaded, Err):
        return loaded
    checked = require_valid(loaded.value)
    if isinstance(checked, Err):
        return checked
    return loaded


def cmd_judge(args) -> Result[dict, JudgeError]:
    loaded = _valid_record(args.anchors)
    if isinstance(loaded, Err):
        return loaded
    rec = loaded.value
    if args.all:
        anchors = [a for a in rec["anchors"] if a["status"] in JUDGED_STATUSES]
    else:
        if not args.anchor:
            return fail("VALIDATION.INPUT", "pass --anchor ID (repeatable) or --all")
        anchors = []
        for aid in dict.fromkeys(args.anchor):
            found = find_anchor(rec, aid)
            if isinstance(found, Err):
                return found
            anchors.append(found.value)
        if not args.include_unlocked:
            unlocked = [a["id"] for a in anchors if a.get("status") not in JUDGED_STATUSES]
            if unlocked:
                return fail("ANCHOR.NOT_LOCKED", f"anchors not locked or promoted: {', '.join(unlocked)}",
                            action="Lock the anchor first, or pass --include-unlocked to judge a proposed or retired anchor deliberately.")
    if not anchors:
        return fail("ANCHOR.NONE", "no locked anchors to judge", action="Lock at least one anchor, or pass --anchor.")
    # An anchor whose `exclude` names this item is never asked: no question set, no size check, no request.
    exclusions = {a["id"]: exclusion_for(a, args.item_id) for a in anchors}
    judged_anchors = [a for a in anchors if exclusions[a["id"]] is None]
    built = judge_context(args, rec, remote=bool(judged_anchors), triage=True)
    if isinstance(built, Err):
        return built
    ctx = built.value
    loaded = read_item(args)
    if isinstance(loaded, Err):
        return loaded
    item = loaded.value
    resolved = prepare_question_sets(ctx, judged_anchors)
    if isinstance(resolved, Err):
        return resolved
    prepared = resolved.value
    if ctx["gate"] == "plan":
        # Checked for every anchor before any request, so an oversize plan bead sends nothing and is never shortened.
        def misfit(text):
            if len(text) > ctx["max_item_chars"]:
                return f"the plan-gate item budget is --max-item-chars {ctx['max_item_chars']}"
            trial = dict(item, text=text)
            for a, qs, _ in prepared:
                gate_questions = {k: v for k, v in qs["questions"].items() if k not in (qs.get("optional") or ())}
                size = len(encode_body({"model": ctx["model"], "state": build_state(a, trial), "questions": gate_questions}))
                if size > ctx["max_request_bytes"]:
                    return f"with {a['id']} and its questions the request is {size} bytes, over --max-request-bytes {ctx['max_request_bytes']}"
            return None
        fitted = fit_plan_item(item, misfit, ctx["max_code_lines"])
        if isinstance(fitted, Err):
            return fitted
    judged = judge_entries(ctx, prepared, item, lambda a: a["id"])
    if isinstance(judged, Err):
        return judged
    results, requests_out, model_answered = judged.value
    results = results + [excluded_entry(a, exclusions[a["id"]], ctx["gate"]) for a in anchors if exclusions[a["id"]] is not None]
    results.sort(key=lambda r: (SEVERITY.get(r["verdict"] or "error", 99), r["anchor_id"]))
    run = assemble_run(ctx, rec, args.anchors, item, results, requests_out, model_answered)
    path = write_run(run, args.out, default_run_dir(args))
    # a dry run asks Jev nothing, so an unsendable request is reported in its cell, not as a Jev failure
    failures = [] if ctx["dry_run"] else cell_failures(results)
    if failures:  # the partial run is kept on disk; the command still could not do its job
        judged = sum(1 for r in results if r["verdict"] not in ("excluded", "deterministic_only"))
        return jev_failure("judged cells", failures, judged, run)  # data is the run as written (run_path, results, summary)
    return Ok({"success": True, "data": run, "error": None})


def cmd_check_fixtures(args) -> Result[dict, JudgeError]:
    loaded = _valid_record(args.anchors)
    if isinstance(loaded, Err):
        return loaded
    rec = loaded.value
    if args.all:
        anchors = [a for a in rec["anchors"] if a.get("status") != "retired"]
    elif args.id:
        found = find_anchor(rec, args.id)
        if isinstance(found, Err):
            return found
        anchors = [found.value]
    else:
        return fail("VALIDATION.INPUT", "pass --id ANC-NNN or --all")
    built = judge_context(args, rec)
    if isinstance(built, Err):
        return built
    ctx = built.value
    resolved = prepare_question_sets(ctx, anchors)
    if isinstance(resolved, Err):
        return resolved
    prepared = resolved.value
    fixtures_dir = (pathlib.Path(args.run_dir) / "fixtures") if args.run_dir else None
    checks, runs, written = [], [], []
    for a, qs, overridden in prepared:
        fx = a.get("fixtures") or {}
        for kind in ("pass", "fail"):
            for n, sentence in enumerate(fx.get(kind) or [], 1):
                if not isinstance(sentence, str) or not sentence.strip():
                    continue
                item = {"id": f"fixture:{a['id']}:{kind}:{n}", "title": "fixture", "text": sentence.strip(), "truncated": False, "source": "fixtures"}
                key = f"{a['id']}:{kind}:{n}"
                judged = judge_entries(ctx, [(a, qs, overridden)], item, lambda _a, key=key: key)
                if isinstance(judged, Err):
                    return judged
                results, requests_out, model_answered = judged.value
                run = assemble_run(ctx, rec, args.anchors, item, results, requests_out, model_answered)
                verdict = results[0]["verdict"]
                check = {"anchor_id": a["id"], "kind": kind, "index": n, "sentence": item["text"], "verdict": verdict,
                         "expected": list(FIXTURE_EXPECTED[kind]),
                         "expected_ok": (verdict in FIXTURE_EXPECTED[kind]) if not ctx["dry_run"] and verdict != "deterministic_only" else None}
                if results[0].get("error"):
                    check["error"] = results[0]["error"]
                if results[0].get("confirm_error"):
                    check["confirm_error"] = results[0]["confirm_error"]
                if fixtures_dir is not None:
                    written.append(write_run(run, "", fixtures_dir))
                    check["run_path"] = run["run_path"]
                checks.append(check)
                runs.append(run)
    if not checks:
        return fail("FIXTURE.NONE", "no fixture sentences recorded on the selected anchors",
                    action="Add fixtures with `update --id ANC-NNN --fixture-pass ... --fixture-fail ...`.")
    mismatches = [f"{c['anchor_id']}:{c['kind']}:{c['index']}" for c in checks if c["expected_ok"] is False]
    judged = sum(1 for c in checks if c["expected_ok"] is not None)
    data = {"gate": args.gate, "dry_run": ctx["dry_run"], "fixture": ctx["answers_fixture"] is not None,
            "checks": checks, "mismatches": mismatches, "judged": judged,
            # nothing judged (every check deterministic_only, or a dry run) is never a pass: no fixture was tested
            "pass": not mismatches and not ctx["dry_run"] and judged > 0,
            "path": str(fixtures_dir) if fixtures_dir is not None else None,
            "run_path": str(fixtures_dir) if fixtures_dir is not None else None, "runs": written}
    if judged == 0 and not ctx["dry_run"]:
        data["note"] = ("no fixture was judged: every selected check is deterministic_only (a structural anchor at the plan "
                        "gate is never sent to Jev), so pass is false; test these anchors with their deterministic_check")
    failures = [] if ctx["dry_run"] else cell_failures(checks, lambda c: f"{c['anchor_id']}:{c['kind']}:{c['index']}")
    if failures:
        return jev_failure("fixture checks", failures, len(checks), data)
    if ctx["dry_run"]:
        data["requests"] = [b for r in runs for b in r.get("requests", [])]
        data["totals"] = {"requests": len(data["requests"]), "bytes": sum(len(encode_body(b)) for b in data["requests"]),
                          "max_request_bytes": ctx["max_request_bytes"]}
    return Ok({"success": True, "data": data, "error": None})


# --------------------------------------------------------------------------- #
# anchor quality: lint, dedupe, rubric (no run files; nothing leaves the machine without allow_remote_judge: true)
# --------------------------------------------------------------------------- #
def live_anchors(rec: dict) -> Result[list, JudgeError]:
    """Every non-retired anchor of the record."""
    listed = anchor_list(rec)
    if isinstance(listed, Err):
        return listed
    return Ok([a for a in listed.value if isinstance(a, dict) and a.get("status") != "retired"])


def select_anchors(rec: dict, anchor_id: str, all_: bool, what: str) -> Result[list, JudgeError]:
    """--id picks one anchor (any status); --all, or neither flag, picks every non-retired anchor."""
    if anchor_id and all_:
        return fail("VALIDATION.INPUT", f"pass --id or --all to {what}, not both")
    if anchor_id:
        found = find_anchor(rec, anchor_id)
        if isinstance(found, Err):
            return found
        return Ok([found.value])
    return live_anchors(rec)


def cmd_lint(args) -> Result[dict, JudgeError]:
    loaded = load_record(args.anchors)
    if isinstance(loaded, Err):
        return loaded
    selected = select_anchors(loaded.value, args.id, args.all, "lint")
    if isinstance(selected, Err):
        return selected
    rows = [{"id": a.get("id"), "warnings": lint_statement(a.get("statement") or "", anchor_quote(a), a.get("source_type") or "ANC")}
            for a in selected.value]
    count = sum(len(r["warnings"]) for r in rows)
    known = plan_item_ids(args)
    if isinstance(known, Err):
        return known
    data = {"clean": count == 0, "anchors": rows, "warning_count": count, "path": args.anchors}
    data.update(exclusion_lint(selected.value, known.value))
    return Ok({"success": True, "data": data, "error": None})


def plan_item_ids(args) -> Result[Any, JudgeError]:
    """Plan item ids for the exclusion check: --plan-item (repeatable) plus --plan-items FILE; None when neither is given.

    FILE is `bd show --json` output (one record or a list, each with `id`) or a JSON list of ids.
    """
    ids = [i for i in (getattr(args, "plan_item", None) or []) if i]
    path = getattr(args, "plan_items", "")
    if path:
        loaded = load_json(path, "plan items file")
        if isinstance(loaded, Err):
            return loaded
        obj = loaded.value
        if isinstance(obj, dict):
            obj = [obj]
        if not isinstance(obj, list) or not all(isinstance(x, str) or (isinstance(x, dict) and isinstance(x.get("id"), str)) for x in obj):
            return fail("INPUT.MALFORMED", "plan items file must be bd show JSON or a list of item ids",
                        action="Pass `bd show --json` output, or a JSON array of ids.")
        ids.extend(x if isinstance(x, str) else x["id"] for x in obj)
    if not ids and not path:
        return Ok(None)
    return Ok(set(ids))


def exclusion_lint(anchors: list, known) -> dict:
    """Exclusions naming an item the plan does not have. A warning only: it never changes lint's data.clean or blocks judge."""
    listed = [(a.get("id"), x) for a in anchors for x in (a.get("exclude") or []) if isinstance(x, dict)]
    if not listed and known is None:
        return {}  # no exclusions and no plan items: lint output is what it was before exclusions existed
    if known is None:
        return {"exclusions": len(listed), "exclusion_warnings": [], "exclusions_checked": False}
    warnings = [{"id": aid, "item": x.get("item"),
                 "warning": f"exclude names item {x.get('item')!r}, which is not in the plan; the exclusion never applies"}
                for aid, x in listed if x.get("item") not in known]
    return {"exclusions": len(listed), "exclusion_warnings": warnings, "exclusions_checked": True}


def check_threshold(value: float, name: str = "--threshold") -> Result[float, JudgeError]:
    if not (0.0 <= value <= 1.0):
        return fail("VALIDATION.INPUT", f"{name} must be between 0 and 1, got {value}")
    return Ok(value)


def cmd_dedupe(args) -> Result[dict, JudgeError]:
    loaded = load_record(args.anchors)
    if isinstance(loaded, Err):
        return loaded
    listed = live_anchors(loaded.value)
    if isinstance(listed, Err):
        return listed
    anchors = listed.value
    checked = check_threshold(args.threshold)
    if isinstance(checked, Err):
        return checked
    threshold = checked.value
    pairs = []
    for i, a in enumerate(anchors):
        for b in anchors[i + 1:]:
            score, fa, fb = best_similarity(a.get("statement") or "", anchor_quote(a), b.get("statement") or "", anchor_quote(b))
            if score >= threshold:
                pairs.append({"a": a.get("id"), "b": b.get("id"), "jaccard": round(score, 3), "field_a": fa, "field_b": fb})
    pairs.sort(key=lambda p: (-p["jaccard"], str(p["a"]), str(p["b"])))
    loaded = load_answers_fixture(args.answers_file, "'<a>|<b>'")
    if isinstance(loaded, Err):
        return loaded
    fixture = loaded.value
    jev_mode = bool(args.jev or fixture is not None)
    dry_run = bool(args.dry_run)
    data = {"threshold": threshold, "confidence_threshold": DEFAULT_THRESHOLD, "jev": jev_mode, "dry_run": dry_run, "fixture": fixture is not None,
            "anchors": len(anchors), "pairs": pairs, "likely_duplicates": []}
    if jev_mode:
        loaded = load_simple_question_set("dedupe", (DEDUPE_QUESTION,))
        if isinstance(loaded, Err):
            return loaded
        qs = loaded.value
        api_key = ""
        if pairs and not dry_run and fixture is None:
            cleared = remote_preflight(args, "The anchor statements")
            if isinstance(cleared, Err):
                return cleared
            api_key = cleared.value
        by_id = {a.get("id"): a for a in anchors}
        requests_out, failures = [], []
        for pair in pairs:
            a, b = by_id[pair["a"]], by_id[pair["b"]]
            body = {"model": args.model, "questions": qs["questions"], "state": {
                "a": {"id": a.get("id"), "statement": a.get("statement") or ""},
                "b": {"id": b.get("id"), "statement": b.get("statement") or ""}}}
            if dry_run:
                fitted = fit_request(body, DEFAULT_MAX_REQUEST_BYTES, DEFAULT_MIN_ITEM_CHARS, {})
                if isinstance(fitted, Ok):
                    pair["jev"] = None
                else:
                    pair["jev"] = {"choice": None, "confidence": None, "error": fitted.error.to_json()["error"]}
                requests_out.append(body)
                continue
            asked = ask_jev_once(args, api_key, body, qs["questions"], fixture, (f"{pair['a']}|{pair['b']}", "*"))
            if isinstance(asked, Ok):
                resp, meta = asked.value
                choice, conf = answer_choice(resp["answers"][DEDUPE_QUESTION])
                pair["jev"] = {"choice": choice, "confidence": round(conf, 3), "attempts": meta.get("attempts")}
            else:
                exc = asked.error
                if exc.code in ABORT_CODES:
                    return asked
                pair["jev"] = {"choice": None, "confidence": None, "attempts": (exc.meta or {}).get("attempts"), "error": exc.to_json()["error"]}
                failures.append((f"{pair['a']}|{pair['b']}", exc))
        if dry_run:
            data["requests"] = requests_out
            data["totals"] = {"requests": len(requests_out), "bytes": sum(len(encode_body(b)) for b in requests_out), "max_request_bytes": DEFAULT_MAX_REQUEST_BYTES}
            data["likely_duplicates"] = [p for p in pairs if p["jaccard"] >= LIKELY_DUPLICATE_JACCARD]
        else:
            data["likely_duplicates"] = [p for p in pairs if p.get("jev") and p["jev"].get("choice") == "yes" and (p["jev"].get("confidence") or 0.0) >= DEFAULT_THRESHOLD]
            if failures:  # an unanswered pair is not "not a duplicate"
                return jev_failure("pairs", failures, len(pairs), data)
    else:
        data["likely_duplicates"] = [p for p in pairs if p["jaccard"] >= LIKELY_DUPLICATE_JACCARD]
    return Ok({"success": True, "data": data, "error": None})


RUBRIC_QUESTIONS = ("single_constraint", "observable", "faithful", "scoped", "self_contained")


def rubric_confirmation_outcome(responses: list, questions: tuple, threshold: float, lint_errors: list) -> Result[dict, JudgeError]:
    """Classify two rubric confirmations and an optional tie-breaker.

    A third response is meaningful only when the first two choices disagree.
    Choice disagreement that a third answer cannot resolve is unstable. A
    resolved choice supported by fewer than two confident matching answers is
    inconclusive. Confident no/unclear is needs_work; only all-confident yes
    with no deterministic lint errors is testable.
    """
    if len(responses) not in (2, 3):
        return fail("RUBRIC.CONFIRMATION", "rubric confirmation needs two responses and an optional third")
    disagreements = []
    unstable = []
    low = []
    failing = []
    resolved = {}
    for qid in questions:
        cells = [answer_choice(response["answers"][qid]) for response in responses]
        first_disagrees = cells[0][0] != cells[1][0]
        if first_disagrees:
            disagreements.append(qid)
            if len(cells) != 3:
                return fail("RUBRIC.CONFIRMATION", f"third response missing for disagreement on {qid}")
            confident = {
                choice: [index + 1 for index, (got, confidence) in enumerate(cells)
                         if got == choice and confidence >= threshold]
                for choice in ("yes", "no", "unclear")
            }
            supported = [choice for choice, calls in confident.items() if len(calls) >= 2]
            competing = [choice for choice, calls in confident.items() if calls]
            if len(supported) == 1 and len(competing) == 1:
                choice = supported[0]
                matching = confident[choice]
                confidences = [cells[index - 1][1] for index in matching]
            elif len(competing) > 1:
                unstable.append(qid)
                resolved[qid] = {"choice": None, "confidence": None, "matching_calls": []}
                continue
            else:
                low.append(qid)
                resolved[qid] = {"choice": None, "confidence": None, "matching_calls": []}
                continue
        else:
            choice = cells[0][0]
            matching = [1, 2]
            confidences = [cells[0][1], cells[1][1]]
        min_confidence = min(confidences)
        resolved[qid] = {
            "choice": choice,
            "confidence": round(min_confidence, 3),
            "matching_calls": matching,
        }
        if min_confidence < threshold:
            low.append(qid)
        elif choice != "yes":
            failing.append(qid)

    if unstable:
        verdict = "unstable"
    elif low:
        verdict = "inconclusive"
    elif failing or lint_errors:
        verdict = "needs_work"
    else:
        verdict = "testable"
    return Ok({
        "verdict": verdict,
        "failing": list(dict.fromkeys(failing + low + unstable + list(lint_errors))),
        "low_confidence": low,
        "unstable": unstable,
        "initial_disagreements": disagreements,
        "resolved": resolved,
    })


def rubric_fixture_response(fixture: dict, keys: tuple, index: int, questions: dict) -> Result[dict, JudgeError]:
    value = None
    for key in keys:
        value = fixture.get(key)
        if value is not None:
            break
    if value is None:
        return fail("FIXTURE.MISSING", f"answers fixture has no entry for {keys[0]} (or '*')",
                    action=f"Add an entry for {keys[0]} (or '*') to the answers file.")
    if isinstance(value, list):
        if index >= len(value):
            return fail("FIXTURE.MISSING", f"answers fixture has no confirmation {index + 1} for {keys[0]}",
                        action=f"Give {keys[0]} one answer object per confirmation in the answers file.")
        value = value[index]
    return validate_response({"model": "fixture", "answers": value}, questions)


def _rubric_ask(args, api_key: str, body: dict, qs: dict, fixture, cid, index: int) -> Result[tuple, JudgeError]:
    """Confirmation `index` for one candidate: the fixture entry when a fixture is given, else one live request. Ok((response, meta))."""
    if fixture is not None:
        response = rubric_fixture_response(fixture, (cid, "*"), index, qs["questions"])
        if isinstance(response, Err):
            return response
        return Ok((response.value, {"attempts": 1}))
    return ask_jev_once(args, api_key, body, qs["questions"], None, (cid, "*"))


def _rubric_judgement(args, api_key: str, body: dict, qs: dict, fixture, cid, threshold: float, lint_errors: list) -> Result[tuple, JudgeError]:
    """Two confirmations, a third on disagreement, then the outcome. Ok((responses, attempts, outcome))."""
    responses = []
    attempts = 0
    for index in range(2):
        asked = _rubric_ask(args, api_key, body, qs, fixture, cid, index)
        if isinstance(asked, Err):
            return asked
        response, meta = asked.value
        responses.append(response)
        attempts += meta.get("attempts", 1)
    first_choices = [answer_choice(responses[0]["answers"][qid])[0] for qid in qs["questions"]]
    second_choices = [answer_choice(responses[1]["answers"][qid])[0] for qid in qs["questions"]]
    if first_choices != second_choices:
        asked = _rubric_ask(args, api_key, body, qs, fixture, cid, 2)
        if isinstance(asked, Err):
            return asked
        response, meta = asked.value
        responses.append(response)
        attempts += meta.get("attempts", 1)
    outcome = rubric_confirmation_outcome(responses, tuple(qs["questions"]), threshold, lint_errors)
    if isinstance(outcome, Err):
        return outcome
    return Ok((responses, attempts, outcome.value))


def rubric_entries(args, candidates: list, threshold: float, dry_run: bool, fixture, what: str) -> Result[tuple, JudgeError]:
    """Lint, then one Jev request per candidate {id, statement, quote, scope, source_type}. Returns Ok((entries, requests_out)).

    An entry is testable when every rubric question has two confident yes
    confirmations and deterministic requirement-form lint has no error. A
    third call is made only when the first two choices disagree. Nothing is
    written to disk.
    """
    loaded = load_simple_question_set("testability", RUBRIC_QUESTIONS)
    if isinstance(loaded, Err):
        return loaded
    qs = loaded.value
    api_key = ""
    if candidates and not dry_run and fixture is None:
        cleared = remote_preflight(args, what)
        if isinstance(cleared, Err):
            return cleared
        api_key = cleared.value
    entries, requests_out, failures = [], [], []
    for c in candidates:
        warnings = lint_statement(c["statement"], c["quote"], c.get("source_type") or "ANC")
        lint_errors = [w for w in warnings if w == LINT_VERBATIM or w in REQUIREMENT_FORM_WARNINGS]
        entry = {"id": c["id"], "statement": c["statement"], "quote": c["quote"], "scope": c.get("scope") or "",
                 "verdict": None, "failing": list(lint_errors), "answers": None, "lint": {"warnings": warnings, "errors": lint_errors}}
        body = {"model": args.model, "questions": qs["questions"],
                "state": {"anchor": {"statement": c["statement"], "quote": c["quote"], "scope": c.get("scope") or ""}}}
        if dry_run:
            fitted = fit_request(body, DEFAULT_MAX_REQUEST_BYTES, DEFAULT_MIN_ITEM_CHARS, {})
            if isinstance(fitted, Err):
                entry["error"] = fitted.error.to_json()["error"]
            requests_out.extend((body, json.loads(json.dumps(body))))
            entries.append(entry)
            continue
        judged = _rubric_judgement(args, api_key, body, qs, fixture, c["id"], threshold, lint_errors)
        if isinstance(judged, Ok):
            responses, attempts, outcome = judged.value
            entry["answers"] = responses[0]["answers"]
            entry["answers_second"] = responses[1]["answers"]
            if len(responses) == 3:
                entry["answers_third"] = responses[2]["answers"]
            entry["attempts"] = attempts
            entry["confirmations"] = outcome
            entry["failing"] = outcome["failing"]
            entry["verdict"] = outcome["verdict"]
        else:
            exc = judged.error
            if exc.code in ABORT_CODES:
                return judged
            entry["verdict"] = "error"
            entry["failing"] = [f"error: {exc.code}"] + lint_errors
            entry["error"] = exc.to_json()["error"]
            failures.append((str(c["id"]), exc))
        entries.append(entry)
    if failures:  # a rubric that could not run is never needs_work, and `add --rubric` never records on it
        return jev_failure("rubric candidates", failures, len(candidates), {"anchors": entries})
    return Ok((entries, requests_out))


def cmd_rubric(args) -> Result[dict, JudgeError]:
    checked = check_threshold(args.threshold)
    if isinstance(checked, Err):
        return checked
    threshold = checked.value
    loaded = load_answers_fixture(args.answers_file, "anchor id (or 'candidate')")
    if isinstance(loaded, Err):
        return loaded
    fixture = loaded.value
    if args.statement is not None:
        if args.anchors or args.id or args.all:
            return fail("VALIDATION.INPUT", "pass either --statement/--quote/--scope for a candidate or --anchors with --id/--all, not both")
        statement = args.statement.strip()
        if not statement:
            return fail("VALIDATION.INPUT", "statement cannot be empty")
        quote = (args.quote or "").strip()
        if not quote:
            return fail("VALIDATION.INPUT", "--quote is required with --statement: the rubric checks the statement against the user's words",
                        action='Pass --quote "<the user\'s exact words>".')
        candidates = [{"id": "candidate", "statement": statement, "quote": quote, "scope": args.scope or "", "source_type": "ANC"}]
        path = None
    else:
        if not args.anchors:
            return fail("VALIDATION.INPUT", "pass --anchors with --id/--all, or --statement/--quote/--scope for a candidate")
        loaded = load_record(args.anchors)
        if isinstance(loaded, Err):
            return loaded
        selected = select_anchors(loaded.value, args.id, args.all, "run the rubric")
        if isinstance(selected, Err):
            return selected
        anchors = selected.value
        candidates = [{"id": a.get("id"), "statement": a.get("statement") or "", "quote": anchor_quote(a), "scope": a.get("scope") or "",
                       "source_type": a.get("source_type") or "ANC"} for a in anchors]
        path = args.anchors
    dry_run = bool(args.dry_run)
    judged = rubric_entries(args, candidates, threshold, dry_run, fixture, "The anchor statements, quotes and scopes")
    if isinstance(judged, Err):
        return judged
    entries, requests_out = judged.value
    data = {"threshold": threshold, "dry_run": dry_run, "fixture": fixture is not None, "path": path, "anchors": entries,
            "needs_work": [e["id"] for e in entries if e["verdict"] != "testable"],
            "pass": bool(entries) and all(e["verdict"] == "testable" for e in entries) and not dry_run}
    if dry_run:
        data["requests"] = requests_out
        data["totals"] = {"requests": len(requests_out), "bytes": sum(len(encode_body(b)) for b in requests_out), "max_request_bytes": DEFAULT_MAX_REQUEST_BYTES}
    return Ok({"success": True, "data": data, "error": None})


def add_rubric_gate(args, statement: str, quote: str, source_type: str) -> Result[tuple, JudgeError]:
    """`add --rubric`: judge the candidate before writing: Ok((entry, refusal)).

    refusal is an ANCHOR.NOT_TESTABLE JudgeError (reported as data.refused, exit 0) unless the rubric passed or
    --allow-untestable was given, else None. Err when the rubric could not run (a Jev failure or a missing
    answers entry): exit 2 and nothing is recorded, --allow-untestable or not.
    """
    loaded = load_answers_fixture(getattr(args, "answers_file", ""), "'candidate'")
    if isinstance(loaded, Err):
        return loaded
    candidate = {"id": "candidate", "statement": statement, "quote": quote, "scope": args.scope or "", "source_type": source_type}
    judged = rubric_entries(args, [candidate], DEFAULT_THRESHOLD, False, loaded.value, "The anchor statement, quote and scope")
    if isinstance(judged, Err):
        return judged
    entry = judged.value[0][0]
    if entry["verdict"] != "testable" and not getattr(args, "allow_untestable", False):
        return Ok((entry, JudgeError(
            "ANCHOR.NOT_TESTABLE", f"the statement did not pass the testability rubric ({entry['verdict']}): " + ", ".join(entry["failing"]),
            recoverable=True,
            action="Reword the statement so each failing criterion holds (see references/anchor-capture.md), re-run `rubric --statement ... --quote ...`, "
                   "or pass --allow-untestable to record it anyway.")))
    return Ok((entry, None))


# --------------------------------------------------------------------------- #
# reporting
# --------------------------------------------------------------------------- #
def quote_statement(statement: str) -> str:
    s = statement.strip()
    return f'"{s}"' if s.endswith((".", "!", "?")) else f'"{s}."'


def sentence(text: str) -> str:
    s = str(text or "").strip()
    return s if s.endswith((".", "!", "?")) else s + "."


def triage_lines(r: dict) -> list:
    """The triage answers of one failed cell, one line, in question order; or why there are none. The answers never change the
    verdict; they are what triage/rules.json routes on, so the reader sees why a cell goes to exclude, plan, question or human."""
    lines = []
    answers = r.get("answers") or {}
    shown = []
    for qid in r.get("triage_questions") or []:
        if isinstance(answers.get(qid), dict):
            choice, conf = answer_choice(answers[qid])
            shown.append(f"`{qid}` {choice} ({conf:.2f})")
    if shown:
        lines.append(">   Triage: " + ", ".join(shown) + ".")
    if r.get("extra_questions_dropped"):
        lines.append(">   Not asked, over the request size cap: " + ", ".join(f"`{q}`" for q in r["extra_questions_dropped"]) + ".")
    if isinstance(r.get("triage_error"), dict) and r["triage_error"].get("code"):
        lines.append(f">   Triage error {r['triage_error']['code']}: {r['triage_error'].get('message', '')}".rstrip())
    return lines


def report_lines(run: dict, include_passing: bool = False) -> list:
    """One blockquote per judged anchor: the testable statement, then the provenance quote, then the explanation.

    ANC:     > **item · id · verdict** Anchor: <statement> You said (<context>, <date>): "<quote>" <explanation>
    REQ/ADR: > **item · id · verdict** Anchor: <statement> REQ-014 (requirements register) says: "<quote>" <explanation>
             (the says-clause is skipped when the quote is the statement, as it is for register text)
    Run files written before the quote field existed are read as quote = statement.
    An excluded cell is always listed, with the exclusion's reason in place of the provenance and explanation.
    A failed cell's triage answers follow on a `Triage:` line (run files written before 0.9.1 do not record them).
    """
    lines = []
    item_id = run["item"]["id"]
    for r in run["results"]:
        v = r.get("verdict") or "error"
        if v in ("satisfied", "not_applicable", "deterministic_only") and not include_passing:
            continue
        if v == "excluded":
            # Always shown, so the reason a cell was not judged is in every report.
            lines.append(f"> **{item_id} · {r['anchor_id']} · excluded** Anchor: {sentence(r.get('statement') or '')} "
                         f"Not judged: {sentence((r.get('exclusion') or {}).get('reason') or '')}")
            lines.append("")
            continue
        label = v.replace("_", " ")
        statement = r.get("statement") or ""
        quote = anchor_quote(r)
        if r.get("source_type") == "ANC":
            provenance = f"You said ({r.get('context') or 'planning session'}, {short_date(r.get('stated_at') or '')}): {quote_statement(quote)}"
        elif same_text(statement, quote):
            provenance = ""
        else:
            register = REGISTER_NAMES.get(r.get("source_type"), str(r.get("source_type")))
            provenance = f"{r.get('source_ref') or r['anchor_id']} ({register}) says: {quote_statement(quote)}"
        parts = [f"> **{item_id} · {r['anchor_id']} · {label}** Anchor: {sentence(statement)}", provenance, r.get("explanation") or ""]
        if v == "underspecified" and r.get("leaning"):
            parts.append(f"(leaning: {r['leaning']})")
        lines.append(" ".join(p for p in parts if p).rstrip())
        for q in r.get("low_questions") or []:
            ask = " (second ask)" if q.get("ask") == "second" else ""
            lines.append(f">   Low: {q['role']} `{q['question_id']}` answered {q['choice']} at {q['confidence']:.2f}{ask}.")
        if r.get("unstable"):
            lines.append(f">   Unstable: two identical requests gave {r.get('verdict_first')} then {r.get('verdict_second')}.")
        lines.extend(triage_lines(r))
        if v == "error" and isinstance(r.get("error"), dict) and r["error"].get("code"):
            lines.append(f">   Error {r['error']['code']}: {r['error'].get('message', '')}".rstrip())
        if r.get("deterministic_check") and v in ("satisfied", "asserted_only") and run["gate"] == "dev":
            lines.append(f">   Pair with the deterministic check: {r['deterministic_check']}")
        lines.append("")
    return lines


def cmd_report(args) -> Result[Any, JudgeError]:
    loaded = load_run(args.run, "run file")
    if isinstance(loaded, Err):
        return loaded
    run = loaded.value
    s = run["summary"]
    head = [f"## Anchor gate: {run['gate']} · {run['item']['id']}" + (f" - {run['item']['title']}" if run['item'].get('title') else ""), "",
            f"Run `{run['run_id']}`, model `{run.get('model_answered') or run.get('model_requested')}`, threshold {run['threshold']}. "
            + ("DRY RUN. " if run.get("dry_run") else "") + ("FIXTURE answers. " if run.get("fixture") else "")
            + "Counts: " + (", ".join(f"{k} {v}" for k, v in (s.get("counts") or {}).items()) or "none") + ".", ""]
    body = report_lines(run, include_passing=args.all)
    if not any(line and "· excluded**" not in line for line in body):
        body = body + ["All judged anchors are satisfied or not applicable.", ""]
    md = "\n".join(head + body).rstrip() + "\n"
    if args.format == "md":
        return Ok(md)
    return Ok({"success": True, "data": {"run_id": run["run_id"], "pass": s.get("pass"), "failing": s.get("failing"), "markdown": md}, "error": None})


def collect_runs(paths: list) -> list:
    runs = []
    for p in paths:
        pp = pathlib.Path(p)
        files = sorted(pp.glob("*.json")) if pp.is_dir() else [pp]
        for f in files:
            try:
                r = json.loads(f.read_text(encoding="utf-8"))
            except (json.JSONDecodeError, OSError, UnicodeDecodeError):
                continue
            if isinstance(r, dict) and r.get("schema") == "plan-anchoring/run/1" and not r.get("dry_run") \
                    and isinstance(r.get("results"), list) and isinstance(r.get("item"), dict):
                runs.append(r)
    return runs


def cmd_coverage(args) -> Result[Any, JudgeError]:
    runs = collect_runs(args.runs)
    if args.gate:
        runs = [r for r in runs if r.get("gate") == args.gate]
    if not runs:
        return fail("RUNS.NONE", "no run files found", action="Run `judge` first, or check the --runs path.")
    record = None
    if getattr(args, "anchors", ""):
        loaded = _valid_record(args.anchors)
        if isinstance(loaded, Err):
            return loaded
        record = {a["id"]: a for a in loaded.value["anchors"]}
    ordered = sorted(runs, key=lambda r: str(r.get("created_at", "")))
    # latest run per (gate, item), for runs_considered and to mark cells carried from an earlier run
    latest = {}
    for r in ordered:
        latest[(r["gate"], r["item"]["id"])] = r

    # without a --gate filter, columns are qualified so plan and dev verdicts for one item never overwrite each other
    def column(gate, item_id):
        return item_id if args.gate else f"{gate}:{item_id}"
    # latest result per (gate, item, anchor): a re-run of some anchors never drops the others' results
    cells = {}
    for r in ordered:
        for res in r["results"]:
            cells[(r["gate"], r["item"]["id"], res["anchor_id"])] = (res, r)
    matrix, excluded, carried, rejudge = {}, {}, {}, {}
    items = sorted({column(g, i) for g, i in latest})
    for (gate, item_id, anchor_id), (res, r) in cells.items():
        col = column(gate, item_id)
        verdict = res["verdict"]
        reason = (res.get("exclusion") or {}).get("reason", "")
        if record is not None and anchor_id in record:
            # the record is authoritative for exclusions: one recorded after the run applies, one removed since needs a re-run
            current = exclusion_for(record[anchor_id], item_id)
            if current is not None:
                verdict, reason = "excluded", current.get("reason", "")
            elif verdict == "excluded":
                rejudge.setdefault(anchor_id, []).append(col)
                verdict = None
        if verdict is None:
            matrix.setdefault(anchor_id, {})  # still listed, and uncovered unless another item satisfies it
            continue
        matrix.setdefault(anchor_id, {})[col] = verdict
        if verdict == "excluded":
            excluded.setdefault(anchor_id, {})[col] = reason
        if r is not latest[(gate, item_id)]:
            carried.setdefault(anchor_id, {})[col] = r.get("run_id")
    anchors_sorted = sorted(matrix)
    uncovered = [a for a in anchors_sorted if not any(v == "satisfied" for v in matrix[a].values())]
    contradicted = {a: [i for i, v in matrix[a].items() if v == "contradicted"] for a in anchors_sorted if any(v == "contradicted" for v in matrix[a].values())}
    data = {"gate": args.gate or "all", "items": items, "anchors": anchors_sorted, "matrix": matrix,
            "uncovered_anchors": uncovered, "contradicted": contradicted, "excluded": excluded, "runs_considered": len(latest),
            "carried_from_earlier_run": carried, "exclusion_removed_rejudge": rejudge}
    if args.format == "md":
        lines = [f"## Anchor coverage ({data['gate']})", "", "| Anchor | " + " | ".join(items) + " | Covered |", "| --- | " + " | ".join("---" for _ in items) + " | --- |"]
        for a in anchors_sorted:
            row = [(matrix[a].get(i) or "-").replace("_", " ") for i in items]
            lines.append(f"| {a} | " + " | ".join(row) + f" | {'no' if a in uncovered else 'yes'} |")
        lines.append("")
        if uncovered:
            lines.append("Uncovered anchors (no item satisfies them): " + ", ".join(uncovered))
        if excluded:
            lines.append("")
            lines.append("Excluded cells (not judged):")
            for a in sorted(excluded):
                for i, reason in sorted(excluded[a].items()):
                    lines.append(f"- {a} x {i}: {reason}")
        if carried:
            lines.append("")
            lines.append("Cells from an earlier run than the item's latest (that run did not judge them):")
            for a in sorted(carried):
                for i, run_id in sorted(carried[a].items()):
                    lines.append(f"- {a} x {i}: {run_id}")
        if rejudge:
            lines.append("")
            lines.append("Exclusion removed since the run; re-judge: " + ", ".join(f"{a} x {i}" for a in sorted(rejudge) for i in sorted(rejudge[a])))
        return Ok("\n".join(lines) + "\n")
    return Ok({"success": True, "data": data, "error": None})


def diff_rank(verdict) -> int:
    """Severity index for diffs; an error (or a missing verdict) ranks below every real verdict, so becoming one is a regression."""
    if verdict is None or verdict == "error":
        return -1
    return SEVERITY.get(verdict, -1)


def cmd_diff(args) -> Result[dict, JudgeError]:
    loaded = load_run(args.prev, "previous run")
    if isinstance(loaded, Err):
        return loaded
    prev = loaded.value
    loaded = load_run(args.next, "next run")
    if isinstance(loaded, Err):
        return loaded
    nxt = loaded.value
    if prev["gate"] != nxt["gate"]:
        return fail("RUNS.MISMATCH", "runs are for different gates")
    if prev["item"]["id"] != nxt["item"]["id"]:
        return fail("RUNS.MISMATCH", f"runs are for different items ({prev['item']['id']} vs {nxt['item']['id']})",
                    action="Diff two runs of the same bead or sprint.")
    pv = {r["anchor_id"]: r for r in prev["results"]}
    nv = {r["anchor_id"]: r for r in nxt["results"]}
    changed, added, removed = [], [], []
    for aid in sorted(set(pv) | set(nv)):
        if aid not in pv:
            added.append({"anchor_id": aid, "verdict": nv[aid]["verdict"]})
        elif aid not in nv:
            removed.append({"anchor_id": aid, "verdict": pv[aid]["verdict"]})
        elif pv[aid]["verdict"] != nv[aid]["verdict"]:
            changed.append({"anchor_id": aid, "from": pv[aid]["verdict"], "to": nv[aid]["verdict"],
                            "improved": diff_rank(nv[aid]["verdict"]) > diff_rank(pv[aid]["verdict"])})
    def passing(v):
        return v is not None and v != "error" and v not in FAILING

    def item_chars(run):
        item = run.get("item") or {}
        value = item.get("chars_before_reduction", item.get("chars", 0))
        return value if isinstance(value, int) and not isinstance(value, bool) else 0

    before_chars, after_chars = item_chars(prev), item_chars(nxt)

    data = {"gate": nxt["gate"], "item": nxt["item"]["id"], "prev": prev["run_id"], "next": nxt["run_id"],
            "changed": changed, "added": added, "removed": removed,
            # regressions: the verdict got more severe (kept as before). pass_to_fail / fail_to_pass: which side of the gate moved.
            # A verdict change is not a regression by itself: 8 correct satisfied -> not_addressed flips were once labelled so.
            "regressions": [c["anchor_id"] for c in changed if not c["improved"]],
            "pass_to_fail": [c["anchor_id"] for c in changed if passing(c["from"]) and not passing(c["to"])],
            "fail_to_pass": [c["anchor_id"] for c in changed if not passing(c["from"]) and passing(c["to"])],
            "item_size": {"before_chars": before_chars, "after_chars": after_chars,
                          "delta_chars": after_chars - before_chars, "grew": after_chars > before_chars},
            "excluded": [{"anchor_id": aid, "reason": (nv[aid].get("exclusion") or {}).get("reason", "")}
                         for aid in sorted(nv) if nv[aid].get("verdict") == "excluded"],
            "pass_before": prev["summary"].get("pass"), "pass_after": nxt["summary"].get("pass")}
    if getattr(args, "expected", ""):
        loaded = load_json(args.expected, "expected verdicts")
        if isinstance(loaded, Err):
            return loaded
        expected = loaded.value
        if not isinstance(expected, dict) or not all(isinstance(k, str) and isinstance(v, str) for k, v in expected.items()):
            return fail("INPUT.MALFORMED", "expected verdicts must be a JSON object {anchor_id: verdict}", action="Write the verdict a human reading gives each anchor.")
        cells = []
        for aid, want in sorted(expected.items()):
            if want not in VERDICTS:
                return fail("VALIDATION.INPUT", f"expected verdict for {aid} must be one of {VERDICTS}, got {want!r}")
            p = pv[aid]["verdict"] if aid in pv else None
            n = nv[aid]["verdict"] if aid in nv else None
            cells.append({"anchor_id": aid, "expected": want, "prev": p, "next": n, "prev_matches": p == want, "next_matches": n == want})
        data["expected"] = {
            "path": args.expected, "cells": cells,
            "corrections": [c["anchor_id"] for c in cells if c["next_matches"] and not c["prev_matches"]],
            "regressions": [c["anchor_id"] for c in cells if c["prev_matches"] and not c["next_matches"]],
            "still_off": [c["anchor_id"] for c in cells if not c["prev_matches"] and not c["next_matches"]],
            "all_match": all(c["next_matches"] for c in cells),
        }
    return Ok({"success": True, "data": data, "error": None})


# --------------------------------------------------------------------------- #
# cli
# --------------------------------------------------------------------------- #
def add_judge_options(parser, with_item: bool) -> None:
    parser.add_argument("--anchors", required=True)
    parser.add_argument("--gate", choices=("plan", "dev"), required=True)
    if with_item:
        parser.add_argument("--anchor", action="append", help="anchor id; repeatable")
        parser.add_argument("--all", action="store_true", help="all locked/promoted anchors")
        parser.add_argument("--include-unlocked", action="store_true", help="allow --anchor to name proposed or retired anchors")
        parser.add_argument("--item-id", required=True, help="bead id, PR number, or sprint id")
        parser.add_argument("--item-title", default="")
        parser.add_argument("--item-file", default="", help="file with the bead or work description; stdin if omitted")
        parser.add_argument("--max-item-chars", type=int, default=DEFAULT_MAX_ITEM_CHARS,
                            help="dev gate: truncate the item to this; plan gate: refuse a larger bead with PLAN.TOO_LARGE")
        parser.add_argument("--max-code-lines", type=int, default=DEFAULT_MAX_CODE_LINES,
                            help="plan gate: only fenced code blocks longer than this may be sent as signatures plus a redaction "
                                 "marker, and only when the bead would not otherwise fit (longest first, as few as needed)")
        parser.add_argument("--out", default="", help="explicit run file path")
    else:
        parser.add_argument("--id", default="", help="one anchor id")
        parser.add_argument("--all", action="store_true", help="every non-retired anchor that has fixtures")
    parser.add_argument("--questions", default="", help="question-set JSON; default assets/questions/<gate>.json")
    parser.add_argument("--extra-questions", action="append", default=[], metavar="FILE.jsonl",
                        help="repeatable: JSONL, one question per line ({id, type, instructions, criteria}); added to the gate questions "
                             "and the triage questions for this run, asked and recorded with every cell, never gating. Use it to try a "
                             "question once; to keep one, add its line to .sc/plan-anchor/triage/<gate>-questions.jsonl (see --no-triage-questions)")
    parser.add_argument("--no-triage-questions", action="store_true",
                        help="judge: do not add the triage questions. They are on by default for the planning gate: five extra, never-gating "
                             "questions (owns, specific, stated, paraphrase, partial), asked in a second request and only for a cell that "
                             "failed the gate, and recorded on that cell, so the gate request and every verdict are the same with or without "
                             "them. Their intent is triage: when a cell fails, they say whether the item should own the "
                             "requirement, state it more specifically, or whether the gate question needs tuning, so you fix the right thing. "
                             "They never change a verdict. The set is .sc/plan-anchor/triage/<gate>-questions.jsonl in your repository "
                             "(seeded once from the shipped default, never overwritten by upgrades; the nearest one wins, else the shipped copy). "
                             "To change a default, edit that line (keep its id); to add a question, add a line ({id, type, instructions, criteria}); "
                             "to drop one, delete its line. Routing of failing cells is in triage/rules.json beside it. Details: triage/README.md")
    parser.add_argument("--overrides-file", default="", help="JSON {anchor_id: {questions: {...}, roles: {...}}} read from the question beads; merged after anchors.json overrides")
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--threshold", type=float, default=DEFAULT_THRESHOLD)
    parser.add_argument("--max-request-bytes", type=int, default=None,
                        help=f"default plan {DEFAULT_MAX_REQUEST_BYTES} (what Jev reads reliably: a contradiction 28k deep was missed), "
                             f"dev {DEFAULT_MAX_REQUEST_BYTES_DEV} (Jev took 107k bytes in one attempt). dev gate: shorten the item until each "
                             "serialised request is under this; plan gate: refuse a bead that does not fit")
    parser.add_argument("--confirm-near-threshold", action="store_true", default=False,
                        help=f"re-judge once when a gating confidence is within {NEAR_THRESHOLD_BAND} of the threshold and require agreement "
                             "(the default; --no-confirm turns it off)")
    parser.add_argument("--no-confirm", action="store_true", help="never re-judge near the threshold")
    parser.add_argument("--min-item-chars", type=int, default=DEFAULT_MIN_ITEM_CHARS,
                        help="never shorten the item below this on a context overflow; record JEV.CONTEXT_EXCEEDED instead")
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("--run-dir", default="", help="judge: default <anchors dir>/anchor-runs; check-fixtures: write runs under <run-dir>/fixtures/")
    parser.add_argument("--dry-run", action="store_true", help="write the Jev requests; call nothing")
    parser.add_argument("--answers-file", default="", help="fixture answers keyed by anchor id (check-fixtures: '<anchor>:pass:<n>') or '*'; no network")
    parser.add_argument("--repository-root", default=".", help=REPO_ROOT_HELP)


class _UsageError(Exception):
    pass


class _JsonArgumentParser(argparse.ArgumentParser):
    """Usage errors become a VALIDATION.INPUT envelope (exit 2) instead of argparse's text on stderr."""

    def error(self, message):  # argparse hook; never returns
        raise _UsageError(f"{self.prog}: {message}")


def build_parser() -> argparse.ArgumentParser:
    p = _JsonArgumentParser(prog="jev_judge.py", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True, parser_class=_JsonArgumentParser)

    a = sub.add_parser("add", help="append an anchor record (lints the statement, refuses likely duplicates); data.added")
    a.add_argument("--anchors", required=True)
    a.add_argument("--statement", required=True, help="the testable anchor text, written by the orchestrator and approved by the user")
    a.add_argument("--quote", default="", help="the user's verbatim words (required for ANC; defaults to the statement for REQ/ADR)")
    a.add_argument("--kind", choices=KINDS, default="directive",
                   help="directive: compliance, conflict and relevance all gate; prohibition: compliance is not asked of the verdict; "
                        "structural: skipped by Jev at the plan gate (deterministic_only), judged at the dev gate")
    a.add_argument("--allow-similar", action="store_true", help="record the anchor even when it looks like an existing one (data.refused.code ANCHOR.DUPLICATE_CANDIDATE)")
    a.add_argument("--strict", action="store_true", help="refuse the anchor when the testability lint has any warning (data.refused.code ANCHOR.LINT)")
    a.add_argument("--rubric", action="store_true", help="ask Jev the testability rubric first; refuse (data.refused.code ANCHOR.NOT_TESTABLE) unless it passes")
    a.add_argument("--allow-untestable", action="store_true", help="with --rubric: record the anchor even when the rubric says needs_work")
    a.add_argument("--answers-file", default="", help="with --rubric: fixture answers keyed 'candidate' or '*'; no network")
    a.add_argument("--repository-root", default=".", help="with --rubric: " + REPO_ROOT_HELP)
    a.add_argument("--model", default=DEFAULT_MODEL, help="with --rubric: Jev model")
    a.add_argument("--timeout", type=float, default=30.0, help="with --rubric: seconds per request")
    a.add_argument("--stated-by", default=os.environ.get("USER", "user"))
    a.add_argument("--stated-at", default="", help="ISO date; default today's local date")
    a.add_argument("--context", default="planning session")
    a.add_argument("--summary", default="", help="short paraphrase for tables only; never judged")
    a.add_argument("--scope", default="", help="where the anchor applies; feeds the relevance question")
    a.add_argument("--deterministic-check", default="", help="a code-level check to pair with the dev gate")
    a.add_argument("--source-type", choices=SOURCE_TYPES, default="ANC")
    a.add_argument("--source-ref", default="", help="REQ-014 / ADR-0007 when source type is REQ/ADR")
    a.add_argument("--status", choices=STATUSES, default="proposed", help="proposed or locked; retired is refused")
    a.add_argument("--plan", default="", help="used only when creating a new anchors.json; default: the name of the folder holding --anchors (phase-b)")
    a.add_argument("--intent-ref", default="", help="used only when creating a new anchors.json; default: intent.md beside --anchors")
    a.set_defaults(func=cmd_add)

    u = sub.add_parser("update", help="edit statement, scope, summary, deterministic check or fixtures (never the quote)")
    u.add_argument("--anchors", required=True)
    u.add_argument("--id", required=True)
    u.add_argument("--statement", help="reword the testable anchor text (version bump; previous_statement kept in history)")
    u.add_argument("--strict", action="store_true", help="refuse when the testability lint has any warning (data.updated false, data.refused.code ANCHOR.LINT)")
    u.add_argument("--kind", choices=KINDS, help="directive, prohibition or structural (see `add --help`)")
    u.add_argument("--scope")
    u.add_argument("--summary")
    u.add_argument("--deterministic-check")
    u.add_argument("--fixture-pass", action="append", help="a sentence a bead would say if it satisfied the anchor; repeatable")
    u.add_argument("--fixture-fail", action="append", help="a sentence a bead would say if it violated the anchor; repeatable")
    u.add_argument("--clear-fixtures", action="store_true", help="drop existing fixtures before adding the given ones")
    u.add_argument("--quote", help=argparse.SUPPRESS)  # refused: the quote is verbatim provenance
    u.set_defaults(func=cmd_update)

    rt = sub.add_parser("retire", help="retire an anchor (status retired, version bump, history event)")
    rt.add_argument("--anchors", required=True)
    rt.add_argument("--id", required=True)
    rt.add_argument("--replaced-by", default="", help="the anchor that carries the changed directive")
    rt.add_argument("--reason", default="")
    rt.set_defaults(func=cmd_retire)

    ex = sub.add_parser("exclude", help="record that an anchor does not apply to one plan item (judge reports it as excluded); no version bump")
    ex.add_argument("--anchors", required=True)
    ex.add_argument("--id", default="", help="anchor id")
    ex.add_argument("--item", default=None, help="plan item id, exactly as judge --item-id gets it")
    ex.add_argument("--reason", default="", help="one sentence: why the anchor does not apply to this item (required unless --remove)")
    ex.add_argument("--remove", action="store_true", help="drop the exclusion for --item")
    ex.add_argument("--from", dest="from_file", default="", metavar="FILE.jsonl",
                    help="apply many at once: one {id, item, reason} or {id, item, remove: true} per line; all or nothing")
    ex.set_defaults(func=cmd_exclude)

    v = sub.add_parser("validate", help="check anchors.json; data.valid false with data.problems when it has problems")
    v.add_argument("--anchors", required=True)
    v.set_defaults(func=cmd_validate)

    ln = sub.add_parser("lint", help="testability warnings for anchor statements; data.clean false when any")
    ln.add_argument("--anchors", required=True)
    ln.add_argument("--id", default="", help="one anchor id")
    ln.add_argument("--all", action="store_true", help="every non-retired anchor (the default when --id is not given)")
    ln.add_argument("--plan-items", default="", help="bd show JSON or a JSON list of ids: warn on exclusions naming another item")
    ln.add_argument("--plan-item", action="append", help="a plan item id for the exclusion check; repeatable")
    ln.set_defaults(func=cmd_lint)

    dd = sub.add_parser("dedupe", help="find pairs of similar anchors by token Jaccard, optionally confirmed by Jev; data.likely_duplicates lists them")
    dd.add_argument("--anchors", required=True)
    dd.add_argument("--threshold", type=float, default=DEDUPE_DEFAULT_THRESHOLD, help="report pairs at or above this token Jaccard")
    dd.add_argument("--jev", action="store_true", help="ask Jev whether each reported pair is the same constraint")
    dd.add_argument("--dry-run", action="store_true", help="with --jev: show the requests; call nothing")
    dd.add_argument("--answers-file", default="", help="fixture answers keyed '<a>|<b>' or '*' (implies --jev); no network")
    dd.add_argument("--repository-root", default=".", help="with --jev: " + REPO_ROOT_HELP)
    dd.add_argument("--model", default=DEFAULT_MODEL)
    dd.add_argument("--timeout", type=float, default=30.0)
    dd.set_defaults(func=cmd_dedupe)

    rb = sub.add_parser("rubric", help="judge anchor statements against the testability rubric (lint + Jev); data.pass false unless all testable")
    rb.add_argument("--anchors", default="", help="anchors.json; with --id or --all")
    rb.add_argument("--id", default="", help="one anchor id")
    rb.add_argument("--all", action="store_true", help="every non-retired anchor (the default with --anchors)")
    rb.add_argument("--statement", help="judge a candidate not yet in the record (id 'candidate'); needs --quote")
    rb.add_argument("--quote", default="", help="the candidate's verbatim quote")
    rb.add_argument("--scope", default="", help="the candidate's scope")
    rb.add_argument("--threshold", type=float, default=DEFAULT_THRESHOLD, help="minimum confidence for a yes to count")
    rb.add_argument("--dry-run", action="store_true", help="show the requests; call nothing")
    rb.add_argument("--answers-file", default="", help="fixture answers keyed by anchor id ('candidate' for --statement) or '*'; no network")
    rb.add_argument("--repository-root", default=".", help=REPO_ROOT_HELP)
    rb.add_argument("--model", default=DEFAULT_MODEL)
    rb.add_argument("--timeout", type=float, default=30.0)
    rb.set_defaults(func=cmd_rubric)

    r = sub.add_parser("render", help="write anchors.md")
    r.add_argument("--anchors", required=True)
    r.add_argument("--out", default="")
    r.set_defaults(func=cmd_render)

    sb = sub.add_parser("set-beads", help="record bead ids on an anchor")
    sb.add_argument("--anchors", required=True)
    sb.add_argument("--id", required=True)
    sb.add_argument("--anchor-bead", default="")
    sb.add_argument("--plan-bead", default="")
    sb.add_argument("--dev-bead", default="")
    sb.set_defaults(func=cmd_set_beads)

    pr = sub.add_parser("promote", help="promote an anchor to an ADR")
    pr.add_argument("--anchors", required=True)
    pr.add_argument("--id", required=True)
    pr.add_argument("--adr", required=True)
    pr.set_defaults(func=cmd_promote)

    ci = sub.add_parser("compose-item", help="build the labelled item text from bd JSON")
    ci.add_argument("--show-json", required=True, help="output of `bd show <id> --json` (file, or - for stdin)")
    ci.add_argument("--children-json", default="", help="output of `bd children <id> --json`; closed children are appended (ignored with --for plan)")
    ci.add_argument("--sections", default="", help=f"comma list from {list(SECTION_KEYS)}; default all non-empty sections")
    ci.add_argument("--for", dest="for_gate", choices=tuple(COMPOSE_FOR), default="",
                    help="plan: title, description and acceptance criteria (what Jev reads; identical verdicts to the full text, 2.6x fewer tokens); "
                         "dev: every section plus closed children")
    ci.add_argument("--out", default="", help="write the text here; without it the text is returned in data.text")
    ci.set_defaults(func=cmd_compose_item)

    ap = sub.add_parser("agent-preflight", help="validate agent paths and identifiers before reading or sending data")
    ap.add_argument("--workspace-root", required=True, help="absolute root from `git rev-parse --show-toplevel`")
    ap.add_argument("--script", required=True, help="selected installed jev_judge.py; must equal this running script and be the nearest per-folder install")
    ap.add_argument("--anchors", required=True, help="anchors.json inside the workspace")
    ap.add_argument("--run-dir", default="", help="output directory inside the workspace; default beside anchors.json")
    ap.add_argument("--evidence", action="append", default=[], help="existing evidence file inside the workspace; repeatable")
    ap.add_argument("--identifier", action="append", default=[], help="bead, sprint or anchor id; repeatable")
    ap.add_argument("--pr-number", default="", help="positive decimal GitHub pull-request number")
    ap.add_argument("--correlation-id", default="", help="safe correlation id used to derive a temporary work key")
    ap.set_defaults(func=cmd_agent_preflight)

    sh = sub.add_parser("shape", help="size and shape metrics of plan beads; informs, never gates")
    sh.add_argument("--beads", default="", help="directory of `bd show --json` files")
    sh.add_argument("--children-json", default="", help="output of `bd children <id> --json`: every child is shaped")
    sh.add_argument("--show-json", action="append", help="output of `bd show <id> --json`; repeatable")
    sh.add_argument("--anchors", default="", help="anchors.json: report unknown ANC references and anchors no bead references")
    sh.add_argument("--max-code-lines", type=int, default=DEFAULT_MAX_CODE_LINES, help="chars_after_reduction reduces every fenced block longer than this (the plan gate's floor)")
    sh.add_argument("--format", choices=("json", "md"), default="json")
    sh.set_defaults(func=cmd_shape)

    j = sub.add_parser("judge", help="judge one item against anchors; data.summary.pass")
    add_judge_options(j, with_item=True)
    j.set_defaults(func=cmd_judge)

    cf = sub.add_parser("check-fixtures", help="judge each anchor's fixture sentences; pass -> satisfied, fail -> not_addressed or contradicted; data.pass")
    add_judge_options(cf, with_item=False)
    cf.set_defaults(func=cmd_check_fixtures)

    rp = sub.add_parser("report", help="render a run")
    rp.add_argument("--run", required=True)
    rp.add_argument("--all", action="store_true", help="include satisfied and not-applicable rows")
    rp.add_argument("--format", choices=("json", "md"), default="json")
    rp.set_defaults(func=cmd_report)

    cv = sub.add_parser("coverage", help="anchors x items matrix")
    cv.add_argument("--runs", nargs="+", required=True, help="run files or directories")
    cv.add_argument("--gate", choices=("plan", "dev"), default="")
    cv.add_argument("--format", choices=("json", "md"), default="json")
    cv.add_argument("--anchors", default="", help="anchors.json: apply its current exclusions to the runs (one recorded after a run applies; one removed since is listed to re-judge)")
    cv.set_defaults(func=cmd_coverage)

    d = sub.add_parser("diff", help="compare two runs")
    d.add_argument("--prev", required=True)
    d.add_argument("--next", required=True)
    d.add_argument("--expected", default="", help="JSON {anchor_id: verdict} from a human reading; reports corrections and regressions against it")
    d.set_defaults(func=cmd_diff)
    return p


CANCELED_EXIT = 130


def canceled_envelope() -> dict:
    """The interrupt envelope: typed like every error, plus the canceled / aborted_by flags."""
    out = JudgeError("CANCELED", "interrupted before the command finished", recoverable=True,
                     action="Rerun the command; a judge run is written only when it completes, so nothing partial was recorded.").to_json()
    out.update({"canceled": True, "aborted_by": "user"})
    return out


def main(argv=None) -> int:
    """0 when the command ran (its outcome, negative or not, is in data); 2 when it could not; 130 on interrupt."""
    try:
        return _main(argv)
    except KeyboardInterrupt:
        emit(canceled_envelope())
        return CANCELED_EXIT


def _main(argv=None) -> int:
    try:
        args = build_parser().parse_args(argv)
    except _UsageError as exc:
        emit(JudgeError("VALIDATION.INPUT", str(exc), action="Run 'jev_judge.py <subcommand> --help' for the accepted arguments.").to_json())
        return 2
    fmt = getattr(args, "format", "json")
    try:
        outcome = args.func(args)
    except Exception as exc:  # noqa: BLE001 - a JSON error object beats a traceback for the calling agent
        emit(JudgeError("SCRIPT.INTERNAL", f"{type(exc).__name__}: {exc}",
                        action="Report this with the command line; check the JSON inputs against references/anchor-document.md.").to_json())
        return 2
    if isinstance(outcome, Err):
        emit(outcome.error.to_json())
        return 2
    emit(outcome.value, fmt)
    return 0


if __name__ == "__main__":
    try:
        utf8_stdio()
        code = main()
    except KeyboardInterrupt:  # between the imports and main's own handler
        emit(canceled_envelope())
        code = CANCELED_EXIT
    sys.exit(code)
