# pyright: strict
"""Identify installed sprint QA assignment contracts."""
from pathlib import Path
import re
import yaml
from typing import TypeGuard

# These reviewers use their agent contracts instead of orchestration assignment templates.
RUST_REVIEWERS = frozenset({
    "rust-qa-agent", "rust-code-reviewer", "rust-best-practices-agent", "rust-service-hardening-agent",
})


def is_object_mapping(value: object) -> TypeGuard[dict[object, object]]:
    return isinstance(value, dict)


def qa_capable_reviewer(name: str, templates_dir: Path) -> bool:
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", name):
        return False
    if name in RUST_REVIEWERS:
        return True
    path = templates_dir / f"{name}-assignment.json.j2"
    try:
        text = path.read_text(encoding="utf-8")
        front = text.split("---", 2)
        if len(front) != 3 or front[0].strip():
            return False
        data = yaml.safe_load(front[1])
    except (OSError, UnicodeError, yaml.YAMLError):
        return False
    if not is_object_mapping(data):
        return False
    required = data.get("required_variables")
    defaults = data.get("defaults", {})
    if not isinstance(required, list) or not is_object_mapping(defaults):
        return False
    if not all(key in required and key not in defaults for key in ("branch", "commit", "worktree_path")):
        return False
    return (
        "review_targets" in required and "review_targets" not in defaults
    ) or isinstance(defaults.get("review_targets"), list)


def qa_capable_reviewers(templates_dir: Path) -> list[str]:
    suffix = "-assignment.json.j2"
    local = (name for path in templates_dir.glob(f"*{suffix}")
             if qa_capable_reviewer(name := path.name.removesuffix(suffix), templates_dir))
    return sorted(RUST_REVIEWERS | set(local))
