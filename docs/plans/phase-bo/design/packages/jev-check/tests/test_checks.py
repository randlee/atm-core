import hashlib
import json
import os
import subprocess
from pathlib import Path
from typing import Any

import pytest

from jev_check import (Backend, Question, JevCheckError, SessionStore, build_request, check_llm, extract_python_branches,
                       extract_table_column, git_show, load_check, payload_bytes, run_check, run_request)
from jev_check.extractors import GitShow
from jev_check.llm import api_key

HOOK = '''def hook(t):
    match t:
        case T.A:
            return HookReply.refused(Refusal(ErrorId.X))
        case T.B:
            return None
'''
SPEC = "# s\n\n## Transitions\n| id | transition |\n|---|---|\n| T-1 | A |\n| T-2 | B |\n\n## Errors\n| code |\n|---|\n| X |\n"
QSET = {"schema": "plan-anchoring/questions/1", "questions": {
    "implemented": {"type": "choice", "instructions": "{subject} implemented?", "criteria": {"yes": "y", "no": "n"}},
    "listed": {"type": "choice", "instructions": "{subject} listed?", "criteria": {"yes": "y", "no": "n"}}}}
CHECK = '''id = "task-close"
question_set = "q.json"
[inputs.hook]
kind = "file"
path = "hook.py"
[inputs.rows]
kind = "table-column"
path = "spec.md"
table = "Transitions"
column = "id"
[inputs.branches]
kind = "python-branches"
path = "hook.py"
[[expand]]
template = "implemented"
subjects = "rows"
[[expand]]
template = "listed"
subjects = "branches"
'''
HAIKU = Backend("haiku", "claude-haiku-5-5", "ANTHROPIC_API_KEY")


@pytest.fixture()
def repo(tmp_path: Path) -> tuple[Path, str]:
    """A fixture git repo at one fixed commit (fixed author, dates and content)."""
    env = {**os.environ, "GIT_AUTHOR_DATE": "2026-11-01T00:00:00Z", "GIT_COMMITTER_DATE": "2026-11-01T00:00:00Z",
           "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
    (tmp_path / "hook.py").write_text(HOOK)
    (tmp_path / "spec.md").write_text(SPEC)
    (tmp_path / "q.json").write_text(json.dumps(QSET))
    (tmp_path / "check.toml").write_text(CHECK)
    for cmd in (["git", "init", "-q"], ["git", "add", "."], ["git", "commit", "-q", "-m", "fixture"]):
        subprocess.run(cmd, cwd=tmp_path, env=env, check=True)
    rev = subprocess.run(["git", "rev-parse", "HEAD"], cwd=tmp_path, capture_output=True, text=True, check=True).stdout.strip()
    return tmp_path, rev


def shower(root: Path) -> GitShow:
    def show(rev: str, path: str) -> str:
        cwd = os.getcwd()
        os.chdir(root)
        try:
            return git_show(rev, path)
        finally:
            os.chdir(cwd)
    return show


class FakeHttp:
    """Answers count and message posts from canned replies; records every post."""

    def __init__(self, count: int, message: dict[str, Any]) -> None:
        self.count, self.message = count, message
        self.posts: list[tuple[str, str, dict[str, Any]]] = []

    def post(self, host: str, path: str, headers: dict[str, str], body: bytes) -> tuple[int, bytes]:
        self.posts.append((host, path, json.loads(body)))
        if path.endswith(("count_tokens", "input_tokens")):
            return 200, json.dumps({"input_tokens": self.count}).encode()
        return 200, json.dumps(self.message).encode()


def haiku_message(text: str, input_tokens: int, extra: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    return {"content": [*(extra or []), {"type": "text", "text": text}],
            "usage": {"input_tokens": input_tokens, "output_tokens": 9}}


Q1: dict[str, Question] = {"q": {"type": "choice", "instructions": "i", "criteria": {"yes": "y", "no": "n"}}}
def reply_of(choices: dict[str, str]) -> str:
    return json.dumps({"answers": [{"question": k, "choice": c, "confidence": 0.7, "probabilities": [
        {"criterion": "yes", "p": 0.3 if c == "no" else 0.7}, {"criterion": "no", "p": 0.7 if c == "no" else 0.3}]}
        for k, c in choices.items()]})


ONE = reply_of({"q": "no"})


@pytest.fixture()
def keys(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    monkeypatch.setenv("OPENAI_API_KEY", "k")


# config
@pytest.mark.parametrize("text, needle", [
    ('extra = 1\n' + CHECK, "unknown key"),
    (CHECK.replace('kind = "file"', 'kind = "url"'), "unknown source kind"),
    (CHECK.replace('subjects = "rows"', 'subjects = "nope"'), "input name"),
])
def test_config_load_errors(tmp_path: Path, text: str, needle: str) -> None:
    p = tmp_path / "c.toml"
    p.write_text(text)
    with pytest.raises(JevCheckError) as err:
        load_check(p)
    assert err.value.code == "CONFIG_INVALID" and needle in str(err.value)


def test_missing_input_at_rev(repo: tuple[Path, str]) -> None:
    root, rev = repo
    cfg = load_check(root / "check.toml")
    cfg["inputs"]["hook"]["path"] = "gone.py"
    with pytest.raises(JevCheckError) as err:
        build_request(cfg, rev, shower(root))
    assert err.value.code == "INPUT_MISSING"


# extractors
def test_python_branches_lists_arms_and_refusals(repo: tuple[Path, str]) -> None:
    root, rev = repo
    assert extract_python_branches(shower(root)(rev, "hook.py"), "hook.py") == {
        "hook.py:3": "case T.A", "hook.py:4": "return HookReply.refused(Refusal(ErrorId.X))", "hook.py:5": "case T.B"}


def test_table_column_rows(repo: tuple[Path, str]) -> None:
    root, rev = repo
    assert extract_table_column(shower(root)(rev, "spec.md"), "spec.md", "Transitions", "id") == {
        "T-1": "| T-1 | A |", "T-2": "| T-2 | B |"}


def test_empty_extraction_is_typed() -> None:
    with pytest.raises(JevCheckError) as err:
        extract_python_branches("x = 1\n", "a.py")
    assert err.value.code == "EXTRACTION_EMPTY"
    with pytest.raises(JevCheckError) as err:
        extract_table_column("# none\n", "s.md", "Transitions", "id")
    assert err.value.code == "EXTRACTION_EMPTY"


# request
def test_request_is_one_question_per_row_and_branch(repo: tuple[Path, str]) -> None:
    root, rev = repo
    state, questions = build_request(load_check(root / "check.toml"), rev, shower(root))
    assert state["hook"] == HOOK
    assert list(questions) == ["implemented:T-1", "implemented:T-2", "listed:hook.py:3", "listed:hook.py:4",
                               "listed:hook.py:5"]
    assert questions["listed:hook.py:4"]["instructions"] == (
        "Subject `branches['hook.py:4']`: return HookReply.refused(Refusal(ErrorId.X)) listed?")


# Haiku / Luna, API-direct
def test_haiku_request_is_the_payload_alone(keys: None) -> None:
    http = FakeHttp(100, haiku_message(ONE, 100))
    answers, reply = check_llm(HAIKU, {"s": 1}, Q1, http)
    assert answers == {"q": {"choice": "no", "confidence": 0.7, "probabilities": {"yes": 0.3, "no": 0.7}}}
    (h1, p1, count), (h2, p2, body) = http.posts
    assert (h1, p1, h2, p2) == ("api.anthropic.com", "/v1/messages/count_tokens", "api.anthropic.com", "/v1/messages")
    assert body["messages"] == [{"role": "user", "content": payload_bytes({"s": 1}, Q1).decode()}]
    assert set(body) == {"model", "max_tokens", "messages", "output_config"}  # no system, no tools
    item = body["output_config"]["format"]["schema"]["properties"]["answers"]["items"]
    assert item["required"] == ["question", "choice", "confidence", "probabilities"]
    assert count == {k: v for k, v in body.items() if k != "max_tokens"}
    assert (reply.input_tokens, reply.prompt_tokens, reply.commands) == (100, 100, 0)


def test_luna_request_is_the_payload_alone(keys: None) -> None:
    luna = Backend("luna", "gpt-6-luna", "OPENAI_API_KEY")
    message: dict[str, Any] = {"output": [{"type": "reasoning"}, {"type": "message", "content": [{"type": "output_text", "text": ONE}]}],
               "usage": {"input_tokens": 50}}
    http = FakeHttp(50, message)
    _, reply = check_llm(luna, {"s": 1}, Q1, http)
    (_, p1, count), (_, p2, body) = http.posts
    assert (p1, p2) == ("/v1/responses/input_tokens", "/v1/responses")
    assert body["input"] == [{"role": "user", "content": payload_bytes({"s": 1}, Q1).decode()}]
    assert set(body) == {"model", "input", "text"} and count == body
    assert (reply.input_tokens, reply.prompt_tokens, reply.commands) == (50, 50, 0)


TOOL_USE: list[dict[str, Any]] = [{"type": "tool_use", "id": "t", "name": "x", "input": {}}]


@pytest.mark.parametrize("input_tokens, extra, leak, equal", [
    (1000, None, 0, True),
    (1010, None, 10, True),        # within 1% of 1000
    (12000, None, 11000, False),   # what claude -p added from CLAUDE.md and memory
    (1000, TOOL_USE, 0, False),
])
def test_equal_input_record(keys: None, input_tokens: int, extra: list[dict[str, Any]] | None, leak: int,
                            equal: bool, tmp_path: Path) -> None:
    http = FakeHttp(1000, haiku_message(ONE, input_tokens, extra))
    result = run_request("c", "r", "haiku", {"s": 1}, Q1, http=http, store=SessionStore(tmp_path))
    assert result.input.leak == leak and result.input.equal is equal
    assert result.input.prompt_sha256 == hashlib.sha256(payload_bytes({"s": 1}, Q1)).hexdigest()


@pytest.mark.parametrize("reply", [
    "not json", '{"answers": []}', reply_of({"q": "maybe"}), reply_of({"q": "no", "x": "no"}),
    '{"answers": [{"question": "q", "choice": "no", "confidence": 1, "probabilities": [{"criterion": "no", "p": 1}]}]}'])
def test_llm_bad_reply_is_typed(keys: None, reply: str) -> None:
    with pytest.raises(JevCheckError) as err:
        check_llm(HAIKU, {}, Q1, FakeHttp(1, haiku_message(reply, 1)))
    assert err.value.code == "RESPONSE_INVALID"


def test_api_key_missing_sends_nothing(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    http = FakeHttp(1, haiku_message(ONE, 1))
    with pytest.raises(JevCheckError) as err:
        check_llm(HAIKU, {}, Q1, http, lookup=lambda _: "")
    assert err.value.code == "KEY_MISSING" and http.posts == []


def test_api_key_env_first_then_keychain(monkeypatch: pytest.MonkeyPatch) -> None:
    asked: list[str] = []
    def lookup(service: str) -> str:
        asked.append(service)
        return "from-keychain"
    monkeypatch.setenv("ANTHROPIC_API_KEY", "from-env")
    assert api_key(HAIKU, lookup) == "from-env" and asked == []
    monkeypatch.delenv("ANTHROPIC_API_KEY")
    assert api_key(HAIKU, lookup) == "from-keychain" and asked == ["ANTHROPIC_API_KEY"]


# sessions
def test_run_writes_session_and_ask_continues_it(keys: None, repo: tuple[Path, str], tmp_path: Path) -> None:
    root, rev = repo
    now = [1000.0]
    store = SessionStore(tmp_path / "sessions", clock=lambda: now[0])
    reply = reply_of({k: "yes" for k in ["implemented:T-1", "implemented:T-2", "listed:hook.py:3", "listed:hook.py:4",
                                         "listed:hook.py:5"]})
    result = run_check(load_check(root / "check.toml"), rev, "haiku", show=shower(root),
                       http=FakeHttp(10, haiku_message(reply, 10)), store=store)
    assert result.backend == "haiku" and result.model == "claude-haiku-5-5" and result.rev == rev
    http = FakeHttp(20, haiku_message("because line 4 refuses X", 20))
    assert store.ask(result.session_id, "why is T-2 yes?", http) == "because line 4 refuses X"
    body = http.posts[1][2]
    messages: list[dict[str, str]] = body["messages"]
    assert [m["role"] for m in messages] == ["user", "assistant", "user"]
    assert messages[1]["content"] == reply and messages[2]["content"] == "why is T-2 yes?"
    assert "output_config" not in body and "system" not in body
    saved = store.read(result.session_id)
    assert saved["followups"] == [{"question": "why is T-2 yes?", "answer": "because line 4 refuses X"}]
    assert saved["input"] == {"prompt_sha256": result.input.prompt_sha256, "input_tokens": 10, "prompt_tokens": 10,
                              "leak": 0, "commands": 0, "equal": True}
    now[0] += 8 * 86400
    with pytest.raises(JevCheckError) as err:
        store.read(result.session_id)
    assert err.value.code == "SESSION_EXPIRED"
