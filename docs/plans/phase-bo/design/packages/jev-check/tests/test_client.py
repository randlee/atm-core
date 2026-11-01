import json
from pathlib import Path

import pytest

from jev_check import Config, JevCheckError, Question, Response, check, load_config, load_question_set

CONFIG = Config(model="jev-test", host="jev.invalid", path="/v1/x", max_request_bytes=400, attempts=3,
                timeout_seconds=1)
QUESTIONS: dict[str, Question] = {"q": {"type": "choice", "instructions": "Is it?", "criteria": {"yes": "y", "no": "n"}}}
ANSWER = {"choice": "no", "confidence": 0.9, "probabilities": {"yes": 0.1, "no": 0.9}}


class FakeTransport:
    def __init__(self, *responses: Response) -> None:
        self.responses = list(responses)
        self.sent: list[bytes] = []

    def send(self, body: bytes) -> Response:
        self.sent.append(body)
        return self.responses.pop(0)


def ok(model: str = "jev-test") -> Response:
    return Response(200, json.dumps({"model": model, "answers": {"q": ANSWER}}).encode())


def test_one_check_is_one_request_byte_exact() -> None:
    t = FakeTransport(ok())
    answers = check({"code": "x = 1"}, QUESTIONS, transport=t, config=CONFIG)
    assert t.sent == [b'{"model":"jev-test","questions":{"q":{"criteria":{"no":"n","yes":"y"},'
                      b'"instructions":"Is it?","type":"choice"}},"state":{"code":"x = 1"}}']
    assert answers == {"q": ANSWER}


def test_oversized_input_is_refused_and_nothing_is_sent() -> None:
    t = FakeTransport(ok())
    with pytest.raises(JevCheckError) as err:
        check({"code": "x" * 500}, QUESTIONS, transport=t, config=CONFIG)
    assert err.value.code == "INPUT_TOO_LARGE"
    assert t.sent == []


def test_busy_is_retried_with_the_same_body() -> None:
    t, slept = FakeTransport(Response(529, b"", "2"), ok()), list[float]()
    check({"code": "x"}, QUESTIONS, transport=t, config=CONFIG, sleep=slept.append)
    assert len(t.sent) == 2 and t.sent[0] == t.sent[1] and slept == [2.0]


def test_non_busy_error_is_not_retried() -> None:
    t = FakeTransport(Response(500, b""), ok())
    with pytest.raises(JevCheckError) as err:
        check({"code": "x"}, QUESTIONS, transport=t, config=CONFIG, sleep=lambda _: None)
    assert err.value.code == "JEV_UNAVAILABLE" and len(t.sent) == 1


def test_busy_attempts_exhausted() -> None:
    t = FakeTransport(*[Response(429, b"")] * 3)
    with pytest.raises(JevCheckError) as err:
        check({"code": "x"}, QUESTIONS, transport=t, config=CONFIG, sleep=lambda _: None)
    assert err.value.code == "JEV_UNAVAILABLE" and len(t.sent) == 3


def test_answer_ids_must_match_questions() -> None:
    t = FakeTransport(Response(200, json.dumps({"model": "jev-test", "answers": {}}).encode()))
    with pytest.raises(JevCheckError) as err:
        check({"code": "x"}, QUESTIONS, transport=t, config=CONFIG)
    assert err.value.code == "RESPONSE_INVALID"


def test_packaged_config_holds_model_host_path() -> None:
    c = load_config()
    assert (c.model, c.host, c.path, c.max_request_bytes) == ("jev-1.13.0", "api.typesafe.ai", "/v1/systemone", 96000)


def test_question_set_expands_one_question_per_subject(tmp_path: Path) -> None:
    p = tmp_path / "gate.json"
    p.write_text(json.dumps({"schema": "plan-anchoring/questions/1", "questions": {
        "listed": {"type": "choice", "instructions": "Is {subject} listed?", "criteria": {"yes": "y", "no": "n"}}}}))
    qs = load_question_set(p).expand("listed", {"L3": "line 3", "L9": "line 9"})
    assert list(qs) == ["listed:L3", "listed:L9"]
    assert qs["listed:L9"]["instructions"] == "Is line 9 listed?"


def test_question_set_rejects_other_schema(tmp_path: Path) -> None:
    p = tmp_path / "bad.json"
    p.write_text(json.dumps({"schema": "other", "questions": {}}))
    with pytest.raises(JevCheckError) as err:
        load_question_set(p)
    assert err.value.code == "INPUT_INVALID"
