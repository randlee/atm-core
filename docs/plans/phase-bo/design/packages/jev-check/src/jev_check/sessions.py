"""Sessions: each run stores its request and answers; `ask` follows up with an LLM seeded from them."""
import json
import os
import time
import uuid
from collections.abc import Callable
from pathlib import Path
from typing import Any

from jev_check.client import JevCheckError, as_object
from jev_check.llm import Backend, LlmTransport, default_transport

DAY = 86400.0
ASK_SYSTEM = ("You are following up on a set of choice answers about `state`. The request and the answers given "
              "are below. Answer the follow-up question in plain text, citing the parts of `state` you rely on.")


class SessionStore:
    def __init__(self, root: Path, keep_days: float = 7, clock: Callable[[], float] = time.time) -> None:
        self.root, self.keep, self.clock = root, keep_days * DAY, clock

    @classmethod
    def default(cls) -> "SessionStore":
        return cls(Path(os.path.expanduser("~/.cache/jev-check/sessions")))

    def write(self, record: dict[str, Any]) -> str:
        self.root.mkdir(parents=True, exist_ok=True)
        for old in self.root.glob("*.json"):
            if self.clock() - old.stat().st_mtime > self.keep:
                old.unlink()
        sid = str(uuid.uuid4())
        (self.root / f"{sid}.json").write_text(json.dumps({"id": sid, "created_at": self.clock(), **record}, indent=1))
        return sid

    def read(self, sid: str) -> dict[str, object]:
        path = self.root / f"{sid}.json"
        if not path.is_file():
            raise JevCheckError("SESSION_NOT_FOUND", f"no session {sid} under {self.root}")
        record = as_object(json.loads(path.read_text())) or {}
        created = record.get("created_at")
        if not isinstance(created, (int, float)) or self.clock() - created > self.keep:
            raise JevCheckError("SESSION_EXPIRED", f"session {sid} is older than {self.keep / DAY:g} days")
        return record

    def ask(self, sid: str, backend: Backend, question: str, transport: LlmTransport | None = None) -> str:
        record = self.read(sid)
        prompt = json.dumps({"request": record.get("request"), "answers": record.get("answers"),
                             "backend": record.get("backend"), "follow_up": question}, ensure_ascii=False)
        answer = (transport or default_transport(backend)).complete(backend, ASK_SYSTEM, prompt)
        followups = record.get("followups")
        record["followups"] = [*(followups if isinstance(followups, list) else []),
                               {"backend": backend.name, "model": backend.model, "question": question, "answer": answer}]
        (self.root / f"{sid}.json").write_text(json.dumps(record, indent=1))
        return answer
