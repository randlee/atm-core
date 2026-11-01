"""Check config (one TOML per check) -> one request -> JevCheckResult. Unknown keys and source kinds are errors."""
import dataclasses
import hashlib
import tomllib
from dataclasses import dataclass
from importlib.resources import files
from pathlib import Path
from typing import Any, cast

from jev_check.client import Answer, JevCheckError, Question, as_object, check, load_config, payload_bytes
from jev_check.extractors import (GitShow, extract_file, extract_python_branches, extract_table_column, git_show)
from jev_check.llm import Http, check_llm, load_backend
from jev_check.questions import load_question_set
from jev_check.sessions import SessionStore

TOP_KEYS = {"id", "question_set", "inputs", "expand"}
SOURCE_KEYS = {"file": {"kind", "path"}, "table-column": {"kind", "path", "table", "column"},
               "python-branches": {"kind", "path"}}


@dataclass(frozen=True)
class InputRecord:
    """Equal input, recorded on every run. A tuning comparison is valid only when every side is equal.

    prompt_sha256: sha256 of the {questions, state} payload bytes (the same for every backend).
    prompt_tokens: the vendor's count of the request body; None for Jev, which has no count endpoint.
    leak: input_tokens - prompt_tokens, what the backend read beyond the request; None for Jev.
    commands: tool or command calls in the reply (Jev has none: its API takes no tools).
    equal: commands == 0 and leak <= leak_tolerance * prompt_tokens (Jev: commands == 0).
    """
    prompt_sha256: str
    input_tokens: int | None
    prompt_tokens: int | None
    leak: int | None
    commands: int
    equal: bool


@dataclass(frozen=True)
class JevCheckResult:
    check_id: str
    rev: str
    backend: str
    model: str
    session_id: str
    answers: dict[str, Answer]
    input: InputRecord


def leak_tolerance() -> float:
    return float(tomllib.loads(files("jev_check").joinpath("jev.toml").read_text())["leak_tolerance"])


def input_record(state: Any, questions: dict[str, Question], input_tokens: int | None, prompt_tokens: int | None,
                 commands: int) -> InputRecord:
    sha = hashlib.sha256(payload_bytes(state, questions)).hexdigest()
    if prompt_tokens is None:
        return InputRecord(sha, input_tokens, None, None, commands, commands == 0)
    leak = (input_tokens or 0) - prompt_tokens
    return InputRecord(sha, input_tokens, prompt_tokens, leak, commands,
                       commands == 0 and abs(leak) <= leak_tolerance() * prompt_tokens)


def load_check(path: Path) -> dict[str, Any]:
    try:
        raw: dict[str, Any] = tomllib.loads(path.read_text())
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise JevCheckError("CONFIG_INVALID", f"cannot read check {path}: {exc}") from None
    if unknown := set(raw) - TOP_KEYS:
        raise JevCheckError("CONFIG_INVALID", f"{path}: unknown key(s) {sorted(unknown)}")
    if not isinstance(raw.get("id"), str) or not isinstance(raw.get("question_set"), str):
        raise JevCheckError("CONFIG_INVALID", f"{path}: id and question_set are required")
    inputs = as_object(raw.get("inputs"))
    if not inputs:
        raise JevCheckError("CONFIG_INVALID", f"{path}: no [inputs]")
    for name, src in inputs.items():
        s = as_object(src) or {}
        kind = s.get("kind")
        if not isinstance(kind, str) or kind not in SOURCE_KEYS:
            raise JevCheckError("CONFIG_INVALID", f"{path}: input {name} has unknown source kind {kind!r}")
        if set(s) != SOURCE_KEYS[kind]:
            raise JevCheckError("CONFIG_INVALID", f"{path}: input {name} ({kind}) needs exactly {sorted(SOURCE_KEYS[kind])}")
    expand_raw: object = raw.get("expand", [])
    expand = cast(list[object], expand_raw) if isinstance(expand_raw, list) else []
    for e in expand:
        entry = as_object(e) or {}
        if set(entry) != {"template", "subjects"} or entry.get("subjects") not in inputs:
            raise JevCheckError("CONFIG_INVALID", f"{path}: [[expand]] needs template and an input name as subjects")
    raw["question_set"] = str((path.parent / raw["question_set"]).resolve())
    return raw


def build_request(cfg: dict[str, Any], rev: str, show: GitShow = git_show) -> tuple[dict[str, Any], dict[str, Question]]:
    state: dict[str, Any] = {}
    for name, src in cfg["inputs"].items():
        text = show(rev, src["path"])
        if src["kind"] == "file":
            state[name] = extract_file(text, src["path"])
        elif src["kind"] == "table-column":
            state[name] = extract_table_column(text, src["path"], src["table"], src["column"])
        else:
            state[name] = extract_python_branches(text, src["path"])
    qs = load_question_set(Path(cfg["question_set"]))
    questions: dict[str, Question] = {}
    for e in cfg.get("expand", []):
        subjects: dict[str, str] = state[e["subjects"]]
        questions |= qs.expand(e["template"], {sid: f"Subject `{e['subjects']}[{sid!r}]`: {text}"
                                               for sid, text in subjects.items()})
    return state, questions


def run_check(cfg: dict[str, Any], rev: str, backend: str, *, show: GitShow = git_show,
              http: Http | None = None, store: SessionStore | None = None) -> JevCheckResult:
    state, questions = build_request(cfg, rev, show)
    return run_request(cfg["id"], rev, backend, state, questions, http=http, store=store)


def run_request(check_id: str, rev: str, backend: str, state: Any, questions: dict[str, Question], *,
                http: Http | None = None, store: SessionStore | None = None) -> JevCheckResult:
    """One request on one backend; the session keeps the request, the reply text and the input record."""
    reply_text = None
    if backend == "jev":
        jev = check(state, questions)
        model, answers, record = load_config().model, jev.answers, input_record(state, questions, jev.input_tokens, None, 0)
    else:
        b = load_backend(backend)
        answers, reply = check_llm(b, state, questions, http)
        model, reply_text = b.model, reply.text
        record = input_record(state, questions, reply.input_tokens, reply.prompt_tokens, reply.commands)
    sid = (store or SessionStore.default()).write(
        {"check_id": check_id, "rev": rev, "backend": backend, "model": model,
         "request": {"state": state, "questions": questions}, "reply_text": reply_text, "answers": answers,
         "input": dataclasses.asdict(record)})
    return JevCheckResult(check_id, rev, backend, model, sid, answers, record)
