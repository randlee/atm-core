"""Request {model, state, questions} -> answers. One check = one request; busy-only retry; no splitting."""
import http.client
import json
import os
import time
import tomllib
from collections.abc import Callable
from dataclasses import dataclass
from importlib.resources import files
from typing import Any, Protocol, TypedDict, cast

BUSY = (429, 502, 529)


class Question(TypedDict):
    type: str
    instructions: str
    criteria: dict[str, str]


class Answer(TypedDict):
    choice: str
    confidence: float
    probabilities: dict[str, float]


@dataclass(frozen=True)
class Response:
    status: int
    body: bytes
    retry_after: str | None = None


class Transport(Protocol):
    def send(self, body: bytes) -> Response: ...


@dataclass(frozen=True)
class Config:
    model: str
    host: str
    path: str
    max_request_bytes: int
    attempts: int
    timeout_seconds: float


class JevCheckError(Exception):
    """code is one of INPUT_TOO_LARGE, INPUT_INVALID, KEY_MISSING, JEV_UNAVAILABLE, RESPONSE_INVALID."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(f"{code}: {message}")
        self.code = code


def load_config() -> Config:
    raw = tomllib.loads(files("jev_check").joinpath("jev.toml").read_text())
    return Config(model=str(raw["model"]), host=str(raw["host"]), path=str(raw["path"]),
                  max_request_bytes=int(raw["max_request_bytes"]), attempts=int(raw["attempts"]),
                  timeout_seconds=float(raw["timeout_seconds"]))


class HttpsTransport:
    def __init__(self, config: Config, key: str | None = None) -> None:
        self.config = config
        self.key = (key if key is not None else os.environ.get("TYPESAFE_API_KEY", "")).strip()
        if not self.key:
            raise JevCheckError("KEY_MISSING", "TYPESAFE_API_KEY is missing; no Jev request was sent")

    def send(self, body: bytes) -> Response:
        conn = http.client.HTTPSConnection(self.config.host, timeout=self.config.timeout_seconds)
        try:
            conn.request("POST", self.config.path, body=body,
                         headers={"Authorization": "Bearer " + self.key, "Content-Type": "application/json"})
            resp = conn.getresponse()
            return Response(resp.status, resp.read(), resp.getheader("Retry-After"))
        except OSError as exc:
            raise JevCheckError("JEV_UNAVAILABLE", f"Jev connection failed: {type(exc).__name__}") from None
        finally:
            conn.close()


def encode_request(config: Config, state: Any, questions: dict[str, Question]) -> bytes:
    if not questions:
        raise JevCheckError("INPUT_INVALID", "a check needs at least one question")
    for key, q in questions.items():
        if q["type"] != "choice" or len(q["criteria"]) < 2:
            raise JevCheckError("INPUT_INVALID", f"question {key} is not a choice question with two or more criteria")
    body = json.dumps({"model": config.model, "state": state, "questions": questions},
                      sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    if len(body) > config.max_request_bytes:
        raise JevCheckError("INPUT_TOO_LARGE", f"request is {len(body)} bytes > {config.max_request_bytes}; not split")
    return body


def as_object(value: object) -> dict[str, object] | None:
    return cast(dict[str, object], value) if isinstance(value, dict) else None


def as_probability(value: object) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise JevCheckError("RESPONSE_INVALID", "Jev confidence or probability is not a number")
    return float(value)


def parse_answers(raw: bytes, config: Config, questions: dict[str, Question]) -> dict[str, Answer]:
    try:
        value: object = json.loads(raw)
    except ValueError:
        raise JevCheckError("RESPONSE_INVALID", "Jev response is not JSON") from None
    body = as_object(value)
    answers = as_object(body.get("answers")) if body is not None and body.get("model") == config.model else None
    if answers is None:
        raise JevCheckError("RESPONSE_INVALID", "unexpected Jev response model or shape")
    if set(answers) != set(questions):
        raise JevCheckError("RESPONSE_INVALID", "Jev answer ids differ from the question ids")
    out: dict[str, Answer] = {}
    for key, item in answers.items():
        a = as_object(item)
        probs = as_object(a.get("probabilities")) if a is not None else None
        choice = a.get("choice") if a is not None else None
        if a is None or probs is None or not isinstance(choice, str) or choice not in questions[key]["criteria"]:
            raise JevCheckError("RESPONSE_INVALID", f"answer {key} is not one of its criteria")
        out[key] = {"choice": choice, "confidence": as_probability(a.get("confidence")),
                    "probabilities": {k: as_probability(p) for k, p in probs.items()}}
    return out


def check(state: Any, questions: dict[str, Question], *, transport: Transport | None = None,
          config: Config | None = None, sleep: Callable[[float], None] = time.sleep) -> dict[str, Answer]:
    """One check = one request. Retries only a busy reply, up to config.attempts sends in total."""
    config = config or load_config()
    body = encode_request(config, state, questions)
    transport = transport or HttpsTransport(config)
    for attempt in range(1, config.attempts + 1):
        resp = transport.send(body)
        if resp.status in BUSY and attempt < config.attempts:
            sleep(retry_delay(resp.retry_after))
            continue
        if resp.status != 200:
            raise JevCheckError("JEV_UNAVAILABLE", f"Jev HTTP {resp.status}")
        return parse_answers(resp.body, config, questions)
    raise JevCheckError("JEV_UNAVAILABLE", "Jev busy; attempts exhausted")


def retry_delay(retry_after: str | None) -> float:
    try:
        return min(30.0, max(0.0, float(retry_after or 1)))
    except ValueError:
        return 30.0
