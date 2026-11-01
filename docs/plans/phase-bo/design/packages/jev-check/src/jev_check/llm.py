"""Haiku and Luna backends: the same {state, questions} request, the reply forced into {choice, confidence}."""
import http.client
import json
import os
import subprocess
import tomllib
from dataclasses import dataclass
from importlib.resources import files
from typing import Any, Protocol, cast

from jev_check.client import Answer, JevCheckError, Question, as_object, as_probability

SYSTEM = ("You answer choice questions about `state`. For each question key choose exactly one key of its "
          "`criteria`. Reply with JSON only: {\"answers\": {\"<question key>\": {\"choice\": \"<criterion key>\", "
          "\"confidence\": <0..1>}}}, one entry per question, no other text.")


@dataclass(frozen=True)
class Backend:
    name: str
    model: str
    transport: str
    key_env: str


class LlmTransport(Protocol):
    def complete(self, backend: Backend, system: str, prompt: str) -> str: ...


def load_backend(name: str) -> Backend:
    raw = tomllib.loads(files("jev_check").joinpath("jev.toml").read_text())
    section = as_object(raw.get(name))
    if name not in ("haiku", "luna") or section is None:
        raise JevCheckError("INPUT_INVALID", f"unknown backend {name}; expected jev, haiku or luna")
    transport = str(section.get("transport"))
    if transport not in ("api", "cli"):
        raise JevCheckError("INPUT_INVALID", f"backend {name}: transport must be api or cli, not {transport}")
    override = os.environ.get(f"JEV_CHECK_{name.upper()}_TRANSPORT")
    return Backend(name, str(section.get("model")), override if override in ("api", "cli") else transport,
                   str(section.get("key_env")))


def request_prompt(state: Any, questions: dict[str, Question]) -> str:
    return json.dumps({"state": state, "questions": questions}, sort_keys=True, ensure_ascii=False)


def parse_reply(text: str, questions: dict[str, Question]) -> dict[str, Answer]:
    body = text.strip()
    if body.startswith("```"):
        body = body.split("\n", 1)[1].rsplit("```", 1)[0]
    try:
        value = as_object(json.loads(body))
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


def check_llm(backend: Backend, state: Any, questions: dict[str, Question],
              transport: LlmTransport | None = None) -> dict[str, Answer]:
    reply = (transport or default_transport(backend)).complete(backend, SYSTEM, request_prompt(state, questions))
    return parse_reply(reply, questions)


def default_transport(backend: Backend) -> LlmTransport:
    return CliTransport() if backend.transport == "cli" else ApiTransport()


class ApiTransport:
    def complete(self, backend: Backend, system: str, prompt: str) -> str:
        key = os.environ.get(backend.key_env, "").strip()
        if not key:
            raise JevCheckError("KEY_MISSING", f"{backend.key_env} is missing; no {backend.name} request was sent")
        if backend.name == "haiku":
            host, path = "api.anthropic.com", "/v1/messages"
            headers = {"x-api-key": key, "anthropic-version": "2023-06-01"}
            body = {"model": backend.model, "max_tokens": 4096, "system": system,
                    "messages": [{"role": "user", "content": prompt}]}
        else:
            host, path = "api.openai.com", "/v1/responses"
            headers = {"Authorization": "Bearer " + key}
            body = {"model": backend.model, "instructions": system, "input": prompt}
        conn = http.client.HTTPSConnection(host, timeout=120)
        try:
            conn.request("POST", path, body=json.dumps(body), headers={**headers, "Content-Type": "application/json"})
            resp = conn.getresponse()
            raw, status = resp.read(), resp.status
        except OSError as exc:
            raise JevCheckError("BACKEND_UNAVAILABLE", f"{backend.name} connection failed: {type(exc).__name__}") from None
        finally:
            conn.close()
        if status != 200:
            raise JevCheckError("BACKEND_UNAVAILABLE", f"{backend.name} HTTP {status}")
        return api_text(backend, raw)


def api_text(backend: Backend, raw: bytes) -> str:
    value = as_object(json.loads(raw)) or {}
    key, kind = ("content", "text") if backend.name == "haiku" else ("output", "message")
    raw_blocks = value.get(key)
    blocks = cast(list[object], raw_blocks) if isinstance(raw_blocks, list) else []
    texts: list[str] = []
    for block in blocks:
        b = as_object(block) or {}
        if backend.name == "haiku" and b.get("type") == kind and isinstance(b.get("text"), str):
            texts.append(str(b["text"]))
        raw_content = b.get("content")
        content = cast(list[object], raw_content) if isinstance(raw_content, list) else []
        texts += [str(c["text"]) for c in map(as_object, content) if c is not None and isinstance(c.get("text"), str)]
    if not texts:
        raise JevCheckError("RESPONSE_INVALID", f"{backend.name} reply has no text")
    return "".join(texts)


class CliTransport:
    def complete(self, backend: Backend, system: str, prompt: str) -> str:
        if backend.name == "haiku":
            cmd = ["claude", "-p", "--model", backend.model, "--output-format", "json", "--tools", "",
                   "--no-session-persistence", "--setting-sources", "", "--strict-mcp-config", "--system-prompt", system]
            text = prompt
        else:
            cmd = ["codex", "exec", "-m", backend.model, "--skip-git-repo-check", "--ephemeral", "-s", "read-only",
                   "--json", "-"]
            text = system + "\n\n" + prompt
        try:
            done = subprocess.run(cmd, input=text, capture_output=True, text=True, timeout=600, check=False)
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise JevCheckError("BACKEND_UNAVAILABLE", f"{backend.name} CLI failed: {type(exc).__name__}") from None
        if done.returncode != 0:
            raise JevCheckError("BACKEND_UNAVAILABLE", f"{backend.name} CLI exit {done.returncode}: {done.stderr[-300:]}")
        return cli_text(backend, done.stdout)


def cli_text(backend: Backend, stdout: str) -> str:
    if backend.name == "haiku":
        value = as_object(json.loads(stdout)) or {}
        result = value.get("result")
        if not isinstance(result, str):
            raise JevCheckError("RESPONSE_INVALID", "claude -p reply has no result")
        return result
    last = ""
    for line in stdout.splitlines():
        event = as_object(json.loads(line)) if line.startswith("{") else None
        item = as_object(event.get("item")) if event is not None else None
        if item is not None and item.get("type") == "agent_message" and isinstance(item.get("text"), str):
            last = str(item["text"])
    if not last:
        raise JevCheckError("RESPONSE_INVALID", "codex exec reply has no agent message")
    return last
