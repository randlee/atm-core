#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path


FENCE_START = "```"
LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
SCHEME_RE = re.compile(r"^[A-Za-z][A-Za-z0-9+.-]*:")


@dataclass(frozen=True)
class FencedBlock:
    doc_path: Path
    language: str
    ordinal: int
    content: str


def extract_fenced_blocks(markdown_text: str) -> list[FencedBlock]:
    blocks: list[FencedBlock] = []
    current_language: str | None = None
    current_lines: list[str] = []
    ordinal = 0
    current_doc = Path("<unknown>")

    for line in markdown_text.splitlines():
        if line.startswith("<!-- doc-path: "):
            current_doc = Path(line.removeprefix("<!-- doc-path: ").removesuffix(" -->").strip())
            continue
        if line.startswith(FENCE_START):
            if current_language is None:
                current_language = line.removeprefix(FENCE_START).strip().lower()
                current_lines = []
                ordinal += 1
                continue
            blocks.append(
                FencedBlock(
                    doc_path=current_doc,
                    language=current_language,
                    ordinal=ordinal,
                    content="\n".join(current_lines).strip() + "\n",
                )
            )
            current_language = None
            current_lines = []
            continue
        if current_language is not None:
            current_lines.append(line)
    return blocks


def validate_json_block(block: FencedBlock) -> list[str]:
    try:
        json.loads(block.content)
    except json.JSONDecodeError as exc:
        return [f"{block.doc_path}: json fenced block #{block.ordinal} failed to parse: {exc}"]
    return []


def validate_xml_block(block: FencedBlock) -> list[str]:
    try:
        ET.fromstring(block.content)
    except ET.ParseError as exc:
        return [f"{block.doc_path}: xml fenced block #{block.ordinal} failed to parse: {exc}"]
    return []


def validate_toml_block(block: FencedBlock) -> list[str]:
    try:
        tomllib.loads(block.content)
    except tomllib.TOMLDecodeError as exc:
        return [f"{block.doc_path}: toml fenced block #{block.ordinal} failed to parse: {exc}"]
    return []


def validate_bash_block(block: FencedBlock) -> list[str]:
    completed = subprocess.run(
        ["bash", "-n"],
        input=block.content,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip()
        return [f"{block.doc_path}: bash fenced block #{block.ordinal} failed `bash -n`: {detail}"]
    return []


def doc_markdown_text(doc_path: Path, root: Path) -> str:
    return f"<!-- doc-path: {doc_path.relative_to(root).as_posix()} -->\n" + doc_path.read_text(encoding="utf-8")


def validate_relative_links(doc_root: Path) -> list[str]:
    errors: list[str] = []
    root = doc_root.resolve()
    for doc_path in sorted(doc_root.rglob("*.md")):
        text = doc_path.read_text(encoding="utf-8")
        for target in LINK_RE.findall(text):
            path_part, _, _anchor = target.partition("#")
            if not path_part:
                continue
            if path_part.startswith("/") or SCHEME_RE.match(path_part):
                errors.append(
                    f"{doc_path.relative_to(doc_root)}: link target `{target}` must stay relative inside the user-doc tree"
                )
                continue
            resolved = (doc_path.parent / path_part).resolve()
            if not resolved.exists():
                errors.append(
                    f"{doc_path.relative_to(doc_root)}: broken relative link target `{target}`"
                )
                continue
            try:
                resolved.relative_to(root)
            except ValueError:
                errors.append(
                    f"{doc_path.relative_to(doc_root)}: link target `{target}` escapes the user-doc tree"
                )
    return errors


def verify_installed_copy(source_root: Path, installed_root: Path) -> list[str]:
    source_files = sorted(path.relative_to(source_root).as_posix() for path in source_root.rglob("*") if path.is_file())
    installed_files = sorted(
        path.relative_to(installed_root).as_posix() for path in installed_root.rglob("*") if path.is_file()
    )
    errors: list[str] = []
    missing = sorted(set(source_files) - set(installed_files))
    extra = sorted(set(installed_files) - set(source_files))
    for relpath in missing:
        errors.append(f"installed copy missing file `{relpath}`")
    for relpath in extra:
        errors.append(f"installed copy has unexpected file `{relpath}`")
    return errors


def validate_fenced_blocks(doc_root: Path) -> list[str]:
    errors: list[str] = []
    validators = {
        "json": validate_json_block,
        "xml": validate_xml_block,
        "toml": validate_toml_block,
        "bash": validate_bash_block,
    }
    for doc_path in sorted(doc_root.rglob("*.md")):
        for block in extract_fenced_blocks(doc_markdown_text(doc_path, doc_root)):
            validator = validators.get(block.language)
            if validator is None:
                continue
            errors.extend(validator(block))
    return errors


def verify_tree(doc_root: Path) -> list[str]:
    return [*validate_relative_links(doc_root), *validate_fenced_blocks(doc_root)]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Verify ATM installed/source user docs")
    parser.add_argument("--source-root", required=True)
    parser.add_argument("--installed-root")
    args = parser.parse_args(argv)

    source_root = Path(args.source_root)
    installed_root = Path(args.installed_root) if args.installed_root else None

    errors = verify_tree(source_root)
    if installed_root is not None:
        errors.extend(verify_installed_copy(source_root, installed_root))
        errors.extend(verify_tree(installed_root))

    if errors:
        for error in errors:
            print(error)
        return 1

    print(f"ok: verified user docs at {source_root}")
    if installed_root is not None:
        print(f"ok: verified installed copy at {installed_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
