"""The plug-in boundary: every .py under lite/skills imports only `atm_bd_types` (and stdlib `sys`).

A plug-in decides; the framework (`atm_bd_core`) and every I/O module stay on the other side.
"""
import ast
from pathlib import Path

import pytest

SKILLS = Path(__file__).parent.parent.parent / "skills"
ALLOWED = {"atm_bd_types", "sys"}


def foreign_imports(source: str) -> list[str]:
    """Top-level module names `source` imports outside ALLOWED; a relative import counts as foreign."""
    found: list[str] = []
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.Import):
            found += [a.name.split(".")[0] for a in node.names]
        elif isinstance(node, ast.ImportFrom):
            found.append("." * node.level + (node.module or "").split(".")[0])
    return [m for m in found if m not in ALLOWED]


def plugin_files() -> list[Path]:
    return sorted(SKILLS.rglob("*.py"))


def test_plugin_files_exist() -> None:
    assert len(plugin_files()) >= 3, SKILLS


@pytest.mark.parametrize("path", plugin_files(), ids=lambda p: p.name)
def test_plugin_imports_only_atm_bd_types(path: Path) -> None:
    assert foreign_imports(path.read_text()) == [], path


@pytest.mark.parametrize("source", [
    "import atm_bd_core",
    "from atm_bd_core import Ports",
    "import subprocess",
    "import os.path",
    "from pathlib import Path",
    "import json",
    "from http import client",
    "import socket",
    "from . import helper",
    "def f():\n    import subprocess",
])
def test_checker_rejects_framework_and_io_imports(source: str) -> None:
    assert foreign_imports(source) != []


def test_checker_accepts_the_plugin_surface() -> None:
    assert foreign_imports("import sys\nfrom atm_bd_types import Refusal\nfrom atm_bd_types import CloseTransition as T") == []
