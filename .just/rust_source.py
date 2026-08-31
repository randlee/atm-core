"""Small lexical Rust source helpers used by lint gates.

This is deliberately not a parser. It skips comments and all quoted literals
while locating a named function's balanced body; Rust architecture tests use
syn for AST-sensitive checks.
"""
from __future__ import annotations

import re


def code_mask(source: str) -> str:
    result = list(source)
    index = 0
    while index < len(source):
        if source.startswith("//", index):
            end = source.find("\n", index)
            end = len(source) if end < 0 else end
        elif source.startswith("/*", index):
            end = source.find("*/", index + 2)
            end = len(source) if end < 0 else end + 2
        elif raw_match := re.match(r"(?:br|rb|r)(?P<hashes>#+)?\"", source[index:]):
            hashes = raw_match.group("hashes") or ""
            delimiter = f'"{hashes}'
            search_from = index + len(raw_match.group(0))
            while True:
                closing_start = source.find(delimiter, search_from)
                if closing_start < 0:
                    end = len(source)
                    break
                end = closing_start + len(delimiter)
                if end == len(source) or source[end] != "#":
                    break
                search_from = end
        elif source.startswith('b"', index):
            quote = '"'
            end = index + 2
            while end < len(source):
                if source[end] == "\\":
                    end += 2
                elif source[end] == quote:
                    end += 1
                    break
                else:
                    end += 1
        elif source[index] in {'"', "'"}:
            quote = source[index]
            end = index + 1
            while end < len(source):
                if source[end] == "\\":
                    end += 2
                elif source[end] == quote:
                    end += 1
                    break
                else:
                    end += 1
        else:
            index += 1
            continue
        for masked in range(index, min(end, len(source))):
            if result[masked] != "\n":
                result[masked] = " "
        index = end
    return "".join(result)


def extract_fn_body(source: str, fn_name: str) -> str:
    masked = code_mask(source)
    match = re.search(rf"\bfn\s+{re.escape(fn_name)}(?:\s*<[^{{(]*>)?\s*\(", masked)
    if match is None:
        raise ValueError(f"read handler `{fn_name}` is missing")
    body_start = masked.find("{", match.end())
    if body_start < 0:
        raise ValueError(f"read handler `{fn_name}` has no body")
    depth = 0
    for index in range(body_start, len(masked)):
        if masked[index] == "{":
            depth += 1
        elif masked[index] == "}":
            depth -= 1
            if depth == 0:
                return source[body_start : index + 1]
    raise ValueError(f"read handler `{fn_name}` body is not closed")
