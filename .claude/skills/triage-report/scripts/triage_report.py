#!/usr/bin/env python3
"""Produce the canonical triage status report for a phase.

The calculations in this module are deliberately independent of presentation.
The JSON emitted by this command is the source of truth; the markdown template
only displays its already-calculated values.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from rdflib import Graph, Literal, Namespace, RDF
    _RDFLIB_ERROR = None
except ImportError as exc:  # pragma: no cover - environment error
    Graph = Literal = Namespace = RDF = None  # type: ignore[assignment]
    _RDFLIB_ERROR = str(exc)


TRIAGE = Namespace("urn:atm:triage:") if Namespace else None
UTC = timezone.utc
TERMINAL_FINDING_STATUSES = frozenset(
    {
        "absent",
        "accepted",
        "closed",
        "dismissed",
        "duplicate",
        "deferred",
        "false_positive",
        "fixed",
        "fixed-ci-green",
        "inherited-fix",
        "invalid",
        "merged",
        "waived",
    }
)
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

# ``validate-findings.py`` deliberately treats malformed Turtle as an
# operational error.  Reports are read-only, however, and must still render
# the valid sprint rows so an operator can see what is affected.  Keep this
# small recovery parser deliberately conservative: it only attributes a
# malformed file when exactly one declared sprint can be recovered from an
# explicit ``triage:foundIn`` token.  Anything else is unattributed and blocks
# dispatch/merge globally.
_FOUND_IN_TOKEN = re.compile(
    r"(?:triage:foundIn|<urn:atm:triage:foundIn>)\s+"
    r"(?:triage:([A-Za-z0-9_.-]+)|<([^>]+)>)"
)


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
        branch_values = list(structure.objects(subject, TRIAGE.branch))
        if len(order_values) != 1:
            raise ReportError(f"{_local(subject)} must have exactly one triage:order")
        if len(criteria_values) != 1:
            raise ReportError(f"{_local(subject)} must have exactly one triage:criteria")
        if len(branch_values) > 1:
            raise ReportError(f"{_local(subject)} may have at most one triage:branch")
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
                "iri": str(subject),
                "order": order,
                "criteria": str(criteria_values[0]),
                "branch": str(branch_values[0]) if branch_values else None,
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


def _phase_sprint(criteria: str) -> str | None:
    """Derive the human phase sprint label from a criteria filename.

    Criteria are conventionally named ``sprint-<prefix>-<number>`` (with an
    optional suffix such as ``-pre``) or, in this repo's longer-standing
    convention predating Phase AI, the separator-free ``sprint-<prefix><number>``
    (e.g. ``sprint-AJ1.md``).  Prefixes are intentionally not hard-coded to any
    phase: the same report producer is used by every phase.  A path that
    advertises the convention but cannot be parsed is rejected so a row is
    never silently labelled with the wrong sprint.
    """
    basename = Path(criteria).name
    match = re.search(
        r"^sprint-([A-Za-z][A-Za-z0-9]*?)[-.]?([0-9]+)(?:-([A-Za-z][A-Za-z0-9]*))?",
        basename,
    )
    if not match:
        if basename.lower().startswith("sprint-"):
            raise ReportError(
                f"unsupported sprint criteria filename {basename!r}; expected "
                "sprint-<prefix>-<number>[-suffix]"
            )
        return None
    suffix = f"-{match.group(3)}" if match.group(3) else ""
    return f"{match.group(1).upper()}.{match.group(2)}{suffix}"


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


def _run_findings_validator(
    root: Path,
    findings_dir: Path,
    structure_path: Path,
    events_path: Path,
) -> dict[str, Any]:
    """Run the canonical findings validator before any report is produced.

    ``validation:fail`` is a completed validation with invalid records, and
    therefore blocks report generation just like an operational validator
    error.  Keeping this subprocess boundary means this script always uses
    the exact validator shipped by graph-orchestration.
    """
    validator = (
        Path(__file__).resolve().parents[2]
        / "graph-orchestration"
        / "scripts"
        / "validate-findings.py"
    )
    if not validator.is_file():
        raise ReportError(f"findings validator not found: {validator}")
    command = [
        sys.executable,
        str(validator),
        "--findings-dir",
        str(findings_dir),
        "--structure",
        str(structure_path),
        "--events",
        str(events_path),
        "--json",
    ]
    try:
        result = subprocess.run(
            command,
            cwd=root,
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as exc:
        raise ReportError(f"could not execute findings validator: {exc}") from exc
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        detail = (result.stderr or result.stdout).strip()
        raise ReportError(
            f"findings validator returned non-JSON (exit {result.returncode}): {detail}"
        ) from exc
    if not isinstance(payload, dict) or payload.get("kind") not in {
        "validation:pass",
        "validation:fail",
        "error",
    }:
        raise ReportError("findings validator returned an invalid result object")
    # A validation failure is data that the report must display, not a reason
    # to suppress every unaffected sprint row.  Operational validator errors
    # (notably malformed finding TTL) are retained as ``kind:error`` and are
    # converted into structured report diagnostics below.  The report remains
    # merge/dispatch-blocked until those diagnostics are repaired.
    if payload.get("kind") == "validation:pass" and result.returncode == 0:
        return payload
    if payload.get("kind") in {"validation:fail", "error"}:
        return payload
    diagnostics = payload.get("diagnostics") or []
    detail = "; ".join(str(item) for item in diagnostics[:8])
    message = payload.get("message") or payload.get("kind") or "unknown result"
    if detail:
        message = f"{message}: {detail}"
    raise ReportError(f"findings validator returned inconsistent result: {message}")


def _malformed_finding_diagnostics(
    findings_dir: Path,
    structure: Graph,
    sprints: list[dict[str, Any]],
    root: Path,
) -> list[dict[str, Any]]:
    """Return actionable diagnostics for malformed finding files.

    ``load_graph`` skips an individual malformed file, which is exactly what
    the read-only report needs.  We retain the parse error and recover
    ``foundIn`` when the broken text still contains one unambiguous declared
    sprint.  A file without a recoverable target is explicitly unattributed;
    it still blocks dispatch/merge but cannot be blamed on a row.
    """
    if not findings_dir.is_dir():
        return []
    known = {str(item["iri"]): item["id"] for item in sprints}
    # Prefix-local values such as ``triage:AICH-S1`` resolve against the
    # canonical product namespace used by the graph.
    known_local = {iri.rsplit(":", 1)[-1]: (iri, sid) for iri, sid in known.items()}
    diagnostics: list[dict[str, Any]] = []
    for path in sorted(findings_dir.glob("*.ttl")):
        try:
            parsed = Graph()
            parsed.parse(str(path), format="turtle")
            continue
        except Exception as exc:  # noqa: BLE001 - report each bad file
            text = path.read_text(encoding="utf-8", errors="replace")
            candidates: set[str] = set()
            for local, full in _FOUND_IN_TOKEN.findall(text):
                iri = full or f"urn:atm:triage:{local}"
                if iri in known:
                    candidates.add(iri)
                elif local in known_local:
                    candidates.add(known_local[local][0])
            sprint_iri = next(iter(candidates)) if len(candidates) == 1 else None
            sprint_id = known.get(sprint_iri) if sprint_iri else None
            try:
                display_path = str(path.relative_to(root))
            except ValueError:
                display_path = str(path)
            diagnostics.append(
                {
                    "code": "malformed_finding_ttl" if sprint_id else "unattributed_malformed_finding_ttl",
                    "level": "error",
                    "path": display_path,
                    "absolute_path": str(path),
                    "sprint": sprint_id,
                    "sprint_iri": sprint_iri,
                    "message": f"malformed Turtle: {exc}",
                    "action": "repair Turtle syntax, then rerun validation before dispatch or merge",
                }
            )
    return diagnostics


def _validation_diagnostics(
    validation: dict[str, Any],
    malformed: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Normalize validator output into stable report diagnostics."""
    diagnostics = list(malformed)
    # Malformed parser diagnostics are represented with structured paths above;
    # avoid duplicating their raw text from the validator payload.
    malformed_paths = {item["path"] for item in malformed}
    for detail in validation.get("diagnostics") or []:
        text = str(detail)
        if "malformed Turtle" in text and any(path in text for path in malformed_paths):
            continue
        diagnostics.append(
            {
                "code": "finding_validation",
                "level": "error" if validation.get("kind") != "validation:pass" else "warning",
                "path": None,
                "absolute_path": None,
                "sprint": None,
                "sprint_iri": None,
                "message": text,
                "action": "repair the finding record and rerun validation before dispatch or merge",
            }
        )
    return diagnostics


def _diagnostic_text(diagnostic: dict[str, Any]) -> str:
    """Render one diagnostic with the exact path and repair action."""
    target = diagnostic.get("sprint") or "unattributed"
    path = diagnostic.get("path") or diagnostic.get("absolute_path") or "unknown path"
    return (
        f"[{target}] {path}: {diagnostic.get('message', 'data error')} "
        f"Action: {diagnostic.get('action', 'repair and revalidate')}"
    )


def _graph_runner():
    """Load graph-orchestration's public graph/query helpers once.

    The report must use the same current-integration resolver, finding scope,
    and resolution semantics as dispatch. Loading that module is preferable to
    a parallel Turtle loader or report-specific query logic.
    """
    runner_path = (
        Path(__file__).resolve().parents[2]
        / "graph-orchestration"
        / "scripts"
        / "query_runner.py"
    )
    spec = importlib.util.spec_from_file_location("triage_report_graph_runner", runner_path)
    if spec is None or spec.loader is None:
        raise ReportError(f"cannot load graph query runner: {runner_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _qa_counts(run: dict[str, Any] | None) -> dict[str, int | None]:
    """Return the final QA-run counts for one sprint, without replaying later work."""
    if run is None:
        return {"blockers": None, "important": None, "minor": None}
    counts: dict[str, int | None] = {}
    for name in ("blockers", "important", "minor"):
        value = run.get(name)
        counts[name] = value if isinstance(value, int) and value >= 0 else None
    return counts


def _live_counts(
    phase_path: Path,
    findings_dir: Path,
    sprints: list[dict[str, Any]],
) -> dict[str, dict[str, int]]:
    """Return unresolved B/I/M counts through the shared graph query.

    These are the merge and dispatch gates.  QA evidence describes the review
    that happened at a particular commit; it must not replace current TTL
    state when a real unresolved occurrence remains on a sprint branch.
    """
    runner = _graph_runner()
    query = (
        Path(__file__).resolve().parents[2]
        / "graph-orchestration"
        / "scripts"
        / "open-findings-for-sprint.sparql"
    )
    try:
        graph = runner.load_graph(str(phase_path), findings_dir=findings_dir)
    except Exception as exc:  # noqa: BLE001 - normalize graph runner failures
        raise ReportError(f"could not load live finding graph: {exc}") from exc

    results: dict[str, dict[str, int]] = {}
    for sprint in sprints:
        branch = sprint["branch"]
        bindings = {"SPRINT": runner.URIRef(sprint["iri"])}
        if branch:
            bindings["BRANCH"] = Literal(branch)
        try:
            rows = runner.run_sparql(graph, query, bindings)
        except Exception as exc:  # noqa: BLE001 - normalize graph runner failures
            raise ReportError(f"could not query live findings for {sprint['id']}: {exc}") from exc
        counts = {"blockers": 0, "important": 0, "minor": 0}
        for row in rows:
            severity = str(row[2]).lower()
            if severity in {"blocking", "critical"}:
                counts["blockers"] += 1
            elif severity == "important":
                counts["important"] += 1
            elif severity == "minor":
                counts["minor"] += 1
            else:
                raise ReportError(
                    f"{sprint['id']}: live findings query returned invalid severity {severity!r}"
                )
        results[sprint["id"]] = counts
    return results


def _current_integration_findings(
    findings_dir: Path,
) -> tuple[dict[str, int], dict[str, int], list[dict[str, str]]]:
    """Return deduplicated active/legacy counts plus stale occurrence diagnostics.

    A sprint row reports the QA result for that sprint's reviewed commit.  A
    later finding may mention an older branch occurrence, but it must not be
    replayed into that historical sprint's QA counts.  Current work belongs in
    the integration summary once, keyed by the canonical finding status.
    """
    active = {"blockers": 0, "important": 0, "minor": 0}
    legacy = {"blockers": 0, "important": 0, "minor": 0}
    stale: list[dict[str, str]] = []

    for path in sorted(findings_dir.glob("*.ttl")):
        try:
            graph = _parse_ttl(path)
        except ReportError:
            # The caller already emits a structured diagnostic for malformed
            # TTL and must keep unaffected rows visible.
            continue
        for finding in set(graph.subjects(RDF.type, TRIAGE.Finding)):
            finding_id = str(next(graph.objects(finding, TRIAGE.findingId), path.stem))
            statuses = {str(value).lower() for value in graph.objects(finding, TRIAGE.status)}
            is_terminal = bool(statuses & TERMINAL_FINDING_STATUSES)
            origin = _local(next(graph.objects(finding, TRIAGE.foundIn), ""))

            if is_terminal:
                for occurrence in graph.objects(finding, TRIAGE.hasOccurrence):
                    occurrence_statuses = {
                        str(value).lower() for value in graph.objects(occurrence, TRIAGE.status)
                    }
                    closed = {str(value).lower() for value in graph.objects(occurrence, TRIAGE.closed)}
                    if not (occurrence_statuses & TERMINAL_FINDING_STATUSES) and "true" not in closed:
                        stale.append(
                            {
                                "finding_id": finding_id,
                                "branch": str(next(graph.objects(occurrence, TRIAGE.branch), "unknown")),
                                "path": path.name,
                            }
                        )
                continue

            raw_severity = str(next(graph.objects(finding, TRIAGE.severity), "")).lower()
            if raw_severity in {"blocking", "critical"}:
                key = "blockers"
            elif raw_severity == "important":
                key = "important"
            elif raw_severity == "minor":
                key = "minor"
            else:
                raise ReportError(f"{path.name}: finding {finding_id} has invalid severity {raw_severity!r}")
            (legacy if origin.endswith("-Legacy") else active)[key] += 1
    return active, legacy, stale


def _origin_repo(root: Path) -> str | None:
    """Return owner/repo for a GitHub origin without hard-coding a repository."""
    try:
        origin = _git(root, "remote", "get-url", "origin")
    except ReportError:
        return None
    match = re.search(r"github\.com[:/]([^/]+)/([^/]+?)(?:\.git)?$", origin)
    return f"{match.group(1)}/{match.group(2)}" if match else None


def _github_prs(root: Path, repo: str, branch: str) -> list[dict[str, Any]] | None:
    """Read the complete PR history for one branch; return None if unavailable."""
    command = [
        "gh", "pr", "list", "--repo", repo, "--head", branch, "--state", "all",
        "--limit", "100", "--json",
        "number,state,headRefName,headRefOid,baseRefName,mergeCommit,mergedAt,url,"
        "statusCheckRollup,createdAt",
    ]
    try:
        result = subprocess.run(command, cwd=root, text=True, capture_output=True, check=False)
    except OSError:
        return None
    if result.returncode != 0:
        return None
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError:
        return None
    return payload if isinstance(payload, list) else None


def _ci_status(checks: Any) -> str | None:
    if not isinstance(checks, list) or not checks:
        return None
    conclusions = [
        str(check.get("conclusion") or check.get("status") or "").upper()
        for check in checks if isinstance(check, dict)
    ]
    if any(value in {"FAILURE", "FAILED", "ERROR", "TIMED_OUT", "CANCELLED"} for value in conclusions):
        return "fail"
    if any(value in {"IN_PROGRESS", "QUEUED", "PENDING", "WAITING", "REQUESTED"} for value in conclusions):
        return "pending"
    if conclusions and all(value in {"SUCCESS", "NEUTRAL", "SKIPPED"} for value in conclusions):
        return "pass"
    return None


def _current_pr(prs: list[dict[str, Any]]) -> dict[str, Any] | None:
    """Prefer the newest open replay over an older merged attempt."""
    if not prs:
        return None
    open_prs = [pr for pr in prs if pr.get("state") == "OPEN"]
    candidates = open_prs or prs
    return max(candidates, key=lambda pr: str(pr.get("createdAt") or pr.get("mergedAt") or ""))


def _github_state(root: Path, sprints: list[dict[str, Any]]) -> tuple[dict[str, dict[str, Any]], str | None]:
    """Collect current PR/CI state and preserve replay history per sprint."""
    repo = _origin_repo(root)
    if repo is None:
        return {}, None
    states: dict[str, dict[str, Any]] = {}
    for sprint in sprints:
        branch = sprint["branch"]
        if branch is None:
            states[sprint["id"]] = {}
            continue
        prs = _github_prs(root, repo, branch)
        if prs is None:
            states[sprint["id"]] = {"branch": branch}
            continue
        current = _current_pr(prs)
        if current is None:
            states[sprint["id"]] = {"branch": branch, "delivery_attempts": []}
            continue
        merge = current.get("mergeCommit")
        merge_oid = merge.get("oid") if isinstance(merge, dict) else None
        states[sprint["id"]] = {
            "branch": branch,
            "head_sha": current.get("headRefOid"),
            "target_branch": current.get("baseRefName"),
            "pr_number": current.get("number"),
            "pr_url": current.get("url"),
            "ci_status": _ci_status(current.get("statusCheckRollup")),
            "merged": current.get("state") == "MERGED",
            "merge_commit": merge_oid,
            "merged_at_utc": current.get("mergedAt"),
            "delivery_attempts": [
                {
                    "pr_number": pr.get("number"),
                    "head_sha": pr.get("headRefOid"),
                    "state": pr.get("state"),
                    "merged_at_utc": pr.get("mergedAt"),
                    "url": pr.get("url"),
                }
                for pr in sorted(prs, key=lambda pr: str(pr.get("createdAt") or ""))
            ],
        }
    return states, repo


def build_report(
    integration_root: Path,
    phase: str | None = None,
    qa_master: Path | None = None,
) -> dict[str, Any]:
    """Build canonical report data. No presentation-layer inference occurs here."""
    if _RDFLIB_ERROR:
        raise ReportError(f"rdflib is required; install it with pip install rdflib ({_RDFLIB_ERROR})")
    root = Path(integration_root).resolve()
    phase_name, requested_phase_path = _phase_dir(root, phase)
    runner = _graph_runner()
    try:
        source = runner.resolve_phase_source(phase_name, str(requested_phase_path))
    except Exception as exc:  # noqa: BLE001 - normalize shared source errors
        raise ReportError(f"could not resolve current integration phase source: {exc}") from exc
    root = source.root
    phase_path = source.ttl_dir
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
    # Validate raw findings before calculating rows.  Unlike dispatch, the
    # read-only report keeps going after a malformed finding file so valid
    # sprint rows remain visible.  ``diagnostics`` still blocks readiness and
    # merge/dispatch globally.
    findings_dir = source.findings_dir
    validation = _run_findings_validator(root, findings_dir, structure_path, events_path)
    malformed_diagnostics = _malformed_finding_diagnostics(
        findings_dir, structure, sprints, root
    )
    diagnostics = _validation_diagnostics(validation, malformed_diagnostics)
    qa = _qa_runs(qa_data)
    live_counts = _live_counts(phase_path, findings_dir, sprints)
    current_counts, legacy_counts, stale_occurrences = _current_integration_findings(findings_dir)
    github, github_repo = _github_state(root, sprints)
    dev = _dev_states(events)
    data_gaps: list[str] = []
    if events is None:
        data_gaps.append(f"events file not found: {events_path}")
    if qa_data is None:
        data_gaps.append(f"QA evidence master not found: {qa_master}")
    if github_repo is None:
        data_gaps.append("GitHub origin is unavailable; PR/CI/merge cells are unknown")
    data_gaps.extend(_diagnostic_text(item) for item in diagnostics)

    rows: list[dict[str, Any]] = []
    for sprint in sprints:
        sid = sprint["id"]
        row_diagnostics = [
            item for item in diagnostics if item.get("sprint") == sid
        ]
        row_errors = [item for item in row_diagnostics if item.get("level") == "error"]
        run = qa.get(sid)
        item = github.get(sid, {})
        counts = live_counts[sid]
        qa_snapshot = _qa_counts(run)
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
        merged = item.get("merged") if isinstance(item.get("merged"), bool) else None
        # A merged sprint is history, not a candidate awaiting another merge.
        ready = None if merged else (None if counts["blockers"] is None else counts["blockers"] == 0)
        if row_errors:
            # Affected rows are visibly blocked even when the malformed file
            # was skipped by the graph loader and therefore did not inflate a
            # B/I/M count.
            quality_gate = False
            ready = False
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
                    "count_basis": "live unresolved TTL findings",
                    "reported_counts": qa_snapshot,
                },
                "branch": item.get("branch"),
                "head_sha": item.get("head_sha"),
                "target_branch": item.get("target_branch"),
                "pr_number": item.get("pr_number"),
                "pr_url": item.get("pr_url"),
                "ci_status": item.get("ci_status"),
                "ci_url": item.get("ci_url"),
                "merged": merged,
                "merge_commit": item.get("merge_commit"),
                "merged_at_utc": item.get("merged_at_utc"),
                "delivery_attempts": item.get("delivery_attempts", []),
                "quality_gate": quality_gate,
                "ready_to_merge": ready,
                "data_status": "error" if row_errors else ("warning" if row_diagnostics else "ok"),
                "diagnostics": row_diagnostics,
            }
        )
        if run is None:
            data_gaps.append(f"{sid}: no authoritative QA run")
        for field in ("branch", "head_sha", "target_branch", "pr_number", "pr_url", "ci_status", "merged"):
            if item.get(field) is None:
                data_gaps.append(f"{sid}: GitHub {field} is missing or unknown")

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
            None
            if row["merged"] is True
            else (
                True if ready is True and previous_merged is True else
                False if ready is False or previous_merged is False else None
            )
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
        marker = " ⚠️" if row.get("diagnostics") else ""
        lines.append(
            f"| {row['id']} ({phase_sprint}){marker} | {row['dev_icon']} | {row['qa_icon']} | "
            f"{row['ci_icon']} | {_pr_cell(row)} | {q['blockers'] if q['blockers'] is not None else '?'} | "
            f"{q['important'] if q['important'] is not None else '?'} | {q['minor'] if q['minor'] is not None else '?'} | "
            f"{row['ready_icon']} | {row['ok_icon']} |"
        )
        detail = (
            f"Sprint: {row['id']} ({phase_sprint})\n"
            f"DEV: {row['dev_icon']}  QA: {row['qa_icon']} {q['verdict'] or 'UNKNOWN'}  "
            f"CI: {row['ci_icon']}  PR: {_pr_cell(row)}\n"
            f"Live B/I/M: {q['blockers'] if q['blockers'] is not None else '?'} / "
            f"{q['important'] if q['important'] is not None else '?'} / "
            f"{q['minor'] if q['minor'] is not None else '?'}  "
            f"QA snapshot B/I/M: {q['reported_counts']['blockers'] if q['reported_counts']['blockers'] is not None else '?'} / "
            f"{q['reported_counts']['important'] if q['reported_counts']['important'] is not None else '?'} / "
            f"{q['reported_counts']['minor'] if q['reported_counts']['minor'] is not None else '?'}  "
            f"Ready: {row['ready_icon']}  OK: {row['ok_icon']}\n"
            f"QA assignment PST: {q['assignment_time_pst'] or 'unknown'}  result PST: {q['result_time_pst'] or 'unknown'}\n"
            f"Branch: {row['branch'] or 'unknown'}  Commit: {row['head_sha'] or 'unknown'}\n"
            f"PR URL: {row['pr_url'] or 'unknown'}"
        )
        if row.get("diagnostics"):
            detail += "\nData diagnostics:\n" + "\n".join(
                f"- {_diagnostic_text(item)}" for item in row["diagnostics"]
            )
        detailed.append(detail)
    integration_row = f"| **integrate/{plan_phase or ('phase-' + phase_name)}** | | — | — | — | — | — | — | — | — |"
    table = (
        "| Sprint | DEV | QA | CI | PR | Live B | Live I | Live M | Ready | OK |\n"
        "|--------|-----|----|----|----|--------|--------|--------|-------|----|\n"
        + "\n".join(lines) + "\n" + integration_row
    )
    if diagnostics:
        table += "\n\nDiagnostics (dispatch/merge blocked):\n" + "\n".join(
            f"- {_diagnostic_text(item)}" for item in diagnostics
        )
    table += (
        "\n\nCurrent integration findings (deduplicated; not replayed into historical sprint QA): "
        f"B/I/M = {current_counts['blockers']} / {current_counts['important']} / {current_counts['minor']}."
    )
    if any(legacy_counts.values()):
        table += (
            " Legacy backlog (separate from active AICH work): "
            f"B/I/M = {legacy_counts['blockers']} / {legacy_counts['important']} / {legacy_counts['minor']}."
        )
    if stale_occurrences:
        table += (
            "\nTTL reconciliation required (does not reopen fixed findings): "
            f"{len(stale_occurrences)} terminal findings retain open occurrences."
        )
    merge_blocked = any(item.get("level") == "error" for item in diagnostics)
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
        "validation": validation,
        "diagnostics": diagnostics,
        "current_integration_counts": current_counts,
        "legacy_counts": legacy_counts,
        "stale_occurrences": stale_occurrences,
        "current_integration_summary": (
            "Current integration findings (deduplicated; not replayed into historical sprint QA): "
            f"B/I/M = {current_counts['blockers']} / {current_counts['important']} / {current_counts['minor']}."
        ),
        "legacy_summary": (
            "Legacy backlog (separate from active phase work): "
            f"B/I/M = {legacy_counts['blockers']} / {legacy_counts['important']} / {legacy_counts['minor']}."
        ) if any(legacy_counts.values()) else "",
        "stale_occurrences_summary": (
            "TTL reconciliation required (does not reopen fixed findings): "
            f"{len(stale_occurrences)} terminal findings retain open occurrences."
        ) if stale_occurrences else "",
        "dispatch_blocked": merge_blocked,
        "merge_blocked": merge_blocked,
        "sources": {
            "integration_root": ".",
            "structure": str(structure_path.relative_to(root)),
            "events": str(events_path.relative_to(root)) if events_path.is_file() else None,
            "qa_master": _source_path(qa_master, root),
            "github_repo": github_repo,
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--integration-root", type=Path)
    parser.add_argument("--phase")
    parser.add_argument("--qa-master", type=Path)
    parser.add_argument("--format", choices=("table", "detailed", "json", "vars"), default="table")
    parser.add_argument("--mode", choices=("table", "detailed"), default="table", help="template display mode for --format vars")
    parser.add_argument("--json", action="store_true", help="alias for --format json")
    args = parser.parse_args(argv)
    try:
        root = args.integration_root or discover_integration_root(Path.cwd())
        report = build_report(root, args.phase, args.qa_master)
    except ReportError as exc:
        print(
            json.dumps(
                {
                    "schema": "triage-report/v1",
                    "kind": "error",
                    "error_code": "report",
                    "message": str(exc),
                    "diagnostics": [],
                    "dispatch_blocked": True,
                    "merge_blocked": True,
                },
                sort_keys=True,
            )
        )
        return 2
    output_format = "json" if args.json else args.format
    report["mode"] = "detailed" if args.format == "detailed" else args.mode
    if output_format == "json":
        print(json.dumps(report, indent=2, sort_keys=True))
    elif output_format == "vars":
        # sc-compose var-files intentionally accept scalar/array values, not
        # the nested evidence objects in the canonical machine report.
        print(json.dumps({key: report[key] for key in (
            "mode", "phase", "plan_phase", "sprint_rows", "integration_row", "detailed_rows",
            "current_integration_summary", "legacy_summary", "stale_occurrences_summary",
            "data_gaps",
        )}, indent=2, sort_keys=True))
    elif output_format == "detailed":
        print(report["detailed_rows"])
    else:
        print(report["table"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
