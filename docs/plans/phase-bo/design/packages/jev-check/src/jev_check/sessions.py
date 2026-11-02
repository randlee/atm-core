"""Sessions: each run stores its request, reply and answers; `ask` continues that conversation on its backend."""
import json
import os
import time
import tomllib
from importlib.resources import files
import uuid
from collections.abc import Callable
from pathlib import Path
from typing import Any, cast

from jev_check.client import JevCheckError, Question, as_object, payload_bytes
from jev_check.llm import Http, Turn, as_list, complete, load_backend

DAY = 86400.0


class SessionStore:
    def __init__(self, root: Path, keep_days: float = 7, clock: Callable[[], float] = time.time) -> None:
        self.root, self.keep, self.clock = root, keep_days * DAY, clock

    @classmethod
    def default(cls) -> "SessionStore":
        """The store named by jev.toml [sessions]: dir and keep_days."""
        raw = tomllib.loads(files("jev_check").joinpath("jev.toml").read_text())["sessions"]
        return cls(Path(os.path.expanduser(str(raw["dir"]))), float(raw["keep_days"]))

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

    def ask(self, sid: str, question: str, http: Http | None = None) -> str:
        """Re-prompt the session: the same user payload, the backend's own reply, earlier follow-ups, then this one."""
        record = self.read(sid)
        backend, reply = record.get("backend"), record.get("reply_text")
        request = as_object(record.get("request")) or {}
        if not isinstance(backend, str) or backend == "jev" or not isinstance(reply, str):
            raise JevCheckError("SESSION_NOT_ASKABLE", f"session {sid} is a {backend} run; only haiku/luna runs continue")
        questions = cast(dict[str, Question], request.get("questions"))
        turns: list[Turn] = [("user", payload_bytes(request.get("state"), questions).decode()), ("assistant", reply)]
        followups = [f for f in map(as_object, as_list(record.get("followups"))) if f is not None]
        for f in followups:
            turns += [("user", str(f.get("question"))), ("assistant", str(f.get("answer")))]
        answer = complete(load_backend(backend), [*turns, ("user", question)], None, http).text
        record["followups"] = [*followups, {"question": question, "answer": answer}]
        (self.root / f"{sid}.json").write_text(json.dumps(record, indent=1))
        return answer
