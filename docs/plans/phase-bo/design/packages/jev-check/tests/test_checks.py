import json
import os
import subprocess
from pathlib import Path

import pytest

from jev_check import (Backend, Question, JevCheckError, SessionStore, build_request, check_llm, extract_python_branches,
                       extract_table_column, git_show, load_check, run_check)
from jev_check.extractors import GitShow
from jev_check.llm import api_text, cli_text

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
HAIKU = Backend("haiku", "claude-haiku-5-5", "api", "ANTHROPIC_API_KEY")


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


class FakeLlm:
    def __init__(self, reply: str) -> None:
        self.reply, self.prompts = reply, list[str]()

    def complete(self, backend: Backend, system: str, prompt: str) -> str:
        self.prompts.append(prompt)
        return self.reply


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


# Haiku / Luna
def test_llm_reply_maps_to_answers() -> None:
    qs: dict[str, Question] = {"q": {"type": "choice", "instructions": "i", "criteria": {"yes": "y", "no": "n"}}}
    llm = FakeLlm('```json\n{"answers": {"q": {"choice": "no", "confidence": 0.7}}}\n```')
    assert check_llm(HAIKU, {"s": 1}, qs, llm) == {
        "q": {"choice": "no", "confidence": 0.7, "probabilities": {"yes": 0.0, "no": 0.7}}}
    assert json.loads(llm.prompts[0]) == {"state": {"s": 1}, "questions": qs}


@pytest.mark.parametrize("reply", ["not json", '{"answers": {}}', '{"answers": {"q": {"choice": "maybe", "confidence": 1}}}'])
def test_llm_bad_reply_is_typed(reply: str) -> None:
    qs: dict[str, Question] = {"q": {"type": "choice", "instructions": "i", "criteria": {"yes": "y", "no": "n"}}}
    with pytest.raises(JevCheckError) as err:
        check_llm(HAIKU, {}, qs, FakeLlm(reply))
    assert err.value.code == "RESPONSE_INVALID"


def test_api_and_cli_text_extraction() -> None:
    luna = Backend("luna", "gpt-6-luna", "cli", "OPENAI_API_KEY")
    assert api_text(HAIKU, b'{"content": [{"type": "text", "text": "A"}, {"type": "text", "text": "B"}]}') == "AB"
    assert api_text(luna, b'{"output": [{"type": "message", "content": [{"type": "output_text", "text": "C"}]}]}') == "C"
    assert cli_text(HAIKU, '{"result": "D", "session_id": "x"}') == "D"
    assert cli_text(luna, '{"type":"item.completed","item":{"type":"agent_message","text":"E"}}\n') == "E"


def test_api_key_missing_sends_nothing(monkeypatch: pytest.MonkeyPatch) -> None:
    from jev_check import ApiTransport
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    with pytest.raises(JevCheckError) as err:
        ApiTransport().complete(HAIKU, "s", "p")
    assert err.value.code == "KEY_MISSING"


# sessions
def test_run_writes_session_and_ask_appends(repo: tuple[Path, str], tmp_path: Path) -> None:
    root, rev = repo
    now = [1000.0]
    store = SessionStore(tmp_path / "sessions", clock=lambda: now[0])
    reply = json.dumps({"answers": {k: {"choice": "yes", "confidence": 0.5} for k in
                                    ["implemented:T-1", "implemented:T-2", "listed:hook.py:3", "listed:hook.py:4",
                                     "listed:hook.py:5"]}})
    result = run_check(load_check(root / "check.toml"), rev, "haiku", show=shower(root), llm=FakeLlm(reply), store=store)
    assert result.backend == "haiku" and result.model == "claude-haiku-5-5" and result.rev == rev
    llm = FakeLlm("because line 4 refuses X")
    assert store.ask(result.session_id, HAIKU, "why is T-2 yes?", llm) == "because line 4 refuses X"
    sent = json.loads(llm.prompts[0])
    assert sent["follow_up"] == "why is T-2 yes?" and sent["request"]["state"]["hook"] == HOOK
    assert sent["answers"]["implemented:T-1"]["choice"] == "yes"
    saved = store.read(result.session_id)
    assert saved["followups"] == [{"backend": "haiku", "model": "claude-haiku-5-5", "question": "why is T-2 yes?",
                                   "answer": "because line 4 refuses X"}]
    now[0] += 8 * 86400
    with pytest.raises(JevCheckError) as err:
        store.read(result.session_id)
    assert err.value.code == "SESSION_EXPIRED"
