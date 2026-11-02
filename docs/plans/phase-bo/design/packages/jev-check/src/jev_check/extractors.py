"""Extractors: each reads `git show <rev>:<path>`. An empty extraction is a typed error."""
import ast
import subprocess
from collections.abc import Callable

from jev_check.client import JevCheckError

GitShow = Callable[[str, str], str]


def git_show(rev: str, path: str) -> str:
    done = subprocess.run(["git", "show", f"{rev}:{path}"], capture_output=True, text=True, check=False)
    if done.returncode != 0:
        raise JevCheckError("INPUT_MISSING", f"git show {rev}:{path} failed: {done.stderr.strip()[:200]}")
    return done.stdout


def extract_file(text: str, path: str) -> str:
    if not text.strip():
        raise JevCheckError("EXTRACTION_EMPTY", f"{path} is empty")
    return text


def extract_table_column(text: str, path: str, table: str, column: str) -> dict[str, str]:
    """Row id (the `column` cell) -> the whole row, from the markdown table under `## <table>`."""
    marker = f"## {table}"
    section = text.split(marker, 1)[1].split("\n## ", 1)[0] if marker in text else ""
    lines = [ln.strip() for ln in section.splitlines() if ln.strip().startswith("|")]
    cells = [[c.strip() for c in ln.strip("|").split("|")] for ln in lines]
    if len(cells) < 3 or column not in cells[0]:
        raise JevCheckError("EXTRACTION_EMPTY", f"{path}: no rows in table {table!r} with column {column!r}")
    i = cells[0].index(column)
    return {row[i]: lines[n + 2] for n, row in enumerate(cells[2:]) if len(row) > i and row[i]}


def extract_python_branches(text: str, path: str) -> dict[str, str]:
    """`case` arms and `return ...Refusal(...)` sites, keyed `<path>:<line>`."""
    tree = ast.parse(text)
    found: dict[str, str] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.match_case):
            found[f"{path}:{node.pattern.lineno}"] = f"case {ast.unparse(node.pattern)}"
        elif (isinstance(node, ast.Return) and node.value is not None
              and any(isinstance(n, ast.Call) and getattr(n.func, "id", None) == "Refusal" for n in ast.walk(node.value))):
            found[f"{path}:{node.lineno}"] = ast.unparse(node)
    if not found:
        raise JevCheckError("EXTRACTION_EMPTY", f"{path}: no case arms or Refusal returns")
    return dict(sorted(found.items(), key=lambda kv: int(kv[0].rsplit(":", 1)[1])))
