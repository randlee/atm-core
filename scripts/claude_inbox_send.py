#!/usr/bin/env python3
"""Write a Claude-compatible team message into a JSON-array inbox atomically."""

from __future__ import annotations

import argparse
import json
import os
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Append a Claude-compatible message to a JSON-array inbox."
    )
    parser.add_argument("--team", required=True)
    parser.add_argument("--to", required=True, dest="recipient")
    parser.add_argument("--from", required=True, dest="sender")
    parser.add_argument("--text", required=True)
    parser.add_argument("--summary", required=True)
    parser.add_argument("--read", action="store_true", default=False)
    return parser.parse_args()


def load_mailbox(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []

    raw = path.read_text(encoding="utf-8").strip()
    if not raw:
        return []

    data = json.loads(raw)
    if not isinstance(data, list):
        raise ValueError(f"{path} does not contain a JSON array mailbox")
    if not all(isinstance(item, dict) for item in data):
        raise ValueError(f"{path} contains non-object mailbox entries")
    return data


def atomic_write_json(path: Path, data: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w", encoding="utf-8", dir=path.parent, delete=False
    ) as handle:
        tmp_path = Path(handle.name)
        json.dump(data, handle, indent=2, ensure_ascii=False)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(tmp_path, path)


def main() -> int:
    args = parse_args()
    inbox = (
        Path.home()
        / ".claude"
        / "teams"
        / args.team
        / "inboxes"
        / f"{args.recipient}.json"
    )
    mailbox = load_mailbox(inbox)
    mailbox.append(
        {
            "from": args.sender,
            "text": args.text,
            "timestamp": datetime.now(timezone.utc)
            .isoformat(timespec="milliseconds")
            .replace("+00:00", "Z"),
            "read": args.read,
            "summary": args.summary,
        }
    )
    atomic_write_json(inbox, mailbox)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
