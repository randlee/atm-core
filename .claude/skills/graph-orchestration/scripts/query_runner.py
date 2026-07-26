#!/usr/bin/env python3
"""
query_runner.py — deterministic next-dispatch resolution for graph-orchestration.

Called by next-dispatch. Returns JSON to stdout; errors to stderr.

The canonical result is a discriminated union with ``dispatch`` set to one of
``dispatch_task``, ``dispatch_fix``, ``awaiting_qa``, ``blocked`` or ``done``.
The old cursor-shaped result remains available only to the compatibility
``next-dev-task`` wrapper; new orchestration must call ``next-dispatch``.

Usage: query_runner.py <PHASE_LOCAL> <TTL_DIR> <SCRIPT_DIR> [--validate-only]
  PHASE_LOCAL  e.g. "F"
  TTL_DIR      path to .sprints/<PHASE>/ directory on integrate/phase-N branch
  SCRIPT_DIR   path to this script's directory (for .sparql files)
  --validate-only  validate structure.ttl/events.ttl and stop before dispatch resolution
"""

import importlib.util
import sys
import json
from datetime import datetime, timezone
from pathlib import Path

try:
    from rdflib import Graph, URIRef, Namespace
    from rdflib.namespace import RDF
except ImportError:
    print("ERROR: rdflib not installed. Run: pip3 install rdflib", file=sys.stderr)
    sys.exit(1)

TRIAGE_BASE = "urn:atm:triage:"
TRIAGE = Namespace(TRIAGE_BASE)
QA_CLOSED = TRIAGE.QAClosed
CLOSED_AT = TRIAGE.closedAt


def _find_repo_root(start: Path):
    """Walk up from start to find the directory containing .triage/"""
    current = start.resolve()
    for _ in range(10):  # max 10 levels up
        if (current / ".triage").exists():
            return current
        parent = current.parent
        if parent == current:
            break
        current = parent
    return None


IGNORE_FILE_NAME = ".graph-orchestration-ignore"


def _load_ignored_phase_dirs(repo_root: Path) -> set:
    """Read repo_root/.triage/.graph-orchestration-ignore into a set of names.

    This is a purely optional, best-effort efficiency/noise-reduction layer:
    it lets known-dead legacy phase directories (whose findings files use an
    old pre-Turtle format and will never be migrated) be skipped entirely
    before ever attempting to open or parse a file under them, avoiding both
    wasted I/O and a repeated stderr `WARNING: skipping malformed findings
    file ...` on every single invocation.

    It is NOT a correctness mechanism and must never be treated as one: the
    membership-based scoping in `load_graph()` (via `known_sprints` /
    `triage:foundIn`) is what actually prevents cross-phase contamination,
    for *every* findings directory, including ones not listed here. Absence
    of a directory from this ignore list — or absence of the file itself —
    changes nothing about correctness, only about how much redundant parsing
    and warning noise is produced for directories known in advance to be
    dead.
    """
    ignore_path = repo_root / ".triage" / IGNORE_FILE_NAME
    if not ignore_path.exists():
        return set()

    names = set()
    for line in ignore_path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        names.add(line)
    return names


def load_graph(ttl_dir: str, *, include_findings: bool = True) -> Graph:
    g = Graph()
    base = Path(ttl_dir)
    structure = base / "structure.ttl"
    events = base / "events.ttl"
    if not structure.exists():
        print(f"ERROR: structure.ttl not found at {structure}", file=sys.stderr)
        sys.exit(1)
    g.parse(str(structure), format="turtle")
    if events.exists():
        g.parse(str(events), format="turtle")

    # Collect the set of triage:Sprint subjects declared by *this phase's*
    # structure.ttl (+ events.ttl, though sprints are only ever declared in
    # structure.ttl in practice). This is the authoritative membership set
    # used below to scope findings — it is derived from the graph itself,
    # not from directory names or naming conventions, so it can't be
    # defeated by a future phase reusing an unprefixed or colliding local
    # sprint label.
    known_sprints = set(g.subjects(RDF.type, TRIAGE.Sprint))

    if not include_findings:
        return g

    # Load triage findings from repo root .triage/ (relative to TTL dir's parent)
    # Walk up from ttl_dir to find repo root (contains .triage/)
    #
    # Findings are filed under `.triage/<phase_id>/findings/*.ttl`, where
    # `<phase_id>` (e.g. "phase-AI") is a project-phase directory that does
    # not necessarily match the `PHASE_LOCAL` sprint-batch label (e.g.
    # "AICH") passed to this script — there is no discoverable
    # findings-path-to-phase mapping in structure.ttl today (no
    # `triage:findingsPath` or similar property), so we cannot statically
    # narrow the *glob* to "the one right directory" for a given phase.
    #
    # Directory-name matching was considered and rejected as the scoping
    # mechanism: it's convention-dependent (relies on `phase-AI` matching
    # `AICH`-prefixed sprint labels) and nothing enforces it — a future
    # phase could reuse an unprefixed or colliding local sprint label with
    # no directory-name collision to catch it, silently attaching a stray
    # well-formed finding to the wrong phase's sprint and corrupting cursor
    # resolution.
    #
    # Instead, each findings file is parsed into its own temporary Graph
    # first (still isolated in try/except, see below), and only the triples
    # belonging to findings whose `triage:foundIn` object is in
    # `known_sprints` (this phase's *actual* declared triage:Sprint
    # subjects, read directly from structure.ttl/events.ttl above) are
    # merged into `g`. This is real membership scoping, not a naming
    # convention: it is enforced by graph structure, so it cannot be
    # defeated by directory or label collisions. Findings whose
    # `triage:foundIn` points at a sprint outside this set are dropped
    # silently — they're simply out of scope for this phase, not an error.
    #
    # A single malformed (non-Turtle) findings file anywhere in the repo
    # must also not abort the entire parse and crash cursor resolution for
    # every phase, including ones with no relationship to the offending
    # file — hence the per-file try/except below, which remains necessary
    # defense-in-depth independent of the scoping filter.
    #
    # Some `.triage/<phase_id>/` directories are permanently closed/dead
    # legacy phases whose findings files were written in an old pre-Turtle
    # format and will never be migrated. Repeatedly globbing, opening, and
    # failing to parse those files on every single invocation is pure,
    # predictable waste: the outcome never changes. `.triage/.graph-
    # orchestration-ignore` (see `_load_ignored_phase_dirs`) lets such
    # directories be named explicitly so they're skipped before `.parse()`
    # is ever called on them — no warning is printed for these, since the
    # directory was deliberately and explicitly acknowledged as dead, unlike
    # an unexpected malformed file that wasn't. This is purely an efficiency
    # optimization layered on top of the membership-based scoping above; it
    # does not replace it, and directories absent from the ignore list are
    # still fully protected by the `known_sprints` membership filter.
    repo_root = _find_repo_root(base)
    if repo_root:
        ignored_phase_dirs = _load_ignored_phase_dirs(repo_root)
        for findings_file in sorted(repo_root.glob(".triage/*/findings/*.ttl")):
            phase_dir_name = findings_file.parent.parent.name
            if phase_dir_name in ignored_phase_dirs:
                continue
            file_graph = Graph()
            try:
                file_graph.parse(str(findings_file), format="turtle")
            except Exception as exc:  # noqa: BLE001 - isolate one bad file, keep loading others
                print(
                    f"WARNING: skipping malformed findings file {findings_file}: {exc}",
                    file=sys.stderr,
                )
                continue

            in_scope_findings = {
                finding
                for finding in file_graph.subjects(TRIAGE.foundIn, None)
                if file_graph.value(finding, TRIAGE.foundIn) in known_sprints
            }
            if not in_scope_findings:
                continue
            for finding in in_scope_findings:
                for triple in file_graph.triples((finding, None, None)):
                    g.add(triple)
                for resolution in file_graph.subjects(TRIAGE.resolves, finding):
                    for triple in file_graph.triples((resolution, None, None)):
                        g.add(triple)
    return g


def run_sparql(g: Graph, sparql_file: Path, bindings: dict) -> list:
    results = g.query(sparql_file.read_text(), initBindings=bindings)
    return list(results)


def _cli_load_graph(ttl_dir: str, *, include_findings: bool) -> Graph:
    """Load a graph while keeping malformed input errors CLI-friendly."""
    try:
        return load_graph(ttl_dir, include_findings=include_findings)
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        print(f"ERROR: query runner failed to load graph: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc


def _cli_run_sparql(g: Graph, sparql_file: Path, bindings: dict) -> list:
    """Run one bundled query while keeping query errors one-line."""
    try:
        return run_sparql(g, sparql_file, bindings)
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        print(f"ERROR: query runner failed to run {sparql_file}: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc


def _timestamp(value):
    """Return a timezone-aware datetime for an RDF timestamp, or ``None``."""

    if value is None:
        return None
    try:
        parsed = value.toPython() if hasattr(value, "toPython") else value
        if isinstance(parsed, datetime):
            if parsed.tzinfo is None:
                return parsed.replace(tzinfo=timezone.utc)
            return parsed
    except (TypeError, ValueError, OverflowError):
        pass
    text = str(value).strip()
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=timezone.utc)
    return parsed


def _local_sprint(sprint_iri: str) -> str:
    """Render an IRI as the stable local label used by task templates."""

    return sprint_iri.rsplit(":", 1)[-1]


def _qa_closed_state(g: Graph, phase_iri: URIRef) -> tuple[dict, list[str]]:
    """Collect immutable QAClosed events and reject malformed lifecycle data."""

    sprints = set(g.subjects(RDF.type, TRIAGE.Sprint))
    sprints = {
        sprint for sprint in sprints if (sprint, TRIAGE.inPhase, phase_iri) in g
    }
    closed: dict[URIRef, tuple[URIRef, datetime]] = {}
    diagnostics: list[str] = []
    for event in g.subjects(RDF.type, QA_CLOSED):
        sprint = g.value(event, TRIAGE.ofSprint)
        if sprint not in sprints:
            diagnostics.append(
                f"#error: QAClosed {event} targets undeclared sprint {sprint}"
            )
            continue
        # closedAt is the canonical property.  triage:at is accepted for
        # forward/backward compatibility with generic lifecycle events.
        raw_closed_at = g.value(event, CLOSED_AT)
        raw_at = g.value(event, TRIAGE.at)
        if raw_closed_at is not None and raw_at is not None and str(raw_closed_at) != str(raw_at):
            diagnostics.append(
                f"#error: QAClosed {event} has conflicting triage:closedAt and triage:at"
            )
        raw_timestamp = raw_closed_at or raw_at
        timestamp = _timestamp(raw_timestamp)
        if timestamp is None:
            diagnostics.append(
                f"#error: QAClosed {event} requires a valid triage:closedAt timestamp"
            )
            continue
        if sprint in closed:
            diagnostics.append(
                f"#error: sprint {_local_sprint(str(sprint))} has duplicate QAClosed events"
            )
            continue
        closed[sprint] = (event, timestamp)

    # Dispatch integrity also depends on assignment/completion timestamps. A
    # malformed event must fail closed instead of silently disappearing from a
    # SPARQL FILTER and allowing a duplicate assignment.
    for event_type, timestamp_predicate, label in (
        (TRIAGE.Assignment, TRIAGE.assignedAt, "Assignment"),
        (TRIAGE.Completion, TRIAGE.at, "Completion"),
    ):
        for event in g.subjects(RDF.type, event_type):
            sprint = g.value(event, TRIAGE.ofSprint)
            if sprint not in sprints:
                diagnostics.append(
                    f"#error: {label} {event} targets undeclared sprint {sprint}"
                )
            if _timestamp(g.value(event, timestamp_predicate)) is None:
                diagnostics.append(
                    f"#error: {label} {event} requires a valid {timestamp_predicate.n3()} timestamp"
                )
    return closed, diagnostics


def _findings_by_sprint(g: Graph) -> dict[URIRef, list[dict]]:
    """Return unresolved findings grouped by origin sprint."""

    findings: dict[URIRef, list[dict]] = {}
    for finding in g.subjects(RDF.type, TRIAGE.Finding):
        sprint = g.value(finding, TRIAGE.foundIn)
        if sprint is None:
            continue
        # A Resolution points to its finding.  Keeping this as a direct graph
        # lookup avoids treating unrelated resolution metadata as closure.
        if any(True for _ in g.subjects(TRIAGE.resolves, finding)):
            continue
        raw_found_at = g.value(finding, TRIAGE.foundAt)
        found_at = _timestamp(raw_found_at)
        severity = str(g.value(finding, TRIAGE.severity) or "").lower()
        finding_id = g.value(finding, TRIAGE.findingId)
        findings.setdefault(sprint, []).append(
            {
                "iri": str(finding),
                "id": str(finding_id) if finding_id is not None else None,
                "severity": severity,
                "found_at": found_at,
                "found_at_raw": str(raw_found_at) if raw_found_at is not None else None,
                "description": str(g.value(finding, TRIAGE.description) or ""),
            }
        )
    for values in findings.values():
        values.sort(key=lambda item: (item["found_at"] or datetime.min.replace(tzinfo=timezone.utc), item["iri"]))
    return findings


def _sprint_state(g: Graph, sprint: URIRef, findings: dict[URIRef, list[dict]]) -> dict:
    """Summarize assignment/completion state for one sprint."""

    assignments = []
    for assignment in g.subjects(RDF.type, TRIAGE.Assignment):
        if g.value(assignment, TRIAGE.ofSprint) != sprint:
            continue
        at = _timestamp(g.value(assignment, TRIAGE.assignedAt))
        assignments.append((assignment, at))
    completions = []
    for completion in g.subjects(RDF.type, TRIAGE.Completion):
        if g.value(completion, TRIAGE.ofSprint) != sprint:
            continue
        at = _timestamp(g.value(completion, TRIAGE.at))
        completions.append((completion, at))
    in_flight = any(
        assigned_at is not None
        and not any(completed_at is not None and completed_at > assigned_at for _, completed_at in completions)
        for _, assigned_at in assignments
    )
    open_findings = findings.get(sprint, [])
    blockers = [finding for finding in open_findings if finding["severity"] == "blocking"]
    return {
        "assignments": assignments,
        "completions": completions,
        "in_flight": in_flight,
        "open_findings": open_findings,
        "blockers": blockers,
    }


def _dispatch_result(
    g: Graph,
    phase_iri: URIRef,
    script_dir: Path,
) -> dict:
    """Resolve the next exact dispatch target without accepting overrides."""

    closed, closure_diagnostics = _qa_closed_state(g, phase_iri)
    if closure_diagnostics:
        raise ValueError("; ".join(closure_diagnostics))

    # The explicit query is an integrity gate, not a hint.  Assignments made
    # at/after closure are refusals even when a later sprint is available.
    closed_rows = _cli_run_sparql(
        g, script_dir / "closed-sprint-targets.sparql", {"PHASE": phase_iri}
    )
    if closed_rows:
        sprint_label = _local_sprint(str(closed_rows[0][1]))
        raise PermissionError(
            f"REFUSED: {sprint_label} is CLOSED. Post-closure findings must target "
            "the earliest open descendant or remediation sprint."
        )

    findings = _findings_by_sprint(g)
    sprints = []
    for sprint in g.subjects(RDF.type, TRIAGE.Sprint):
        if (sprint, TRIAGE.inPhase, phase_iri) not in g:
            continue
        order = g.value(sprint, TRIAGE.order)
        criteria = g.value(sprint, TRIAGE.criteria)
        state = _sprint_state(g, sprint, findings)
        sprints.append((int(order), sprint, str(criteria), state))
    sprints.sort(key=lambda item: (item[0], str(item[1])))

    # Derive late-finding promotions with the bundled query.  The graph-based
    # map below selects the same earliest eligible target and retains the
    # immutable origin on every promotion record.
    promotion_rows = _cli_run_sparql(
        g, script_dir / "post-closure-remediation.sparql", {"PHASE": phase_iri}
    )
    promotions_by_target: dict[URIRef, list[dict]] = {}
    promotion_seen: set[tuple[str, str]] = set()
    for row in promotion_rows:
        finding_iri = URIRef(str(row[0]))
        origin = URIRef(str(row[4]))
        candidate = URIRef(str(row[7]))
        key = (str(finding_iri), str(candidate))
        if key in promotion_seen:
            continue
        promotion_seen.add(key)
        # Only the earliest eligible candidate is a valid promotion target.
        origin_order = int(row[5])
        candidate_order = int(row[8])
        target_entry = next((entry for entry in sprints if entry[1] == candidate), None)
        if target_entry is None or target_entry[0] <= origin_order:
            continue
        target_state = target_entry[3]
        if target_entry[1] in closed or target_state["in_flight"] or target_state["blockers"]:
            continue
        prior = promotions_by_target.setdefault(candidate, [])
        if any(item["finding_iri"] == str(finding_iri) for item in prior):
            continue
        prior.append(
            {
                "finding_iri": str(finding_iri),
                "finding_id": str(row[1]) if row[1] else None,
                "severity": str(row[2] or "").lower(),
                "origin_sprint": _local_sprint(str(origin)),
                "target_sprint": _local_sprint(str(candidate)),
                "found_at": str(row[3]),
                "closed_at": str(row[6]),
                "candidate_order": candidate_order,
            }
        )
    for values in promotions_by_target.values():
        values.sort(key=lambda item: (item["finding_id"] or "", item["finding_iri"]))

    blocked_sprints = []
    in_flight_sprints = []
    for order, sprint, _, state in sprints:
        if sprint in closed:
            continue
        if state["blockers"]:
            blocked_sprints.append(
                {
                    "sprint": _local_sprint(str(sprint)),
                    "sprint_iri": str(sprint),
                    "order": order,
                    "finding_ids": [item["id"] for item in state["blockers"] if item["id"]],
                }
            )
        elif state["in_flight"]:
            in_flight_sprints.append(_local_sprint(str(sprint)))

    # next-dispatch.sparql is the canonical candidate ordering query.  The
    # Python state checks classify the selected row and add promotions.
    candidate_rows = _cli_run_sparql(
        g, script_dir / "next-dispatch.sparql", {"PHASE": phase_iri}
    )
    candidate_iris = [URIRef(str(row[0])) for row in candidate_rows]
    candidates = [entry for entry in sprints if entry[1] in candidate_iris or entry[1] in promotions_by_target]
    candidates.sort(key=lambda item: (item[0], str(item[1])))

    for order, sprint, criteria, state in candidates:
        if sprint in closed or state["in_flight"] or state["blockers"]:
            continue
        own_findings = list(state["open_findings"])
        promoted = promotions_by_target.get(sprint, [])
        all_findings = own_findings + [
            {
                "id": item["finding_id"],
                "iri": item["finding_iri"],
                "severity": item["severity"],
            }
            for item in promoted
        ]
        all_findings.sort(key=lambda item: (item.get("id") or "", item["iri"]))
        missing_ids = [item["iri"] for item in all_findings if not item.get("id")]
        if missing_ids:
            raise ValueError(
                f"dispatch_fix requires findingId for every outstanding finding; missing on {', '.join(missing_ids)}"
            )
        target = {
            "sprint": _local_sprint(str(sprint)),
            "sprint_iri": str(sprint),
            "sprint_order": order,
            "criteria_doc": criteria,
        }
        dispatch = "dispatch_fix" if all_findings else "dispatch_task"
        result = {
            "schema": "next-dispatch/v1",
            "dispatch": dispatch,
            "target": target,
            "outstanding_finding_ids": [item["id"] for item in all_findings],
            "promotions": promoted,
            "blocked_sprints": blocked_sprints,
            "in_flight_sprints": in_flight_sprints,
        }
        # Keep a compact projection for sc-compose templates; this is derived
        # output, never an input that can override the scheduler.
        result["vars"] = target | {"finding_ids": result["outstanding_finding_ids"]}
        return result

    unresolved_late = [
        promotion
        for values in promotions_by_target.values()
        for promotion in values
    ]
    # A late finding with no eligible descendant is a hard blocked state, not
    # a completed phase. This prevents an all-closed graph from hiding a QA
    # record that still needs remediation.
    late_without_target = []
    for origin, origin_findings in findings.items():
        if origin not in closed:
            continue
        closed_at = closed[origin][1]
        promoted_iris = {
            item["finding_iri"] for item in unresolved_late
        }
        for item in origin_findings:
            if item["found_at"] is not None and item["found_at"] > closed_at and item["iri"] not in promoted_iris:
                late_without_target.append(item)
    if late_without_target:
        return {
            "schema": "next-dispatch/v1",
            "dispatch": "blocked",
            "target": None,
            "outstanding_finding_ids": [item["id"] for item in late_without_target if item["id"]],
            "promotions": unresolved_late,
            "blocked_sprints": blocked_sprints,
            "in_flight_sprints": [],
            "reason": "late findings have no eligible open descendant or remediation sprint",
        }
    if in_flight_sprints:
        return {
            "schema": "next-dispatch/v1",
            "dispatch": "awaiting_qa",
            "target": None,
            "outstanding_finding_ids": [],
            "promotions": unresolved_late,
            "blocked_sprints": blocked_sprints,
            "in_flight_sprints": in_flight_sprints,
            "reason": "all eligible work is assigned and awaiting Completion/QA",
        }
    if blocked_sprints:
        return {
            "schema": "next-dispatch/v1",
            "dispatch": "blocked",
            "target": None,
            "outstanding_finding_ids": [],
            "promotions": unresolved_late,
            "blocked_sprints": blocked_sprints,
            "in_flight_sprints": [],
            "reason": "all remaining open sprints have unresolved blocking findings",
        }
    all_closed = bool(sprints) and all(sprint in closed for _, sprint, _, _ in sprints)
    return {
        "schema": "next-dispatch/v1",
        "dispatch": "done" if all_closed else "awaiting_qa",
        "target": None,
        "outstanding_finding_ids": [],
        "promotions": unresolved_late,
        "blocked_sprints": [],
        "in_flight_sprints": [],
        "reason": "all sprints are QAClosed" if all_closed else "all completions require QAClosed",
    }


def _load_validator(script_dir: Path):
    """Load the canonical findings validator without duplicating its rules."""

    validator_path = script_dir / "validate-findings.py"
    if not validator_path.exists():
        raise RuntimeError(f"validate-findings.py not found at {validator_path}")
    spec = importlib.util.spec_from_file_location("graph_orchestration_validator", validator_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"unable to load validator at {validator_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _validate_findings_before_query(ttl_dir: str, script_dir: Path) -> None:
    """Run raw findings validation before *any* graph query.

    ``load_graph`` intentionally scopes findings by ``foundIn``.  Running this
    gate first prevents malformed or incomplete records from disappearing in
    that scope filter.  A normal validation failure (exit-1 equivalent) and a
    validator execution error both stop cursor resolution; warnings alone are
    allowed by the validator's discriminated result contract.
    """

    repo_root = _find_repo_root(Path(ttl_dir))
    if repo_root is None:
        raise RuntimeError(
            "cannot locate repository root containing .triage; findings validation cannot run"
        )
    triage_root = repo_root / ".triage"
    findings_dirs = sorted(
        path for path in triage_root.glob("*/findings") if path.is_dir()
    ) if triage_root.exists() else []
    # An existing but empty .triage tree is a valid no-findings input.  Passing
    # the root directory still exercises the validator and gives a structured
    # error if the directory itself is missing or unreadable.
    if not findings_dirs:
        findings_dirs = [triage_root]

    validator = _load_validator(script_dir)
    structure = Path(ttl_dir) / "structure.ttl"
    events_path = Path(ttl_dir) / "events.ttl"
    events = events_path if events_path.exists() else None

    for findings_dir in findings_dirs:
        result = validator.run_validation(
            findings_dir=findings_dir,
            structure=structure,
            events=events,
            script_dir=script_dir,
        )
        if result.kind == "validation:pass":
            continue
        summary = getattr(result, "summary", None)
        counts = (
            f" ({summary.errors} error(s), {summary.warnings} warning(s))"
            if summary is not None
            else ""
        )
        print(
            f"ERROR: findings validation blocked query resolution for {findings_dir}"
            f"{counts}",
            file=sys.stderr,
        )
        for diagnostic in result.diagnostics[:20]:
            print(diagnostic, file=sys.stderr)
        if len(result.diagnostics) > 20:
            print(
                f"… {len(result.diagnostics) - 20} diagnostic line(s) truncated; "
                "run validate-findings.py directly for the full report",
                file=sys.stderr,
            )
        if result.kind == "error":
            raise SystemExit(2)
        raise SystemExit(1)


def _legacy_cursor_result(g: Graph, phase_iri: URIRef, script_dir: Path) -> dict:
    """Preserve the old cursor projection for existing consumers only."""

    cursor_rows = _cli_run_sparql(g, script_dir / "cursor.sparql", {"PHASE": phase_iri})
    if cursor_rows:
        sprint_iri = str(cursor_rows[0][0])
        return {
            "phase": "TRAVERSAL",
            "vars": {
                "sprint": _local_sprint(sprint_iri),
                "sprint_iri": sprint_iri,
                "sprint_order": int(cursor_rows[0][1]),
                "criteria_doc": str(cursor_rows[0][2]),
            },
        }
    incomplete_rows = _cli_run_sparql(g, script_dir / "all-complete.sparql", {"PHASE": phase_iri})
    if incomplete_rows:
        return {
            "phase": "AWAITING",
            "vars": {},
            "_incomplete_sprints": [str(row[0]) for row in incomplete_rows],
        }
    cleanup_rows = _cli_run_sparql(g, script_dir / "open-findings-sprint.sparql", {"PHASE": phase_iri})
    if cleanup_rows:
        return {
            "phase": "CLEANUP",
            "vars": {},
            "_findings_raw": [
                {
                    "sprint_iri": str(row[1]),
                    "severity": str(row[2]),
                    "foundAt": str(row[3]),
                    "description": str(row[4]),
                }
                for row in cleanup_rows
            ],
        }
    return {"phase": "DONE", "vars": {}, "_findings_raw": []}


def main():
    allowed_flags = {"--validate-only", "--legacy-cursor"}
    if len(sys.argv) < 4 or len(sys.argv) > 6 or any(flag not in allowed_flags for flag in sys.argv[4:]):
        print(
            "Usage: query_runner.py <PHASE_LOCAL> <TTL_DIR> <SCRIPT_DIR> "
            "[--validate-only] [--legacy-cursor]",
            file=sys.stderr,
        )
        sys.exit(1)

    phase_local = sys.argv[1]
    ttl_dir = sys.argv[2]
    script_dir = Path(sys.argv[3])
    validate_only = "--validate-only" in sys.argv[4:]
    legacy_cursor = "--legacy-cursor" in sys.argv[4:]

    phase_iri = URIRef(f"{TRIAGE_BASE}Phase{phase_local}")
    # This is deliberately before structure loading and before --validate-only:
    # every entry point must prove that raw findings are valid.
    try:
        _validate_findings_before_query(ttl_dir, script_dir)
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        print(f"ERROR: findings validation could not run: {exc}", file=sys.stderr)
        raise SystemExit(2) from exc

    g = _cli_load_graph(ttl_dir, include_findings=not validate_only)
    validate_rows = _cli_run_sparql(
        g, script_dir / "validate-structure.sparql", {"PHASE": phase_iri}
    )
    if validate_rows:
        for row in validate_rows:
            print(f"ERROR: structure violation: {row[0]} — {row[1]}", file=sys.stderr)
        sys.exit(1)

    if validate_only:
        try:
            _, lifecycle_diagnostics = _qa_closed_state(g, phase_iri)
            if lifecycle_diagnostics:
                for diagnostic in lifecycle_diagnostics:
                    print(f"ERROR: lifecycle violation: {diagnostic}", file=sys.stderr)
                raise SystemExit(1)
            closed_rows = _cli_run_sparql(
                g, script_dir / "closed-sprint-targets.sparql", {"PHASE": phase_iri}
            )
            if closed_rows:
                sprint_label = _local_sprint(str(closed_rows[0][1]))
                print(
                    f"REFUSED: {sprint_label} is CLOSED. Post-closure findings must target "
                    "the earliest open descendant or remediation sprint.",
                    file=sys.stderr,
                )
                raise SystemExit(1)
        except SystemExit:
            raise
        except Exception as exc:  # noqa: BLE001 - CLI validation boundary
            print(f"ERROR: lifecycle validation failed: {exc}", file=sys.stderr)
            raise SystemExit(1) from exc
        print(json.dumps({"schema": "next-dispatch/v1", "dispatch": "validate_only", "vars": {}}, indent=2))
        return

    try:
        result = _legacy_cursor_result(g, phase_iri, script_dir) if legacy_cursor else _dispatch_result(g, phase_iri, script_dir)
    except PermissionError as exc:
        print(str(exc), file=sys.stderr)
        raise SystemExit(1) from exc
    except ValueError as exc:
        print(f"ERROR: dispatch validation failed: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
