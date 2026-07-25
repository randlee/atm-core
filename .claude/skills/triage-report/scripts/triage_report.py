#!/usr/bin/env python3
"""Produce the canonical triage status report for a phase.

The calculations in this module are deliberately independent of presentation.
The JSON emitted by this command is the source of truth; the markdown template
only displays its already-calculated values.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from rdflib import Graph, Namespace, RDF
    _RDFLIB_ERROR = None
except ImportError as exc:  # pragma: no cover - environment error
    Graph = Namespace = RDF = None  # type: ignore[assignment]
    _RDFLIB_ERROR = str(exc)


TRIAGE = Namespace("urn:atm:triage:") if Namespace else None
UTC = timezone.utc
ICONS = {
    "assigned": "📥",
    "in_progress": "🌀",
    "done": "✅",
    "findings": "🚩",
    "fixing": "🔨",
    "blocked": "🚧",
    "fail": "❌",
    "merged": "🏁",
    "ready": "🚀",
}


class ReportError(RuntimeError):
    """An operational error which prevents a trustworthy report."""


def _git(cwd: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ["git", *args], cwd=cwd, text=True, capture_output=True, check=True
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = getattr(exc, "stderr", "") or str(exc)
        raise ReportError(f"git {' '.join(args)} failed: {detail.strip()}") from exc
    return result.stdout.strip()


def discover_integration_root(cwd: Path) -> Path:
    """Find the one current-phase integration worktree, fail closed if unclear."""
    cwd = Path(_git(cwd, "rev-parse", "--show-toplevel"))
    branch = _git(cwd, "branch", "--show-current")
    if re.fullmatch(r"integrate/phase-.+", branch):
        return cwd
    worktrees = []
    current_path: Path | None = None
    current_branch: str | None = None
    for line in _git(cwd, "worktree", "list", "--porcelain").splitlines() + [""]:
        if line.startswith("worktree "):
            current_path = Path(line.removeprefix("worktree "))
        elif line.startswith("branch refs/heads/integrate/phase-"):
            current_branch = line.removeprefix("branch refs/heads/")
        elif not line.strip():
            if current_path is not None and current_branch is not None:
                worktrees.append((current_path, current_branch))
            current_path = current_branch = None
    if len(worktrees) != 1:
        names = ", ".join(str(path) for path, _ in worktrees) or "none"
        raise ReportError(
            "cannot determine a unique integrate/phase-* worktree; "
            f"found {len(worktrees)} ({names}); pass --integration-root"
        )
    return worktrees[0][0]


def _phase_dir(root: Path, requested: str | None) -> tuple[str, Path]:
    sprints = root / ".sprints"
    if requested:
        phase = requested.removeprefix("phase-")
        path = sprints / phase
        if not (path / "structure.ttl").is_file():
            raise ReportError(f"missing phase structure: {path / 'structure.ttl'}")
        return phase, path
    candidates = sorted(p.parent for p in sprints.glob("*/structure.ttl"))
    if len(candidates) != 1:
        raise ReportError(
            "cannot determine a unique phase under .sprints; "
            f"found {len(candidates)}; pass --phase"
        )
    return candidates[0].name, candidates[0]


def _parse_ttl(path: Path) -> Graph:
    graph = Graph()
    try:
        graph.parse(path, format="turtle")
    except Exception as exc:  # noqa: BLE001 - convert parser failures to JSON error
        raise ReportError(f"{path}: malformed Turtle ({exc})") from exc
    return graph


def _local(value: Any) -> str:
    text = str(value)
    return text.rsplit(":", 1)[-1]


def _timestamp(value: Any) -> datetime | None:
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def _timestamp_text(value: Any) -> str | None:
    parsed = _timestamp(value)
    return parsed.isoformat().replace("+00:00", "Z") if parsed else None


def _json(path: Path) -> Any:
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        return None
    except (OSError, json.JSONDecodeError) as exc:
        raise ReportError(f"{path}: cannot read JSON ({exc})") from exc


def _sprints(structure: Graph, phase: str) -> list[dict[str, Any]]:
    result = []
    for subject in structure.subjects(RDF.type, TRIAGE.Sprint):
        phase_refs = list(structure.objects(subject, TRIAGE.inPhase))
        if len(phase_refs) != 1:
            raise ReportError(f"{_local(subject)} must have exactly one triage:inPhase")
        if _local(phase_refs[0]) != f"Phase{phase}":
            continue
        order_values = list(structure.objects(subject, TRIAGE.order))
        criteria_values = list(structure.objects(subject, TRIAGE.criteria))
        if len(order_values) != 1:
            raise ReportError(f"{_local(subject)} must have exactly one triage:order")
        if len(criteria_values) != 1:
            raise ReportError(f"{_local(subject)} must have exactly one triage:criteria")
        order_text = str(order_values[0]).strip()
        if not re.fullmatch(r"[0-9]+", order_text):
            raise ReportError(f"{_local(subject)} triage:order must be an integer")
        try:
            order = int(order_text)
        except ValueError as exc:
            raise ReportError(f"{_local(subject)} triage:order must be an integer") from exc
        result.append(
            {
                "id": _local(subject),
                "order": order,
                "criteria": str(criteria_values[0]),
            }
        )
    if not result:
        raise ReportError(f"{structure}: no sprints declared for phase {phase}")
    result.sort(key=lambda row: (row["order"], row["id"]))
    if any(left["order"] == right["order"] for left, right in zip(result, result[1:])):
        raise ReportError(f"{phase}: sprint orders must be unique")
    return result


def _dev_states(events: Graph | None) -> dict[str, dict[str, Any]]:
    states: dict[str, dict[str, Any]] = {}
    if events is None:
        return states
    for subject in events.subjects(RDF.type, TRIAGE.Assignment):
        sprint = next(events.objects(subject, TRIAGE.ofSprint), None)
        if sprint is None:
            continue
        key = _local(sprint)
        stamp = next(events.objects(subject, TRIAGE.assignedAt), None)
        candidate = _timestamp(stamp)
        current = states.setdefault(key, {})
        if candidate and (current.get("assignment_dt") is None or candidate > current["assignment_dt"]):
            current.update({"assignment_dt": candidate, "assignment_at": _timestamp_text(stamp)})
    for subject in events.subjects(RDF.type, TRIAGE.Completion):
        sprint = next(events.objects(subject, TRIAGE.ofSprint), None)
        if sprint is None:
            continue
        key = _local(sprint)
        stamp = next(events.objects(subject, TRIAGE.at), None)
        candidate = _timestamp(stamp)
        current = states.setdefault(key, {})
        if candidate and (current.get("completion_dt") is None or candidate > current["completion_dt"]):
            current.update({"completion_dt": candidate, "completion_at": _timestamp_text(stamp)})
    return states


def _qa_runs(master: Any) -> dict[str, dict[str, Any]]:
    if master is None:
        return {}
    if not isinstance(master, dict) or not isinstance(master.get("runs"), list):
        raise ReportError("QA evidence master must contain a runs array")
    latest: dict[str, dict[str, Any]] = {}
    for run in master.get("runs", []):
        if not isinstance(run, dict):
            raise ReportError("QA evidence master runs entries must be objects")
        if run.get("run_type", "qa") != "qa":
            continue
        sprint = run.get("aich_sprint")
        if not sprint:
            continue
        candidate = _timestamp(run.get("result_time_utc") or run.get("assignment_time_utc"))
        previous = latest.get(sprint)
        previous_dt = _timestamp(previous.get("result_time_utc")) if previous else None
        if previous is None or (candidate and (previous_dt is None or candidate > previous_dt)):
            latest[sprint] = run
    return latest


def _metadata(metadata: Any) -> dict[str, dict[str, Any]]:
    if metadata is None:
        return {}
    if not isinstance(metadata, dict) or not isinstance(metadata.get("sprints"), list):
        raise ReportError("metadata must contain a sprints array")
    values = metadata["sprints"]
    result: dict[str, dict[str, Any]] = {}
    for item in values:
        if not isinstance(item, dict) or not item.get("id"):
            raise ReportError("metadata.sprints entries must be objects with an id")
        key = str(item["id"])
        if key in result:
            raise ReportError(f"metadata contains duplicate sprint {key}")
        result[key] = item
    return result


def _count(run: dict[str, Any] | None, name: str) -> int | None:
    value = run.get(name) if run else None
    return value if isinstance(value, int) and not isinstance(value, bool) and value >= 0 else None


def _phase_sprint(criteria: str) -> str | None:
    match = re.search(r"sprint-(?:ai|AI)-([0-9]+)(-pre)?", criteria)
    if not match:
        return None
    return f"AI.{match.group(1)}{'-pre' if match.group(2) else ''}"


def _status_icon(status: str | None) -> str:
    normalized = status.lower().replace("-", "_") if isinstance(status, str) else ""
    if normalized in {"pass", "passed", "success", "green", "ok"}:
        return ICONS["done"]
    if normalized in {"fail", "failed", "failure", "red"}:
        return ICONS["fail"]
    if normalized in {"blocked", "failing", "error"}:
        return ICONS["blocked"]
    if normalized in {"pending", "running", "in_progress", "queued"}:
        return ICONS["in_progress"]
    return "—"


def _gate_icon(value: bool | None) -> str:
    if value is True:
        return ICONS["ready"]
    if value is False:
        return ICONS["blocked"]
    return "?"


def _pr_cell(meta: dict[str, Any]) -> str:
    number = meta.get("pr_number")
    if isinstance(number, int) and not isinstance(number, bool):
        return f"#{number}{' ' + ICONS['merged'] if meta.get('merged') is True else ''}"
    return "—"


def _source_path(path: Path | None, root: Path) -> str | None:
    if path is None:
        return None
    try:
        return str(path.relative_to(root))
    except ValueError:
        return f"<external>/{path.name}"


def _validate_metadata(item: dict[str, Any], sprint: str) -> None:
    for field in ("branch", "head_sha", "target_branch", "pr_url", "ci_status", "ci_url", "merge_commit", "merged_at_utc", "assignment_ack_message_id", "assignment_ack_time_utc"):
        if item.get(field) is not None and not isinstance(item[field], str):
            raise ReportError(f"metadata {sprint}.{field} must be a string or null")
    if item.get("pr_number") is not None and (not isinstance(item["pr_number"], int) or isinstance(item["pr_number"], bool)):
        raise ReportError(f"metadata {sprint}.pr_number must be an integer or null")
    if item.get("merged") is not None and not isinstance(item["merged"], bool):
        raise ReportError(f"metadata {sprint}.merged must be boolean or null")


def _validate_qa(run: dict[str, Any] | None, sprint: str) -> None:
    if run is None:
        return
    for field in ("blockers", "important", "minor"):
        if field in run and run[field] is not None and (not isinstance(run[field], int) or isinstance(run[field], bool) or run[field] < 0):
            raise ReportError(f"QA run {sprint}.{field} must be a non-negative integer or null")


def build_report(
    integration_root: Path,
    phase: str | None = None,
    qa_master: Path | None = None,
    metadata: Path | None = None,
) -> dict[str, Any]:
    """Build canonical report data. No presentation-layer inference occurs here."""
    if _RDFLIB_ERROR:
        raise ReportError(f"rdflib is required; install it with pip install rdflib ({_RDFLIB_ERROR})")
    root = Path(integration_root).resolve()
    phase_name, phase_path = _phase_dir(root, phase)
    structure_path = phase_path / "structure.ttl"
    events_path = phase_path / "events.ttl"
    structure = _parse_ttl(structure_path)
    events = _parse_ttl(events_path) if events_path.is_file() else None
    sprints = _sprints(structure, phase_name)

    plan_phase = None
    for candidate in sprints:
        match = re.search(r"docs/plans/([^/]+)/", candidate.get("criteria", ""))
        if match:
            plan_phase = match.group(1)
            break
    if qa_master is None:
        # The criteria document is the phase-name source of truth. This keeps
        # AICH (the graph namespace) distinct from phase-ai (the plan folder).
        qa_master = root / "docs" / "plans" / (plan_phase or f"phase-{phase_name.lower()}") / ".audit" / "qa-evidence-master.json"
    qa_master = Path(qa_master)
    if not qa_master.is_absolute():
        qa_master = root / qa_master
    qa_data = _json(qa_master)
    if metadata is not None and not metadata.is_absolute():
        metadata = root / metadata
    metadata_data = _json(metadata) if metadata else None
    qa = _qa_runs(qa_data)
    meta = _metadata(metadata_data)
    dev = _dev_states(events)
    data_gaps: list[str] = []
    if events is None:
        data_gaps.append(f"events file not found: {events_path}")
    if qa_data is None:
        data_gaps.append(f"QA evidence master not found: {qa_master}")
    if metadata is None:
        data_gaps.append("PR/CI/branch/merge metadata not supplied; those cells are unknown")
    elif metadata_data is None:
        data_gaps.append(f"metadata not found: {metadata}")

    rows: list[dict[str, Any]] = []
    for sprint in sprints:
        sid = sprint["id"]
        run = qa.get(sid)
        item = meta.get(sid, {})
        _validate_metadata(item, sid)
        _validate_qa(run, sid)
        counts = {name: _count(run, name) for name in ("blockers", "important", "minor")}
        verdict = str(run.get("verdict", "")) if run else None
        if not verdict and run and isinstance(run.get("pass"), bool):
            verdict = "PASS" if run["pass"] else "FAIL"
        dev_state = dev.get(sid, {})
        completion = dev_state.get("completion_dt")
        assignment = dev_state.get("assignment_dt")
        orphan_completion = completion is not None and assignment is None
        dev_done = completion is not None and assignment is not None and completion >= assignment
        dev_status = "done" if dev_done else ("in_progress" if assignment else None)
        if orphan_completion:
            data_gaps.append(f"{sid}: completion exists without an assignment")
        known_counts = all(value is not None for value in counts.values())
        quality_gate = (all(value == 0 for value in counts.values()) if known_counts else None)
        ready = None if counts["blockers"] is None else counts["blockers"] == 0
        rows.append(
            {
                **sprint,
                "phase_sprint": (run or {}).get("phase_sprint") or _phase_sprint(sprint["criteria"]),
                "dev_status": dev_status,
                "dev_assignment_at_utc": dev_state.get("assignment_at"),
                "dev_completion_at_utc": dev_state.get("completion_at"),
                "qa": {
                    "run_id": run.get("run_id") if run else None,
                    "assignment_time_utc": run.get("assignment_time_utc") if run else None,
                    "assignment_time_pst": run.get("assignment_time_pst") if run else None,
                    "assignment_message_id": run.get("assignment_message_id") if run else None,
                    "result_message_id": run.get("result_message_id") if run else None,
                    "result_time_utc": run.get("result_time_utc") if run else None,
                    "result_time_pst": run.get("result_time_pst") if run else None,
                    "assignment_shared_path": run.get("assignment_shared_path") if run else None,
                    "result_shared_path": run.get("result_shared_path") if run else None,
                    "result_temp_path": run.get("result_temp_path") if run else None,
                    "verdict": verdict or None,
                    "pass": run.get("pass") if run else None,
                    "blockers": counts["blockers"],
                    "important": counts["important"],
                    "minor": counts["minor"],
                    "count_basis": run.get("count_basis") if run else None,
                },
                "branch": item.get("branch"),
                "head_sha": item.get("head_sha"),
                "target_branch": item.get("target_branch"),
                "pr_number": item.get("pr_number"),
                "pr_url": item.get("pr_url"),
                "ci_status": item.get("ci_status"),
                "ci_url": item.get("ci_url"),
                "merged": item.get("merged") if isinstance(item.get("merged"), bool) else None,
                "merge_commit": item.get("merge_commit"),
                "merged_at_utc": item.get("merged_at_utc"),
                "assignment_ack_message_id": item.get("assignment_ack_message_id"),
                "assignment_ack_time_utc": item.get("assignment_ack_time_utc"),
                "quality_gate": quality_gate,
                "ready_to_merge": ready,
            }
        )
        if run is None:
            data_gaps.append(f"{sid}: no authoritative QA run")
        else:
            for field in ("blockers", "important", "minor"):
                if counts[field] is None:
                    data_gaps.append(f"{sid}: QA {field} count is missing or null")
        if sid not in meta:
            data_gaps.append(f"{sid}: no explicit PR/CI/branch/merge metadata")
        for field in ("branch", "head_sha", "target_branch", "pr_number", "pr_url", "ci_status", "merged", "assignment_ack_message_id", "assignment_ack_time_utc"):
            if item.get(field) is None:
                data_gaps.append(f"{sid}: metadata {field} is missing or null")

    for index, row in enumerate(rows):
        previous = rows[:index]
        if not previous:
            previous_merged: bool | None = True
        elif any(item["merged"] is False for item in previous):
            previous_merged = False
        elif any(item["merged"] is None for item in previous):
            previous_merged = None
        else:
            previous_merged = True
        row["previous_sprints_merged"] = previous_merged
        ready = row["ready_to_merge"]
        row["ok_to_merge"] = (
            True if ready is True and previous_merged is True else
            False if ready is False or previous_merged is False else None
        )
        row["dev_icon"] = ICONS.get(row["dev_status"], "—")
        qa_value = row["qa"]["verdict"]
        row["qa_icon"] = "✅" if qa_value and qa_value.upper() == "PASS" else (ICONS["fail"] if qa_value else "—")
        row["ci_icon"] = _status_icon(row["ci_status"])
        row["ready_icon"] = _gate_icon(row["ready_to_merge"])
        row["ok_icon"] = _gate_icon(row["ok_to_merge"])

    lines = []
    detailed = []
    for row in rows:
        q = row["qa"]
        phase_sprint = row["phase_sprint"] or "—"
        lines.append(
            f"| {row['id']} ({phase_sprint}) | {row['dev_icon']} | {row['qa_icon']} | "
            f"{row['ci_icon']} | {_pr_cell(row)} | {q['blockers'] if q['blockers'] is not None else '?'} | "
            f"{q['important'] if q['important'] is not None else '?'} | {q['minor'] if q['minor'] is not None else '?'} | "
            f"{row['ready_icon']} | {row['ok_icon']} |"
        )
        detailed.append(
            f"Sprint: {row['id']} ({phase_sprint})\n"
            f"DEV: {row['dev_icon']}  QA: {row['qa_icon']} {q['verdict'] or 'UNKNOWN'}  "
            f"CI: {row['ci_icon']}  PR: {_pr_cell(row)}\n"
            f"B/I/M: {q['blockers'] if q['blockers'] is not None else '?'} / "
            f"{q['important'] if q['important'] is not None else '?'} / "
            f"{q['minor'] if q['minor'] is not None else '?'}  "
            f"Ready: {row['ready_icon']}  OK: {row['ok_icon']}\n"
            f"QA assignment PST: {q['assignment_time_pst'] or 'unknown'}  result PST: {q['result_time_pst'] or 'unknown'}\n"
            f"Branch: {row['branch'] or 'unknown'}  Commit: {row['head_sha'] or 'unknown'}\n"
            f"PR URL: {row['pr_url'] or 'unknown'}"
        )
    integration_row = f"| **integrate/{plan_phase or ('phase-' + phase_name)}** | | — | — | — | — | — | — | — | — |"
    table = (
        "| Sprint | DEV | QA | CI | PR | B | I | M | Ready | OK |\n"
        "|--------|-----|----|----|----|---|---|---|-------|----|\n"
        + "\n".join(lines) + "\n" + integration_row
    )
    return {
        "kind": "triage-report/v1",
        "mode": "table",
        "phase": phase_name,
        "plan_phase": plan_phase,
        "timezone": "PST (UTC-08:00; fixed offset) for QA display; UTC is retained for source timestamps",
        "generated_at_utc": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
        "rows": rows,
        "sprint_rows": "\n".join(lines),
        "integration_row": integration_row,
        "detailed_rows": "\n────────────────────────────────────────\n".join(detailed),
        "table": table,
        "data_gaps": data_gaps,
        "sources": {
            "integration_root": ".",
            "structure": str(structure_path.relative_to(root)),
            "events": str(events_path.relative_to(root)) if events_path.is_file() else None,
            "qa_master": _source_path(qa_master, root),
            "metadata": _source_path(metadata, root),
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--integration-root", type=Path)
    parser.add_argument("--phase")
    parser.add_argument("--qa-master", type=Path)
    parser.add_argument("--metadata", type=Path)
    parser.add_argument("--format", choices=("table", "detailed", "json", "vars"), default="table")
    parser.add_argument("--mode", choices=("table", "detailed"), default="table", help="template display mode for --format vars")
    parser.add_argument("--json", action="store_true", help="alias for --format json")
    args = parser.parse_args(argv)
    try:
        root = args.integration_root or discover_integration_root(Path.cwd())
        report = build_report(root, args.phase, args.qa_master, args.metadata)
    except ReportError as exc:
        print(json.dumps({"kind": "error", "error": str(exc)}, sort_keys=True))
        return 2
    output_format = "json" if args.json else args.format
    report["mode"] = "detailed" if args.format == "detailed" else args.mode
    if output_format == "json":
        print(json.dumps(report, indent=2, sort_keys=True))
    elif output_format == "vars":
        # sc-compose var-files intentionally accept scalar/array values, not
        # the nested evidence objects in the canonical machine report.
        print(json.dumps({key: report[key] for key in (
            "mode", "phase", "plan_phase", "sprint_rows", "integration_row", "detailed_rows"
        )}, indent=2, sort_keys=True))
    elif output_format == "detailed":
        print(report["detailed_rows"])
    else:
        print(report["table"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
