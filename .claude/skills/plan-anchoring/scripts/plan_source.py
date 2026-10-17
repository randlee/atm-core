#!/usr/bin/env python3
"""Normalize hardened Markdown and tracker-copy phase plans for review.

Expected failures are returned as ``Err(ToolError)`` (see ``result.py``), never
raised; ``main`` turns them into the ``{success, data, error}`` envelope.
Standard library only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from result import Err, Ok, Result, ToolError, err  # noqa: E402


MAX_PLAN_BYTES = 24_000
PHASE_TITLE_RE = re.compile(r"^# Phase ([A-Za-z0-9][A-Za-z0-9-]*)\b", re.MULTILINE)
SPRINT_TITLE_RE = re.compile(r"^# Sprint ([A-Za-z0-9][A-Za-z0-9-]*)\s+[—-]\s+(.+?)\s*$", re.MULTILINE)
HEADING_RE = re.compile(r"^## ([^\n]+?)\s*$", re.MULTILINE)
TABLE_HEADER = ["Sprint", "Authoritative plan", "Tracker", "Relation", "Closure"]
LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
COPY_REF_RE = re.compile(
    r"^plan-copy:(?P<path>[^@]+?)(?:@sha256:(?P<sha256>[0-9a-f]{64}))?$"
)
REQUIRED_HEADINGS = {
    "Scope boundary",
    "Authoritative deliverables",
    "Authoritative acceptance criteria",
    "Authoritative validation",
    "Non-closure",
}


def canonical_json(value: Any) -> bytes:
    """Serialize exactly as jev_judge.encode_body does at the HTTP boundary."""

    return json.dumps(value, allow_nan=False).encode("utf-8")


def normalized_text(text: str) -> str:
    return text.replace("\r\n", "\n").replace("\r", "\n").rstrip("\n") + "\n"


def digest_text(text: str) -> str:
    return hashlib.sha256(normalized_text(text).encode("utf-8")).hexdigest()


def repository_file(root: Path, value: str | Path, *, code: str = "SOURCE.PATH") -> Result[Path, ToolError]:
    supplied = Path(value)
    if supplied.is_absolute():
        return err(code, "source path must be repository-relative")
    lexical = root / supplied
    try:
        resolved = lexical.resolve(strict=True)
        resolved.relative_to(root)
    except (OSError, ValueError):
        return err(code, "source path is missing or escapes the repository")
    if lexical.is_symlink() or not stat.S_ISREG(resolved.stat().st_mode):
        return err(code, "source path must resolve to a non-symlink regular file")
    return Ok(resolved)


def _git_revision(root: Path) -> str | None:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    value = result.stdout.strip()
    return value if result.returncode == 0 and re.fullmatch(r"[0-9a-f]{40,64}", value) else None


def _row(line: str) -> list[str]:
    if not line.strip().startswith("|") or not line.strip().endswith("|"):
        return []
    return [cell.strip() for cell in line.strip()[1:-1].split("|")]


def phase_rows(text: str) -> Result[list[dict[str, str]], ToolError]:
    lines = text.splitlines()
    matches = [index for index, line in enumerate(lines) if _row(line) == TABLE_HEADER]
    if len(matches) != 1:
        return err("SOURCE.PHASE_TABLE", "phase index must contain exactly one sprint table")
    start = matches[0]
    separator = _row(lines[start + 1]) if start + 1 < len(lines) else []
    if len(separator) != len(TABLE_HEADER) or not all(re.fullmatch(r":?-{3,}:?", cell) for cell in separator):
        return err("SOURCE.PHASE_TABLE", "sprint table separator is missing or malformed")
    rows: list[dict[str, str]] = []
    for line in lines[start + 2:]:
        cells = _row(line)
        if not cells:
            break
        if len(cells) != len(TABLE_HEADER):
            return err("SOURCE.PHASE_TABLE", "sprint table row has the wrong number of columns")
        link = LINK_RE.fullmatch(cells[1])
        if not link:
            return err("SOURCE.PHASE_TABLE", "authoritative plan cell must be one Markdown link")
        rows.append({
            "id": cells[0],
            "label": link.group(1),
            "path": link.group(2),
            "tracker_id": cells[2].strip("`"),
            "relation": cells[3].strip("`"),
        })
    if not rows:
        return err("SOURCE.PHASE_TABLE", "sprint table must contain at least one sprint")
    if len({row["id"] for row in rows}) != len(rows):
        return err("SOURCE.DUPLICATE_ID", "phase sprint identifiers must be unique")
    if len({row["path"] for row in rows}) != len(rows):
        return err("SOURCE.DUPLICATE_PATH", "phase sprint paths must be unique")
    return Ok(rows)


def validate_sprint(text: str, expected_id: str | None = None) -> Result[tuple[str, str], ToolError]:
    normalized = normalized_text(text)
    title_matches = SPRINT_TITLE_RE.findall(normalized)
    if len(title_matches) != 1:
        return err("SOURCE.SPRINT_TITLE", "sprint document must contain exactly one Sprint title")
    sprint_id, title = title_matches[0]
    if expected_id is not None and sprint_id != expected_id:
        return err("SOURCE.SPRINT_ID", "sprint title identifier differs from phase table")
    headings = HEADING_RE.findall(normalized)
    for required in REQUIRED_HEADINGS:
        count = headings.count(required)
        if count != 1:
            return err("SOURCE.SECTION", f"sprint must contain exactly one {required!r} section")
    return Ok((sprint_id, title.strip()))


def shape(text: str, *, max_bytes: int = MAX_PLAN_BYTES) -> Result[dict[str, Any], ToolError]:
    normalized = normalized_text(text)
    headings = list(HEADING_RE.finditer(normalized))
    sections: dict[str, int] = {}
    for index, match in enumerate(headings):
        end = headings[index + 1].start() if index + 1 < len(headings) else len(normalized)
        sections[match.group(1)] = len(normalized[match.end():end].encode("utf-8"))
    size = len(normalized.encode("utf-8"))
    evidence = {
        "bytes": size,
        "chars": len(normalized),
        "lines": len(normalized.splitlines()),
        "section_bytes": sections,
        "max_bytes": max_bytes,
        "over_limit": size > max_bytes,
        "truncated": False,
    }
    if size > max_bytes:
        return err("PLAN.TOO_LARGE", "sprint exceeds the reliable review limit", data=evidence)
    return Ok(evidence)


def resolve_phase_index(root: Path, target: str | Path) -> Result[Path, ToolError]:
    supplied = Path(target)
    if supplied.is_absolute():
        return err("SOURCE.PATH", "phase target must be repository-relative")
    lexical = root / supplied
    try:
        resolved = lexical.resolve(strict=True)
        resolved.relative_to(root)
    except (OSError, ValueError):
        return err("SOURCE.PATH", "phase target is missing or escapes the repository")
    if resolved.is_dir():
        candidates = [path for path in resolved.glob("plan-phase-*.md") if path.is_file() and not path.is_symlink()]
        if len(candidates) != 1:
            return err("SOURCE.PHASE_INDEX", "phase directory must contain exactly one plan-phase-*.md index")
        return Ok(candidates[0].resolve())
    if lexical.is_symlink() or not stat.S_ISREG(resolved.stat().st_mode):
        return err("SOURCE.PHASE_INDEX", "phase index must be a non-symlink regular file")
    return Ok(resolved)


def discover_markdown(repository_root: str | Path, target: str | Path) -> Result[dict[str, Any], ToolError]:
    root = Path(repository_root).resolve(strict=True)
    resolved_index = resolve_phase_index(root, target)
    if isinstance(resolved_index, Err):
        return resolved_index
    index = resolved_index.value
    text = index.read_text(encoding="utf-8")
    phases = PHASE_TITLE_RE.findall(text)
    if len(phases) != 1:
        return err("SOURCE.PHASE_ID", "phase index must contain exactly one Phase title")
    phase_id = "phase-" + phases[0].lower().removeprefix("phase-")
    revision = _git_revision(root)
    descriptors = []
    seen_resolved: set[Path] = set()
    rows = phase_rows(text)
    if isinstance(rows, Err):
        return rows
    for row in rows.value:
        relative = (index.parent.relative_to(root) / row["path"]).as_posix()
        located = repository_file(root, relative)
        if isinstance(located, Err):
            return located
        path = located.value
        if path in seen_resolved:
            return err("SOURCE.DUPLICATE_PATH", "multiple sprint links resolve to the same file")
        seen_resolved.add(path)
        item = normalized_text(path.read_text(encoding="utf-8"))
        validated = validate_sprint(item, row["id"])
        if isinstance(validated, Err):
            return validated
        sprint_id, title = validated.value
        item_shape = shape(item)
        if isinstance(item_shape, Err):
            return item_shape
        repo_path = path.relative_to(root).as_posix()
        descriptors.append({
            "id": sprint_id,
            "title": title,
            "text": item,
            "path": repo_path,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "normalized_sha256": digest_text(item),
            "source_kind": "markdown",
            "source_ref": repo_path,
            "source_revision": revision,
            "tracker_id": row["tracker_id"],
            "dependency_relation": row["relation"],
            "shape": item_shape.value,
        })
    return Ok({
        "schema": "plan-anchor/plan-source/1",
        "phase": phase_id,
        "source_kind": "markdown",
        "source": index.relative_to(root).as_posix(),
        "source_revision": revision,
        "sprints": descriptors,
    })


def _tracker_record(value: Any) -> Result[dict[str, Any], ToolError]:
    if not isinstance(value, dict):
        return err("SOURCE.TRACKER", "tracker children must be objects")
    description = value.get("description")
    if not isinstance(description, str):
        return err("SOURCE.TRACKER", "tracker child description must be a sprint document")
    validated = validate_sprint(description)
    if isinstance(validated, Err):
        return validated
    sprint_id, title = validated.value
    item = normalized_text(description)
    external_ref = value.get("external_ref")
    ref_match = COPY_REF_RE.fullmatch(external_ref) if isinstance(external_ref, str) else None
    item_shape = shape(item)
    if isinstance(item_shape, Err):
        return item_shape
    return Ok({
        "id": sprint_id,
        "title": title,
        "text": item,
        "path": value.get("source_path") or (ref_match.group("path") if ref_match else None),
        "sha256": digest_text(item),
        "normalized_sha256": digest_text(item),
        "source_kind": value.get("tracker") or "tracker",
        "source_ref": str(value.get("id") or ""),
        "source_revision": value.get("revision") or value.get("updated_at"),
        "tracker_id": str(value.get("id") or ""),
        "dependency_relation": value.get("dependency_relation"),
        "_dependencies": value.get("dependencies") or [],
        "recorded_source_sha256": value.get("source_sha256") or (
            ref_match.group("sha256") if ref_match else None
        ),
        "updated_at": value.get("updated_at"),
        "external_ref": external_ref,
        "shape": item_shape.value,
    })


def discover_tracker(children: Any, *, phase: str, tracker: str, epic: str) -> Result[dict[str, Any], ToolError]:
    if tracker not in {"bd", "br"}:
        return err("SOURCE.TRACKER", "tracker must be bd or br")
    if not isinstance(children, list):
        return err("SOURCE.TRACKER", "tracker children output must be an array")
    records = []
    for child in children:
        copy = dict(child) if isinstance(child, dict) else child
        if isinstance(copy, dict):
            copy["tracker"] = tracker
        record = _tracker_record(copy)
        if isinstance(record, Err):
            return record
        records.append(record.value)
    ids = [record["id"] for record in records]
    if len(set(ids)) != len(ids):
        return err("SOURCE.DUPLICATE_ID", "tracker sprint identifiers must be unique")
    records.sort(key=lambda record: _sprint_order(record["id"]))
    tracker_to_sprint = {record["tracker_id"]: record["id"] for record in records}
    for record in records:
        if not record["dependency_relation"]:
            blockers = [
                dependency.get("id") or dependency.get("depends_on_id")
                for dependency in record["_dependencies"]
                if isinstance(dependency, dict)
                and (dependency.get("dependency_type") or dependency.get("type")) == "blocks"
            ]
            blockers = [value for value in blockers if isinstance(value, str) and value]
            if blockers:
                names = [tracker_to_sprint.get(value, value) for value in blockers]
                record["dependency_relation"] = "must_follow " + ", ".join(names)
            else:
                record["dependency_relation"] = "root"
        del record["_dependencies"]
    return Ok({
        "schema": "plan-anchor/plan-source/1",
        "phase": phase,
        "source_kind": tracker,
        "source": epic,
        "source_revision": None,
        "sprints": records,
    })


def _tracker_json(
    repository_root: str | Path, tracker: str, arguments: list[str]
) -> Result[Any, ToolError]:
    """Run one fixed tracker argv and decode JSON; never invoke a shell."""

    if tracker not in {"bd", "br"}:
        return err("SOURCE.TRACKER", "tracker must be bd or br")
    binary = shutil.which(tracker)
    if binary is None:
        return err("PREREQ.MISSING", f"{tracker} is not installed")
    root = Path(repository_root).resolve(strict=True)
    result = subprocess.run(
        [binary, *arguments, "--json"],
        cwd=root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if result.returncode != 0:
        return err("SOURCE.TRACKER_COMMAND", f"{tracker} command failed")
    try:
        return Ok(json.loads(result.stdout))
    except json.JSONDecodeError:
        return err("SOURCE.TRACKER_OUTPUT", f"{tracker} returned invalid JSON")


def discover_tracker_epic(
    repository_root: str | Path, tracker: str, epic: str
) -> Result[dict[str, Any], ToolError]:
    """Discover an epic and its direct planning children through bd or br."""

    shown = _tracker_json(repository_root, tracker, ["show", epic])
    if isinstance(shown, Err):
        return shown
    epic_rows = shown.value
    if not isinstance(epic_rows, list) or len(epic_rows) != 1:
        return err("SOURCE.TRACKER_EPIC", "tracker epic lookup must return exactly one issue")
    epic_row = epic_rows[0]
    if not isinstance(epic_row, dict) or epic_row.get("id") != epic:
        return err("SOURCE.TRACKER_EPIC", "tracker epic lookup returned the wrong issue")
    if epic_row.get("issue_type") != "epic":
        return err("SOURCE.TRACKER_EPIC", "selected tracker issue is not an epic")
    phase_matches = PHASE_TITLE_RE.findall(str(epic_row.get("description") or ""))
    if len(phase_matches) != 1:
        return err("SOURCE.PHASE_ID", "tracker epic must contain exactly one Phase title")
    phase = "phase-" + phase_matches[0].lower().removeprefix("phase-")
    dependents = epic_row.get("dependents")
    if not isinstance(dependents, list):
        return err("SOURCE.TRACKER_EPIC", "tracker epic lacks a child list")
    child_ids = [
        value.get("id")
        for value in dependents
        if isinstance(value, dict) and value.get("dependency_type") == "parent-child"
    ]
    if not child_ids or any(not isinstance(value, str) or not value for value in child_ids):
        return err("SOURCE.TRACKER_EPIC", "tracker epic has no valid planning children")
    child_rows = _tracker_json(repository_root, tracker, ["show", *child_ids])
    if isinstance(child_rows, Err):
        return child_rows
    discovered = discover_tracker(child_rows.value, phase=phase, tracker=tracker, epic=epic)
    if isinstance(discovered, Err):
        return discovered
    plan = discovered.value
    plan["source_revision"] = epic_row.get("updated_at")
    return Ok(plan)


def _sprint_order(identifier: str) -> tuple[str, int, str]:
    match = re.fullmatch(r"([A-Za-z]+)(\d+)", identifier)
    return (match.group(1), int(match.group(2)), "") if match else (identifier, -1, identifier)


def _parity_sprints(document: Any) -> Result[list[dict[str, Any]], ToolError]:
    """The sprints of one discovered plan, or a typed error when the JSON is not one."""

    sprints = document.get("sprints") if isinstance(document, dict) else None
    if not isinstance(sprints, list) or not all(
        isinstance(item, dict)
        and isinstance(item.get("id"), str)
        and isinstance(item.get("normalized_sha256"), str)
        for item in sprints
    ):
        return err(
            "SOURCE.PARITY_INPUT",
            "parity input must be a discovered plan whose sprints carry id and normalized_sha256",
            action="Pass the data object from discover-markdown and discover-tracker output.",
        )
    return Ok(sprints)


def compare_parity(markdown: dict[str, Any], tracker: dict[str, Any]) -> Result[dict[str, Any], ToolError]:
    left_sprints = _parity_sprints(markdown)
    if isinstance(left_sprints, Err):
        return left_sprints
    right_sprints = _parity_sprints(tracker)
    if isinstance(right_sprints, Err):
        return right_sprints
    left = {item["id"]: item for item in left_sprints.value}
    right = {item["id"]: item for item in right_sprints.value}
    if list(left) != list(right):
        return err("SOURCE.PARITY_SET", "Markdown and tracker sprint order or identifiers differ")
    drift = []
    for sprint_id in left:
        if left[sprint_id]["normalized_sha256"] != right[sprint_id]["normalized_sha256"]:
            drift.append(sprint_id)
    if drift:
        return err("SOURCE.PARITY_DRIFT", "tracker copy differs from authoritative Markdown", data={"sprints": drift})
    return Ok({"equal": True, "sprints": list(left), "digests": {key: left[key]["normalized_sha256"] for key in left}})


def safe_copy_update(markdown_item: dict[str, Any], tracker_item: dict[str, Any] | None) -> Result[dict[str, Any], ToolError]:
    """Return a typed tracker update, refusing an independently changed copy."""

    if tracker_item is not None:
        recorded = tracker_item.get("recorded_source_sha256")
        current = tracker_item.get("normalized_sha256")
        authoritative = markdown_item["normalized_sha256"]
        if current != authoritative and (recorded is None or current != recorded):
            return err("SOURCE.COPY_CONFLICT", "tracker copy changed independently; refusing overwrite")
    return Ok({
        "tracker_id": tracker_item.get("tracker_id") if tracker_item else None,
        "sprint_id": markdown_item["id"],
        "description": markdown_item["text"],
        "source_path": markdown_item["path"],
        "source_sha256": markdown_item["normalized_sha256"],
    })


def copy_plan_to_tracker(
    repository_root: str | Path,
    markdown: dict[str, Any],
    tracker_plan: dict[str, Any],
    *,
    apply: bool = False,
) -> Result[dict[str, Any], ToolError]:
    """Verify or safely refresh exact tracker copies from authoritative Markdown."""

    tracker = tracker_plan.get("source_kind")
    if tracker not in {"bd", "br"}:
        return err("SOURCE.TRACKER", "tracker plan must come from bd or br")
    left = {item["id"]: item for item in markdown.get("sprints", [])}
    right = {item["id"]: item for item in tracker_plan.get("sprints", [])}
    if list(left) != list(right):
        return err("SOURCE.PARITY_SET", "Markdown and tracker sprint order or identifiers differ")
    actions = []
    for sprint_id, markdown_item in left.items():
        tracker_item = right[sprint_id]
        checked = safe_copy_update(markdown_item, tracker_item)
        if isinstance(checked, Err):
            return checked
        update = checked.value
        expected_ref = (
            f"plan-copy:{markdown_item['path']}"
            f"@sha256:{markdown_item['normalized_sha256']}"
        )
        content_equal = (
            markdown_item["normalized_sha256"] == tracker_item["normalized_sha256"]
        )
        metadata_equal = tracker_item.get("external_ref") == expected_ref
        action = {
            "sprint_id": sprint_id,
            "tracker_id": tracker_item["tracker_id"],
            "status": "verified" if content_equal and metadata_equal else "update_required",
            "content_equal": content_equal,
            "metadata_equal": metadata_equal,
            "source_path": update["source_path"],
            "source_sha256": update["source_sha256"],
        }
        if apply and action["status"] == "update_required":
            if not tracker_item.get("tracker_id") or not tracker_item.get("updated_at"):
                return err("SOURCE.COPY_METADATA", "tracker copy lacks identity or revision")
            with tempfile.TemporaryDirectory(prefix="plan-anchor-copy-") as temporary:
                description = Path(temporary) / "description.md"
                description.write_text(update["description"], encoding="utf-8")
                updated = _tracker_json(
                    repository_root,
                    tracker,
                    [
                        "update",
                        tracker_item["tracker_id"],
                        "--description-file",
                        str(description),
                        "--external-ref",
                        expected_ref,
                        "--if-unchanged",
                        tracker_item["updated_at"],
                    ],
                )
                if isinstance(updated, Err):
                    return updated
            action["status"] = "updated"
        actions.append(action)
    return Ok({
        "equal": all(action["content_equal"] for action in actions),
        "applied": apply,
        "actions": actions,
    })


def canonical_jev_body(model: str, anchor: dict[str, Any], item: dict[str, Any], questions: dict[str, Any]) -> bytes:
    """Build the exact transport body; source metadata is intentionally absent."""

    body = {
        "model": model,
        "state": {
            "anchor": {
                "id": anchor["id"],
                "statement": anchor["statement"],
                "scope": anchor.get("scope") or "",
            },
            "item": {"id": item["id"], "title": item.get("title") or "", "text": normalized_text(item["text"])},
        },
        "questions": questions,
    }
    return canonical_json(body)


def run_binding(plan: dict[str, Any], item: dict[str, Any]) -> dict[str, Any]:
    return {
        "phase": plan["phase"],
        "phase_source": plan["source"],
        "sprint_id": item["id"],
        "sprint_source": item["source_ref"],
        "source_kind": item["source_kind"],
        "source_revision": item["source_revision"],
        "normalized_sha256": item["normalized_sha256"],
    }


def _emit(value: dict[str, Any]) -> None:
    print(json.dumps(value, sort_keys=True, separators=(",", ":")))


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    markdown = sub.add_parser("discover-markdown")
    markdown.add_argument("target")
    markdown.add_argument("--repository-root", default=".")
    tracker = sub.add_parser("discover-tracker")
    tracker.add_argument("--children-json")
    tracker.add_argument("--tracker", required=True, choices=("bd", "br"))
    tracker.add_argument("--epic", required=True)
    tracker.add_argument("--phase")
    tracker.add_argument("--repository-root", default=".")
    parity = sub.add_parser("parity")
    parity.add_argument("--markdown-json", required=True)
    parity.add_argument("--tracker-json", required=True)
    shape_parser = sub.add_parser("shape")
    shape_parser.add_argument("path")
    shape_parser.add_argument("--repository-root", default=".")
    copy_parser = sub.add_parser("copy")
    copy_parser.add_argument("target")
    copy_parser.add_argument("--repository-root", default=".")
    copy_parser.add_argument("--tracker", required=True, choices=("bd", "br"))
    copy_parser.add_argument("--epic", required=True)
    copy_parser.add_argument("--apply", action="store_true")
    return parser


def _run(args: argparse.Namespace) -> Result[dict[str, Any], ToolError]:
    """Dispatch one subcommand. Expected failures come back as ``Err``; I/O errors still raise."""

    if args.command == "discover-markdown":
        return discover_markdown(args.repository_root, args.target)
    if args.command == "discover-tracker":
        if args.children_json:
            if not args.phase:
                return err("SOURCE.PHASE_ID", "--phase is required with --children-json")
            return discover_tracker(
                json.loads(Path(args.children_json).read_text(encoding="utf-8")),
                phase=args.phase,
                tracker=args.tracker,
                epic=args.epic,
            )
        return discover_tracker_epic(
            args.repository_root, args.tracker, args.epic
        )
    if args.command == "parity":
        left = json.loads(Path(args.markdown_json).read_text(encoding="utf-8"))
        right = json.loads(Path(args.tracker_json).read_text(encoding="utf-8"))
        return compare_parity(left, right)
    if args.command == "shape":
        root = Path(args.repository_root).resolve(strict=True)
        source = repository_file(root, args.path)
        if isinstance(source, Err):
            return source
        return shape(source.value.read_text(encoding="utf-8"))
    markdown = discover_markdown(args.repository_root, args.target)
    if isinstance(markdown, Err):
        return markdown
    tracker_plan = discover_tracker_epic(
        args.repository_root, args.tracker, args.epic
    )
    if isinstance(tracker_plan, Err):
        return tracker_plan
    return copy_plan_to_tracker(
        args.repository_root, markdown.value, tracker_plan.value, apply=args.apply
    )


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        outcome = _run(args)
        if isinstance(outcome, Err):
            error = outcome.error
            _emit({"success": False, "data": error.data, "error": error.to_json()})
            return 2
        _emit({"success": True, "data": outcome.value, "error": None})
        return 0
    except (OSError, UnicodeError, json.JSONDecodeError):
        _emit({"success": False, "data": None, "error": {"code": "SOURCE.INPUT", "message": "source input could not be read", "recoverable": False, "suggested_action": None}})
        return 2
    except Exception as exc:  # noqa: BLE001 - the CLI boundary: a JSON error object beats a traceback
        _emit({"success": False, "data": None, "error": {"code": "SCRIPT.INTERNAL", "message": f"{type(exc).__name__}: {exc}", "recoverable": False, "suggested_action": "Report this with the command line and its JSON inputs."}})
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
