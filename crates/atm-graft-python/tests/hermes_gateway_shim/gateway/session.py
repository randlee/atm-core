"""Session value shim for adapter import/lifecycle tests."""

from dataclasses import dataclass
from typing import Any


@dataclass
class SessionSource:
    platform: Any
    chat_id: str
    chat_type: str
    user_id: str
    user_name: str | None = None
