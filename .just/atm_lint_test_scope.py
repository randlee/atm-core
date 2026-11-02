"""ATM's cfg(test-*) extension to sc-lint's Rust test-scope helper."""
from __future__ import annotations

from pathlib import Path

from sc_lint.lint_common import rust_file_test_scope as published_rust_file_test_scope


def _strip_negated_cfg_segments(body: str) -> str:
    result: list[str] = []
    index = 0
    while index < len(body):
        if body.startswith("not", index):
            probe = index + 3
            while probe < len(body) and body[probe].isspace():
                probe += 1
            if probe < len(body) and body[probe] == "(":
                depth = 1
                probe += 1
                while probe < len(body) and depth:
                    if body[probe] == "(":
                        depth += 1
                    elif body[probe] == ")":
                        depth -= 1
                    probe += 1
                result.append(" ")
                index = probe
                continue
        result.append(body[index])
        index += 1
    return "".join(result)


def _is_identifier_character(value: str) -> bool:
    return value == "_" or (value.isascii() and value.isalnum())


def is_rust_test_cfg_attribute(line: str) -> bool:
    stripped = line.strip()
    prefix = "#![cfg(" if stripped.startswith("#![cfg(") else "#[cfg("
    if not stripped.startswith(prefix) or not stripped.endswith(")]"):
        return False
    body = _strip_negated_cfg_segments(stripped[len(prefix):-2])
    start = 0
    while (index := body.find("test", start)) >= 0:
        before = body[index - 1] if index else ""
        after_index = index + len("test")
        after = body[after_index] if after_index < len(body) else ""
        if not (_is_identifier_character(before) or _is_identifier_character(after)):
            return True
        start = after_index
    return False


def _classify_atm_test_scope(lines: list[str]) -> list[bool]:
    if any(line.strip().startswith("#![") and is_rust_test_cfg_attribute(line) for line in lines):
        return [True] * len(lines)
    scope: list[bool] = []
    cfg_pending = False
    cfg_active = False
    block_depth: int | None = None
    brace_balance = 0
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("#[") and is_rust_test_cfg_attribute(stripped):
            cfg_pending = cfg_active = True
            scope.append(True)
            continue
        if cfg_pending and stripped.startswith("#[") and stripped.endswith("]"):
            scope.append(True)
            continue
        scope.append(block_depth is not None or cfg_pending)
        opens, closes = line.count("{"), line.count("}")
        if cfg_pending and opens:
            brace_balance = opens - closes
            block_depth = max(brace_balance, 0)
            cfg_pending = cfg_active = False
        elif cfg_pending and stripped.endswith(";"):
            cfg_pending = cfg_active = False
        elif block_depth is not None:
            brace_balance += opens - closes
            block_depth = max(brace_balance, 0) or None
        elif cfg_active and stripped and not stripped.startswith("#["):
            cfg_pending = cfg_active = False
    return scope


def rust_file_test_scope(path: Path, lines: list[str]) -> list[bool]:
    if any("feature" in line and is_rust_test_cfg_attribute(line) for line in lines):
        return _classify_atm_test_scope(lines)
    return published_rust_file_test_scope(path, lines)
