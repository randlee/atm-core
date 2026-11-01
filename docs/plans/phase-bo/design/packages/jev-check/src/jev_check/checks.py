"""Check config (one TOML per check) -> one request -> JevCheckResult. Unknown keys and source kinds are errors."""
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, cast

from jev_check.client import Answer, JevCheckError, Question, as_object, check, load_config
from jev_check.extractors import (GitShow, extract_file, extract_python_branches, extract_table_column, git_show)
from jev_check.llm import LlmTransport, check_llm, load_backend
from jev_check.questions import load_question_set
from jev_check.sessions import SessionStore

TOP_KEYS = {"id", "question_set", "inputs", "expand"}
SOURCE_KEYS = {"file": {"kind", "path"}, "table-column": {"kind", "path", "table", "column"},
               "python-branches": {"kind", "path"}}


@dataclass(frozen=True)
class JevCheckResult:
    check_id: str
    rev: str
    backend: str
    model: str
    session_id: str
    answers: dict[str, Answer]


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
              llm: LlmTransport | None = None, store: SessionStore | None = None) -> JevCheckResult:
    state, questions = build_request(cfg, rev, show)
    if backend == "jev":
        model, answers = load_config().model, check(state, questions)
    else:
        b = load_backend(backend)
        model, answers = b.model, check_llm(b, state, questions, llm)
    sid = (store or SessionStore.default()).write(
        {"check_id": cfg["id"], "rev": rev, "backend": backend, "model": model,
         "request": {"state": state, "questions": questions}, "answers": answers})
    return JevCheckResult(cfg["id"], rev, backend, model, sid, answers)
