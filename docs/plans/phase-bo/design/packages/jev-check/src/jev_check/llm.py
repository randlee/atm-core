"""Haiku and Luna, API-direct: the user message is the payload bytes Jev gets; no system prompt, no tools.

The reply shape is forced with the vendor's JSON-schema output format. Each run also counts the request's
tokens on the vendor's count endpoint (same body), so input_tokens - prompt_tokens shows anything added.
"""
import http.client
import json
import os
import subprocess
import tomllib
from collections.abc import Callable
from dataclasses import dataclass
from importlib.resources import files
from typing import Any, Protocol, cast

from jev_check.client import Answer, JevCheckError, Question, as_object, as_probability, payload_bytes

ENDPOINTS = {"haiku": ("api.anthropic.com", "/v1/messages", "/v1/messages/count_tokens"),
             "luna": ("api.openai.com", "/v1/responses", "/v1/responses/input_tokens")}
ANTHROPIC_VERSION = "2026-10-21"
MAX_TOKENS = 16000
Turn = tuple[str, str]  # (role, text)


@dataclass(frozen=True)
class Backend:
    name: str
    model: str
    key_env: str


@dataclass(frozen=True)
class LlmReply:
    text: str
    input_tokens: int
    prompt_tokens: int
    commands: int


class Http(Protocol):
    def post(self, host: str, path: str, headers: dict[str, str], body: bytes) -> tuple[int, bytes]: ...


def load_backend(name: str) -> Backend:
    raw = tomllib.loads(files("jev_check").joinpath("jev.toml").read_text())
    section = as_object(raw.get(name))
    if name not in ENDPOINTS or section is None:
        raise JevCheckError("INPUT_INVALID", f"unknown backend {name}; expected jev, haiku or luna")
    return Backend(name, str(section.get("model")), str(section.get("key_env")))


def answer_schema(questions: dict[str, Question]) -> dict[str, Any]:
    def answer(criteria: dict[str, str]) -> dict[str, Any]:
        return {"type": "object", "additionalProperties": False, "required": ["choice", "confidence"],
                "properties": {"choice": {"type": "string", "enum": sorted(criteria)}, "confidence": {"type": "number"}}}
    return {"type": "object", "additionalProperties": False, "required": ["answers"], "properties": {"answers": {
        "type": "object", "additionalProperties": False, "required": sorted(questions),
        "properties": {k: answer(q["criteria"]) for k, q in sorted(questions.items())}}}}


def message_body(backend: Backend, turns: list[Turn], schema: dict[str, Any] | None) -> dict[str, Any]:
    msgs = [{"role": role, "content": text} for role, text in turns]
    if backend.name == "haiku":
        body: dict[str, Any] = {"model": backend.model, "max_tokens": MAX_TOKENS, "messages": msgs}
        if schema is not None:
            body["output_config"] = {"format": {"type": "json_schema", "schema": schema}}
        return body
    body = {"model": backend.model, "input": msgs}
    if schema is not None:
        body["text"] = {"format": {"type": "json_schema", "name": "answers", "schema": schema, "strict": True}}
    return body


def count_body(backend: Backend, body: dict[str, Any]) -> dict[str, Any]:
    return {k: v for k, v in body.items() if k != "max_tokens"}


def headers(backend: Backend, key: str) -> dict[str, str]:
    auth = {"x-api-key": key, "anthropic-version": ANTHROPIC_VERSION} if backend.name == "haiku" else {
        "Authorization": "Bearer " + key}
    return {**auth, "Content-Type": "application/json"}


def as_int(value: object) -> int:
    return value if isinstance(value, int) and not isinstance(value, bool) else 0


def as_list(value: object) -> list[object]:
    return cast(list[object], value) if isinstance(value, list) else []


def parse_message(backend: Backend, raw: bytes) -> tuple[str, int, int]:
    """-> (text, input_tokens, commands) from a vendor reply."""
    try:
        value = as_object(json.loads(raw)) or {}
    except ValueError:
        raise JevCheckError("RESPONSE_INVALID", f"{backend.name} reply is not JSON") from None
    usage = as_object(value.get("usage")) or {}
    texts: list[str] = []
    commands = 0
    if backend.name == "haiku":
        tokens = sum(as_int(usage.get(k)) for k in ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"))
        for block in map(as_object, as_list(value.get("content"))):
            kind = block.get("type") if block is not None else None
            if block is not None and kind == "text" and isinstance(block.get("text"), str):
                texts.append(str(block["text"]))
            elif isinstance(kind, str) and kind.endswith("tool_use"):
                commands += 1
    else:
        tokens = as_int(usage.get("input_tokens"))
        for item in map(as_object, as_list(value.get("output"))):
            kind = item.get("type") if item is not None else None
            if item is not None and kind == "message":
                texts += [str(c["text"]) for c in map(as_object, as_list(item.get("content")))
                          if c is not None and isinstance(c.get("text"), str)]
            elif kind != "reasoning":
                commands += 1
    if not texts:
        raise JevCheckError("RESPONSE_INVALID", f"{backend.name} reply has no text")
    return "".join(texts), tokens, commands


def parse_count(backend: Backend, raw: bytes) -> int:
    try:
        value = as_object(json.loads(raw)) or {}
    except ValueError:
        raise JevCheckError("RESPONSE_INVALID", f"{backend.name} token count is not JSON") from None
    tokens = value.get("input_tokens")
    if not isinstance(tokens, int) or isinstance(tokens, bool):
        raise JevCheckError("RESPONSE_INVALID", f"{backend.name} token count has no input_tokens")
    return tokens


def keychain(service: str) -> str:
    """The macOS Keychain generic password: account $USER, service = the variable name; '' if absent."""
    service = os.environ.get("ANTHROPIC_KEYCHAIN_SERVICE", service) if service == "ANTHROPIC_API_KEY" else service
    try:
        done = subprocess.run(["security", "find-generic-password", "-a", os.environ.get("USER", ""), "-s", service, "-w"],
                              capture_output=True, text=True, timeout=10, check=False)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return done.stdout.strip() if done.returncode == 0 else ""


def api_key(backend: Backend, lookup: Callable[[str], str] = keychain) -> str:
    """Environment first, then the Keychain entry named after the variable. Never printed or logged."""
    key = os.environ.get(backend.key_env, "").strip() or lookup(backend.key_env)
    if not key:
        raise JevCheckError("KEY_MISSING", f"{backend.key_env} is not in the environment or the Keychain; "
                            f"no {backend.name} request was sent")
    return key


def complete(backend: Backend, turns: list[Turn], schema: dict[str, Any] | None, http: "Http | None" = None,
             lookup: Callable[[str], str] = keychain) -> LlmReply:
    """One message request plus one count request with the same body."""
    key = api_key(backend, lookup)
    host, path, count_path = ENDPOINTS[backend.name]
    client = http or HttpsPost()
    body = message_body(backend, turns, schema)
    hdrs = headers(backend, key)
    status, raw = client.post(host, count_path, hdrs, json.dumps(count_body(backend, body)).encode())
    if status != 200:
        raise JevCheckError("BACKEND_UNAVAILABLE", f"{backend.name} token count HTTP {status}: {raw[:300]!r}")
    prompt_tokens = parse_count(backend, raw)
    status, raw = client.post(host, path, hdrs, json.dumps(body).encode())
    if status != 200:
        raise JevCheckError("BACKEND_UNAVAILABLE", f"{backend.name} HTTP {status}: {raw[:300]!r}")
    text, input_tokens, commands = parse_message(backend, raw)
    return LlmReply(text, input_tokens, prompt_tokens, commands)


def parse_reply(text: str, questions: dict[str, Question]) -> dict[str, Answer]:
    try:
        value = as_object(json.loads(text))
    except ValueError:
        raise JevCheckError("RESPONSE_INVALID", "LLM reply is not JSON") from None
    answers = as_object(value.get("answers")) if value is not None else None
    if answers is None or set(answers) != set(questions):
        raise JevCheckError("RESPONSE_INVALID", "LLM answer ids differ from the question ids")
    out: dict[str, Answer] = {}
    for key, item in answers.items():
        a = as_object(item)
        choice = a.get("choice") if a is not None else None
        if a is None or not isinstance(choice, str) or choice not in questions[key]["criteria"]:
            raise JevCheckError("RESPONSE_INVALID", f"LLM answer {key} is not one of its criteria")
        conf = as_probability(a.get("confidence"))
        out[key] = {"choice": choice, "confidence": conf,
                    "probabilities": {c: (conf if c == choice else 0.0) for c in questions[key]["criteria"]}}
    return out


def check_llm(backend: Backend, state: Any, questions: dict[str, Question], http: Http | None = None,
              lookup: Callable[[str], str] = keychain) -> tuple[dict[str, Answer], LlmReply]:
    prompt = payload_bytes(state, questions).decode()
    reply = complete(backend, [("user", prompt)], answer_schema(questions), http, lookup)
    return parse_reply(reply.text, questions), reply


class HttpsPost:
    def post(self, host: str, path: str, headers: dict[str, str], body: bytes) -> tuple[int, bytes]:
        conn = http.client.HTTPSConnection(host, timeout=300)
        try:
            conn.request("POST", path, body=body, headers=headers)
            resp = conn.getresponse()
            return resp.status, resp.read()
        except OSError as exc:
            raise JevCheckError("BACKEND_UNAVAILABLE", f"{host} connection failed: {type(exc).__name__}") from None
        finally:
            conn.close()
