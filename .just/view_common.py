#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import json
import shutil
from typing import Any


VIEW_ROOT = Path("artifacts/view")


def view_root(repo_root: Path) -> Path:
    return repo_root / VIEW_ROOT


def view_dir(repo_root: Path, tool_name: str) -> Path:
    return view_root(repo_root) / tool_name


def reset_view_dir(repo_root: Path, tool_name: str) -> Path:
    target = view_dir(repo_root, tool_name)
    shutil.rmtree(target, ignore_errors=True)
    target.mkdir(parents=True, exist_ok=True)
    return target


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def relative_artifact_path(repo_root: Path, path: Path) -> str:
    return path.relative_to(repo_root).as_posix()

