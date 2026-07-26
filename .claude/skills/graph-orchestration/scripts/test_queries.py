#!/usr/bin/env python3
"""
test_queries.py — Unit tests for graph-orchestration SPARQL queries.

Run from skill root: python3 -m pytest scripts/test_queries.py -v
Requires: rdflib, pytest
"""
import importlib.util
import json
import os
import subprocess
import sys

from pathlib import Path
from rdflib import Graph, URIRef, Namespace

SCRIPTS = Path(__file__).parent
TRIAGE = "urn:atm:triage:"
T = Namespace(TRIAGE)


def _load_query_runner():
    """Import query_runner.py by path (it has no package __init__)."""
    spec = importlib.util.spec_from_file_location(
        "query_runner", SCRIPTS / "query_runner.py"
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules["query_runner"] = module
    spec.loader.exec_module(module)
    return module

PREFIX = f"@prefix triage: <{TRIAGE}> .\n@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n"
PHASE = URIRef(f"{TRIAGE}PhaseF")


def sparql(query_file: str, bindings: dict, *ttl_strings: str) -> list:
    g = Graph()
    for ttl in ttl_strings:
        g.parse(data=ttl, format="turtle")
    results = g.query((SCRIPTS / query_file).read_text(), initBindings=bindings)
    return list(results)


def structure(sprints: list) -> str:
    """Build structure TTL for a phase with N sprints.
    sprints = [(order, sprint_id), ...]
    """
    lines = [PREFIX, "triage:PhaseF a triage:Phase ."]
    for order, sid in sprints:
        lines.append(
            f'triage:{sid} a triage:Sprint ; triage:inPhase triage:PhaseF ;'
            f' triage:order {order} ; triage:criteria "ac/{sid}.md" .'
        )
    return "\n".join(lines)


# ── cursor tests ──────────────────────────────────────────────────────────────

class TestCursor:
    def test_returns_lowest_unstarted_sprint(self):
        s = structure([(1, "S1"), (2, "S2")])
        rows = sparql("cursor.sparql", {"PHASE": PHASE}, s)
        assert len(rows) == 1
        assert str(rows[0][1]) == "1"  # order 1

    def test_skips_in_flight_sprint(self):
        """Sprint with Assignment but no Completion is in-flight — skip it."""
        s = structure([(1, "S1"), (2, "S2")])
        events = PREFIX + """
triage:a1 a triage:Assignment ; triage:ofSprint triage:S1 ;
    triage:assignedTo "arch-ctm" ;
    triage:assignedAt "2026-07-01T10:00:00Z"^^xsd:dateTime .
"""
        rows = sparql("cursor.sparql", {"PHASE": PHASE}, s, events)
        assert len(rows) == 1
        assert str(rows[0][1]) == "2"  # S1 in-flight, returns S2

    def test_skips_validly_completed_sprint(self):
        """Sprint with valid Completion is done — skip it."""
        s = structure([(1, "S1"), (2, "S2")])
        events = PREFIX + """
triage:a1 a triage:Assignment ; triage:ofSprint triage:S1 ;
    triage:assignedTo "arch-ctm" ;
    triage:assignedAt "2026-07-01T10:00:00Z"^^xsd:dateTime .
triage:c1 a triage:Completion ; triage:ofSprint triage:S1 ;
    triage:at "2026-07-01T12:00:00Z"^^xsd:dateTime .
"""
        rows = sparql("cursor.sparql", {"PHASE": PHASE}, s, events)
        assert len(rows) == 1
        assert str(rows[0][1]) == "2"

    def test_returns_sprint_when_completion_invalidated(self):
        """Sprint with Completion + blocking finding after = invalid.
        Dev finished but QA invalidated. No new Assignment yet →
        cursor must return this sprint for re-dispatch."""
        s = structure([(1, "S1"), (2, "S2")])
        events = PREFIX + """
triage:a1 a triage:Assignment ; triage:ofSprint triage:S1 ;
    triage:assignedTo "arch-ctm" ;
    triage:assignedAt "2026-07-01T10:00:00Z"^^xsd:dateTime .
triage:c1 a triage:Completion ; triage:ofSprint triage:S1 ;
    triage:at "2026-07-01T12:00:00Z"^^xsd:dateTime .
triage:f1 a triage:Finding ; triage:foundIn triage:S1 ;
    triage:severity "blocking" ;
    triage:foundAt "2026-07-01T14:00:00Z"^^xsd:dateTime ;
    triage:description "Test failure" .
"""
        rows = sparql("cursor.sparql", {"PHASE": PHASE}, s, events)
        assert len(rows) == 1
        assert str(rows[0][1]) == "1"  # S1 must be returned for re-dispatch

    def test_skips_re_dispatched_sprint(self):
        """Sprint with invalid Completion + new Assignment (in-flight re-dispatch) → skip."""
        s = structure([(1, "S1"), (2, "S2")])
        events = PREFIX + """
triage:a1 a triage:Assignment ; triage:ofSprint triage:S1 ;
    triage:assignedTo "arch-ctm" ;
    triage:assignedAt "2026-07-01T10:00:00Z"^^xsd:dateTime .
triage:c1 a triage:Completion ; triage:ofSprint triage:S1 ;
    triage:at "2026-07-01T12:00:00Z"^^xsd:dateTime .
triage:f1 a triage:Finding ; triage:foundIn triage:S1 ;
    triage:severity "blocking" ;
    triage:foundAt "2026-07-01T14:00:00Z"^^xsd:dateTime ;
    triage:description "Test failure" .
triage:a2 a triage:Assignment ; triage:ofSprint triage:S1 ;
    triage:assignedTo "arch-ctm" ;
    triage:assignedAt "2026-07-01T15:00:00Z"^^xsd:dateTime .
"""
        rows = sparql("cursor.sparql", {"PHASE": PHASE}, s, events)
        assert len(rows) == 1
        assert str(rows[0][1]) == "2"  # S1 re-dispatched/in-flight → returns S2

    def test_returns_empty_when_all_complete(self):
        """All sprints with valid Completions → cursor empty."""
        s = structure([(1, "S1"), (2, "S2")])
        events = PREFIX + """
triage:c1 a triage:Completion ; triage:ofSprint triage:S1 ;
    triage:at "2026-07-01T12:00:00Z"^^xsd:dateTime .
triage:c2 a triage:Completion ; triage:ofSprint triage:S2 ;
    triage:at "2026-07-01T14:00:00Z"^^xsd:dateTime .
"""
        rows = sparql("cursor.sparql", {"PHASE": PHASE}, s, events)
        assert len(rows) == 0


class TestOpenFindingsForSprint:
    def test_normalizes_case_orders_and_excludes_terminal_statuses(self):
        findings = PREFIX + """
triage:minor a triage:Finding ; triage:findingId "M-1" ; triage:foundIn triage:S1 ;
    triage:foundAt "2026-07-01T09:00:00Z"^^xsd:dateTime ; triage:severity "MINOR" ; triage:description "minor" .
triage:important a triage:Finding ; triage:findingId "I-1" ; triage:foundIn triage:S1 ;
    triage:foundAt "2026-07-01T10:00:00Z"^^xsd:dateTime ; triage:severity "Important" ; triage:description "important" .
triage:blocking a triage:Finding ; triage:findingId "B-1" ; triage:foundIn triage:S1 ;
    triage:foundAt "2026-07-01T11:00:00Z"^^xsd:dateTime ; triage:severity "BLOCKING" ; triage:description "blocking" .
triage:resolved a triage:Finding ; triage:findingId "R-1" ; triage:foundIn triage:S1 ;
    triage:foundAt "2026-07-01T08:00:00Z"^^xsd:dateTime ; triage:severity "Blocking" ; triage:status "fixed" ; triage:description "resolved" .
triage:critical a triage:Finding ; triage:findingId "C-1" ; triage:foundIn triage:S1 ;
    triage:foundAt "2026-07-01T12:00:00Z"^^xsd:dateTime ; triage:severity "critical" ; triage:description "critical" .
triage:unknown a triage:Finding ; triage:findingId "U-1" ; triage:foundIn triage:S1 ;
    triage:foundAt "2026-07-01T13:00:00Z"^^xsd:dateTime ; triage:severity "medium" ; triage:description "invalid severity" .
"""
        rows = sparql(
            "open-findings-for-sprint.sparql",
            {"SPRINT": URIRef(f"{TRIAGE}S1")},
            findings,
        )
        assert [(str(row[1]), str(row[2]), str(row[3])) for row in rows] == [
            ("U-1", "invalid", "medium"),
            ("B-1", "blocking", "BLOCKING"),
            ("C-1", "blocking", "critical"),
            ("I-1", "important", "Important"),
            ("M-1", "minor", "MINOR"),
        ]


# ── validate-structure tests ──────────────────────────────────────────────────

class TestValidateStructure:
    def test_valid_structure_returns_zero_rows(self):
        s = structure([(1, "S1"), (2, "S2")])
        rows = sparql("validate-structure.sparql", {"PHASE": PHASE}, s)
        assert len(rows) == 0

    def test_detects_duplicate_order(self):
        ttl = PREFIX + """
triage:PhaseF a triage:Phase .
triage:S1 a triage:Sprint ; triage:inPhase triage:PhaseF ; triage:order 1 ; triage:criteria "ac/S1.md" .
triage:S2 a triage:Sprint ; triage:inPhase triage:PhaseF ; triage:order 1 ; triage:criteria "ac/S2.md" .
"""
        rows = sparql("validate-structure.sparql", {"PHASE": PHASE}, ttl)
        violations = [str(r[0]) for r in rows]
        assert "duplicate-order" in violations

    def test_detects_missing_criteria(self):
        ttl = PREFIX + """
triage:PhaseF a triage:Phase .
triage:S1 a triage:Sprint ; triage:inPhase triage:PhaseF ; triage:order 1 .
"""
        rows = sparql("validate-structure.sparql", {"PHASE": PHASE}, ttl)
        violations = [str(r[0]) for r in rows]
        assert "missing-criteria" in violations


# ── validate-findings tests ───────────────────────────────────────────────────

class TestValidateFindings:
    def test_complete_finding_returns_zero_rows(self):
        findings = PREFIX + """
triage:f1 a triage:Finding ; triage:findingId "F-1" ;
    triage:foundIn triage:S1 ; triage:foundAt
      "2026-07-01T12:00:00Z"^^xsd:dateTime ;
    triage:severity "important" ; triage:description "Issue" .
"""
        rows = sparql("validate-findings.sparql", {}, findings)
        assert rows == []

    def test_missing_graph_critical_fields_are_errors(self):
        findings = PREFIX + """
triage:f1 a triage:Finding ; triage:findingId "F-1" ;
    triage:severity "important" ; triage:description "Issue" .
"""
        rows = sparql("validate-findings.sparql", {}, findings)
        levels = {(str(row[0]), str(row[2])) for row in rows}
        assert ("#error", "triage:foundIn") in levels
        assert ("#error", "triage:foundAt") in levels

    def test_missing_descriptive_fields_are_warnings(self):
        findings = PREFIX + """
triage:f1 a triage:Finding ; triage:foundIn triage:S1 ;
    triage:foundAt "2026-07-01T12:00:00Z"^^xsd:dateTime .
"""
        rows = sparql("validate-findings.sparql", {}, findings)
        levels = {(str(row[0]), str(row[2])) for row in rows}
        assert ("#warning", "triage:findingId") in levels
        assert ("#warning", "triage:severity") in levels
        assert ("#warning", "triage:description") in levels


# ── open-findings-sprint tests ────────────────────────────────────────────────

class TestOpenFindingsSprint:
    def test_excludes_blocking_findings(self):
        s = structure([(1, "S1")])
        findings = PREFIX + """
triage:f1 a triage:Finding ; triage:foundIn triage:S1 ;
    triage:severity "blocking" ;
    triage:foundAt "2026-07-01T12:00:00Z"^^xsd:dateTime ;
    triage:description "blocker" .
"""
        rows = sparql("open-findings-sprint.sparql", {"PHASE": PHASE}, s, findings)
        assert len(rows) == 0

    def test_returns_important_before_minor(self):
        s = structure([(1, "S1")])
        findings = PREFIX + """
triage:f1 a triage:Finding ; triage:foundIn triage:S1 ;
    triage:severity "minor" ;
    triage:foundAt "2026-07-01T10:00:00Z"^^xsd:dateTime ;
    triage:description "minor issue" .
triage:f2 a triage:Finding ; triage:foundIn triage:S1 ;
    triage:severity "important" ;
    triage:foundAt "2026-07-01T11:00:00Z"^^xsd:dateTime ;
    triage:description "important issue" .
"""
        rows = sparql("open-findings-sprint.sparql", {"PHASE": PHASE}, s, findings)
        assert len(rows) == 2
        severities = [str(r[2]) for r in rows]
        assert severities[0] == "important"
        assert severities[1] == "minor"

    def test_excludes_resolved_findings(self):
        s = structure([(1, "S1")])
        data = PREFIX + """
triage:f1 a triage:Finding ; triage:foundIn triage:S1 ;
    triage:severity "important" ;
    triage:foundAt "2026-07-01T10:00:00Z"^^xsd:dateTime ;
    triage:description "issue" .
triage:r1 a triage:Resolution ; triage:resolves triage:f1 ;
    triage:resolvedAt "2026-07-01T12:00:00Z"^^xsd:dateTime .
"""
        rows = sparql("open-findings-sprint.sparql", {"PHASE": PHASE}, s, data)
        assert len(rows) == 0


# ── load_graph filesystem tests ────────────────────────────────────────────────
#
# These need real files on disk (not the in-memory `sparql()` helper above),
# since graph loading and phase-local finding selection are under test.

class TestLoadGraphFindingsIsolation:
    def _make_repo(self, tmp_path: Path, phase_local: str, sprints: list) -> Path:
        """Create a minimal repo layout: <repo>/.sprints/<phase_local>/structure.ttl
        plus an empty .triage/ marker so _find_repo_root() resolves the root."""
        repo = tmp_path / "repo"
        ttl_dir = repo / ".sprints" / phase_local
        ttl_dir.mkdir(parents=True)
        (ttl_dir / "structure.ttl").write_text(structure(sprints))
        (repo / ".triage").mkdir()
        return repo

    def test_malformed_findings_in_unrelated_phase_does_not_crash(self, tmp_path):
        """A non-Turtle legacy findings file under an unrelated phase directory
        must not crash load_graph() for the phase actually being queried."""
        query_runner = _load_query_runner()
        repo = self._make_repo(tmp_path, "F", [(1, "S1")])

        unrelated = repo / ".triage" / "phase-U" / "findings"
        unrelated.mkdir(parents=True)
        (unrelated / "LEGACY-001.ttl").write_text(
            "finding_id: LEGACY-001\ntitle: pre-Turtle legacy finding\nseverity: minor\n"
        )

        g = query_runner.load_graph(str(repo / ".sprints" / "F"))
        rows = list(
            g.query(
                (SCRIPTS / "cursor.sparql").read_text(),
                initBindings={"PHASE": URIRef(f"{TRIAGE}PhaseF")},
            )
        )
        assert len(rows) == 1
        assert str(rows[0][1]) == "1"

    def test_resolver_uses_current_integration_worktree_and_namespace(self, tmp_path, monkeypatch):
        """A sprint branch must never supply copied TTL or another phase's findings."""
        query_runner = _load_query_runner()
        branch_root = self._make_repo(tmp_path / "branch", "AICH", [(1, "S1")])
        integration_root = self._make_repo(tmp_path / "integration", "AICH", [(1, "S1")])

        monkeypatch.setattr(
            query_runner,
            "_integration_worktrees",
            lambda _root, _phase: [(integration_root, "integrate/phase-AI")],
        )

        source = query_runner.resolve_phase_source(
            "AICH", str(branch_root / ".sprints" / "AICH")
        )

        assert source.root == integration_root.resolve()
        assert source.ttl_dir == integration_root / ".sprints" / "AICH"
        assert source.findings_dir == integration_root / ".triage" / "phase-AI" / "findings"
        assert source.branch == "integrate/phase-AI"


    def test_wellformed_findings_in_queried_phase_still_loaded(self, tmp_path):
        """A well-formed, relevant findings file is still parsed and correctly
        drives Completion-invalidation, even with an unrelated malformed
        sibling file present in the glob."""
        query_runner = _load_query_runner()
        repo = self._make_repo(tmp_path, "F", [(1, "S1"), (2, "S2")])

        events = repo / ".sprints" / "F" / "events.ttl"
        events.write_text(
            PREFIX
            + """
triage:a1 a triage:Assignment ; triage:ofSprint triage:S1 ;
    triage:assignedTo "arch-ctm" ;
    triage:assignedAt "2026-07-01T10:00:00Z"^^xsd:dateTime .
triage:c1 a triage:Completion ; triage:ofSprint triage:S1 ;
    triage:at "2026-07-01T12:00:00Z"^^xsd:dateTime .
"""
        )

        relevant = repo / ".triage" / "phase-F" / "findings"
        relevant.mkdir(parents=True)
        (relevant / "ARCH-001.ttl").write_text(
            PREFIX
            + """
triage:f1 a triage:Finding ; triage:foundIn triage:S1 ;
    triage:severity "blocking" ;
    triage:foundAt "2026-07-01T14:00:00Z"^^xsd:dateTime ;
    triage:description "Test failure" .
"""
        )

        unrelated = repo / ".triage" / "phase-U" / "findings"
        unrelated.mkdir(parents=True)
        (unrelated / "LEGACY-001.ttl").write_text(
            "finding_id: LEGACY-001\ntitle: pre-Turtle legacy finding\nseverity: minor\n"
        )

        g = query_runner.load_graph(str(repo / ".sprints" / "F"))
        rows = list(
            g.query(
                (SCRIPTS / "cursor.sparql").read_text(),
                initBindings={"PHASE": URIRef(f"{TRIAGE}PhaseF")},
            )
        )
        # S1's Completion is invalidated by the blocking finding that
        # postdates it, so the cursor must snap back to S1 (order 1),
        # proving the well-formed findings file was actually loaded and
        # joined despite the malformed sibling file in another phase dir.
        assert len(rows) == 1
        assert str(rows[0][1]) == "1"

    def test_malformed_findings_in_queried_phase_itself_does_not_crash(self, tmp_path):
        """Defense in depth: even a malformed file within the correct/relevant
        phase directory must not crash the whole load."""
        query_runner = _load_query_runner()
        repo = self._make_repo(tmp_path, "F", [(1, "S1")])

        relevant = repo / ".triage" / "phase-F" / "findings"
        relevant.mkdir(parents=True)
        (relevant / "BROKEN.ttl").write_text("not: valid-turtle-at-all\n")

        g = query_runner.load_graph(str(repo / ".sprints" / "F"))
        rows = list(
            g.query(
                (SCRIPTS / "cursor.sparql").read_text(),
                initBindings={"PHASE": URIRef(f"{TRIAGE}PhaseF")},
            )
        )
        assert len(rows) == 1
        assert str(rows[0][1]) == "1"

    def test_cross_phase_findings_leakage_is_blocked(self, tmp_path):
        """Sprint-membership scoping: a well-formed finding filed under one
        phase's findings directory but pointing (via triage:foundIn) at a
        sprint that belongs to a DIFFERENT phase must not affect the
        queried phase's cursor result — even though nothing about the file
        itself is malformed, and even though its containing directory name
        does not collide with the queried phase's directory name.

        This proves scoping is enforced by actual sprint declaration
        (structure.ttl membership), not by directory-name convention: the
        two phases here (`PhaseX`/`X-S1` and `PhaseY`/`Y-S1`) use
        unprefixed, non-colliding local sprint labels, and the finding
        targets `Y-S1` specifically, yet querying PhaseX's TTL_DIR must
        come back clean.
        """
        query_runner = _load_query_runner()
        repo = tmp_path / "repo"
        (repo / ".triage").mkdir(parents=True)

        # PhaseX: single sprint X-S1, with an Assignment + Completion so
        # cursor.sparql would normally report PhaseX as fully complete
        # (cursor empty) — unless a stray finding wrongly invalidates it.
        x_dir = repo / ".sprints" / "X"
        x_dir.mkdir(parents=True)
        (x_dir / "structure.ttl").write_text(
            PREFIX
            + """
triage:PhaseX a triage:Phase .
triage:XS1 a triage:Sprint ; triage:inPhase triage:PhaseX ;
    triage:order 1 ; triage:criteria "ac/X-S1.md" .
"""
        )
        (x_dir / "events.ttl").write_text(
            PREFIX
            + """
triage:xa1 a triage:Assignment ; triage:ofSprint triage:XS1 ;
    triage:assignedTo "arch-ctm" ;
    triage:assignedAt "2026-07-01T10:00:00Z"^^xsd:dateTime .
triage:xc1 a triage:Completion ; triage:ofSprint triage:XS1 ;
    triage:at "2026-07-01T12:00:00Z"^^xsd:dateTime .
"""
        )

        # PhaseY: single sprint Y-S1, declared in its own structure.ttl —
        # this is what actually establishes Y-S1 as "known" when PhaseY is
        # queried, but it must NOT leak into a PhaseX query.
        y_dir = repo / ".sprints" / "Y"
        y_dir.mkdir(parents=True)
        (y_dir / "structure.ttl").write_text(
            PREFIX
            + """
triage:PhaseY a triage:Phase .
triage:YS1 a triage:Sprint ; triage:inPhase triage:PhaseY ;
    triage:order 1 ; triage:criteria "ac/Y-S1.md" .
"""
        )

        # A well-formed, valid-Turtle finding filed under PhaseX's own
        # findings directory (so directory-name matching alone would NOT
        # catch this), but its triage:foundIn points at Y-S1 — a sprint
        # that belongs to PhaseY, not PhaseX.
        findings_dir = repo / ".triage" / "phase-X" / "findings"
        findings_dir.mkdir(parents=True)
        (findings_dir / "CROSS-001.ttl").write_text(
            PREFIX
            + """
triage:fcross a triage:Finding ; triage:foundIn triage:YS1 ;
    triage:severity "blocking" ;
    triage:foundAt "2026-07-01T14:00:00Z"^^xsd:dateTime ;
    triage:description "Blocking finding against PhaseY's sprint" .
"""
        )

        g = query_runner.load_graph(str(x_dir))
        rows = list(
            g.query(
                (SCRIPTS / "cursor.sparql").read_text(),
                initBindings={"PHASE": URIRef(f"{TRIAGE}PhaseX")},
            )
        )
        # PhaseX's own sprint (X-S1) has a valid, uninvalidated Completion.
        # The blocking finding against Y-S1 must NOT be picked up when
        # querying PhaseX — if scoping were only directory-name-based (or
        # absent), this finding would still join via `?sprint
        # triage:inPhase $PHASE` filtering alone... but that's exactly the
        # convention-dependent behavior we're closing off further upstream:
        # here we assert the finding's triples aren't even in the graph.
        assert not list(g.triples((URIRef(f"{TRIAGE}fcross"), None, None)))
        assert len(rows) == 0  # PhaseX fully complete; no re-dispatch triggered

    def test_unrelated_malformed_findings_are_never_opened(self, tmp_path, capsys):
        """Historical phases cannot affect a current phase query at all."""
        query_runner = _load_query_runner()
        repo = self._make_repo(tmp_path, "F", [(1, "S1")])

        dead = repo / ".triage" / "phase-U" / "findings"
        dead.mkdir(parents=True)
        (dead / "LEGACY-001.ttl").write_text(
            "finding_id: LEGACY-001\ntitle: pre-Turtle legacy finding\n"
        )

        g = query_runner.load_graph(str(repo / ".sprints" / "F"))
        captured = capsys.readouterr()
        assert captured.err == ""

        rows = list(
            g.query(
                (SCRIPTS / "cursor.sparql").read_text(),
                initBindings={"PHASE": URIRef(f"{TRIAGE}PhaseF")},
            )
        )
        assert len(rows) == 1
        assert str(rows[0][1]) == "1"

    def test_selected_phase_malformed_findings_still_warn(self, tmp_path, capsys):
        """The chosen project phase remains visible to the loader and validator."""
        query_runner = _load_query_runner()
        repo = self._make_repo(tmp_path, "F", [(1, "S1")])
        selected = repo / ".triage" / "phase-F" / "findings"
        selected.mkdir(parents=True)
        (selected / "BROKEN.ttl").write_text(
            "finding_id: LEGACY-002\ntitle: unexpected malformed finding\n"
        )

        g = query_runner.load_graph(str(repo / ".sprints" / "F"))
        captured = capsys.readouterr()

        assert "BROKEN.ttl" in captured.err
        assert "WARNING: skipping malformed findings file" in captured.err

        rows = list(
            g.query(
                (SCRIPTS / "cursor.sparql").read_text(),
                initBindings={"PHASE": URIRef(f"{TRIAGE}PhaseF")},
            )
        )
        assert len(rows) == 1
        assert str(rows[0][1]) == "1"


# ── next-dev-task CLI tests ───────────────────────────────────────────────────

class TestNextDevTaskValidation:
    def _make_repo(self, tmp_path: Path, structure_ttl: str) -> Path:
        repo = tmp_path / "repo"
        ttl_dir = repo / ".sprints" / "F"
        ttl_dir.mkdir(parents=True)
        (ttl_dir / "structure.ttl").write_text(structure_ttl)
        (repo / ".triage").mkdir()
        return repo

    def _run(self, repo: Path, *args: str) -> subprocess.CompletedProcess:
        env = os.environ.copy()
        # Keep the shell wrapper and query runner on the same interpreter as
        # pytest; this also exercises the GRAPH_ORCH_PYTHON escape hatch used
        # by the dependency preflight.
        env["GRAPH_ORCH_PYTHON"] = sys.executable
        return subprocess.run(
            [str(SCRIPTS / "next-dev-task"), *args],
            cwd=repo,
            text=True,
            capture_output=True,
            env=env,
            check=False,
        )

    def test_validate_only_ignores_unrelated_malformed_findings(self, tmp_path):
        repo = self._make_repo(tmp_path, structure([(1, "S1")]))
        malformed = repo / ".triage" / "phase-U" / "findings"
        malformed.mkdir(parents=True)
        (malformed / "LEGACY.ttl").write_text("finding_id: LEGACY-001\n")

        result = self._run(repo, "F", str(repo / ".sprints" / "F"), "--validate-only")

        assert result.returncode == 0, result.stderr
        assert json.loads(result.stdout)["phase"] == "VALIDATE_ONLY"

    def test_selected_malformed_findings_return_json_error_union(self, tmp_path):
        repo = self._make_repo(tmp_path, structure([(1, "S1"), (2, "S2")]))
        malformed = repo / ".triage" / "phase-F" / "findings"
        malformed.mkdir(parents=True)
        broken = malformed / "BROKEN-S1.ttl"
        broken.write_text(
            PREFIX
            + "triage:broken a triage:Finding ; triage:foundIn triage:S1 ;\n"
        )

        result = self._run(repo, "F", str(repo / ".sprints" / "F"))

        assert result.returncode == 2
        payload = json.loads(result.stdout)
        assert payload["kind"] == "error"
        assert payload["error_code"] == "findings_validation"
        assert payload["dispatch_blocked"] is True
        assert any("BROKEN-S1.ttl" in item for item in payload["diagnostics"])
        assert "findings validation blocked query resolution" in result.stderr

    def test_cursor_resolution_is_blocked_by_missing_provenance(self, tmp_path):
        repo = self._make_repo(tmp_path, structure([(1, "S1")]))
        findings = repo / ".triage" / "phase-F" / "findings"
        findings.mkdir(parents=True)
        (findings / "F-1.ttl").write_text(
            PREFIX
            + 'triage:f1 a triage:Finding ; triage:findingId "F-1" ; '
            'triage:severity "important" ; triage:description "missing provenance" .\n'
        )

        result = self._run(repo, "F", str(repo / ".sprints" / "F"))

        assert result.returncode == 1, result.stderr
        payload = json.loads(result.stdout)
        assert payload["kind"] == "validation:fail"
        assert payload["error_code"] == "findings_validation"
        assert payload["dispatch_blocked"] is True
        assert "findings validation blocked query resolution" in result.stderr
        assert "triage:foundIn" in result.stderr
        assert "triage:foundAt" in result.stderr

    def test_cursor_resolution_runs_after_clean_findings_validation(self, tmp_path):
        repo = self._make_repo(tmp_path, structure([(1, "S1")]))
        findings = repo / ".triage" / "phase-F" / "findings"
        findings.mkdir(parents=True)
        (findings / "F-1.ttl").write_text(
            PREFIX
            + 'triage:f1 a triage:Finding ; triage:findingId "F-1" ; '
            'triage:foundIn triage:S1 ; '
            'triage:foundAt "2026-07-01T12:00:00Z"^^xsd:dateTime ; '
            'triage:severity "important" ; triage:description "clean" .\n'
        )

        result = self._run(repo, "F", str(repo / ".sprints" / "F"))

        assert result.returncode == 0, result.stderr
        payload = json.loads(result.stdout)
        assert payload["phase"] == "TRAVERSAL"
        assert payload["vars"]["sprint"] == "S1"
        assert payload["findings"] == [
            {
                "finding_iri": f"{TRIAGE}f1",
                "finding_id": "F-1",
                "severity": "important",
                "raw_severity": "important",
                "status": None,
                "found_at": "2026-07-01T12:00:00+00:00",
                "description": "clean",
            }
        ]

    def test_invalid_severity_returns_json_and_fails_closed(self, tmp_path):
        repo = self._make_repo(tmp_path, structure([(1, "S1")]))
        findings = repo / ".triage" / "phase-F" / "findings"
        findings.mkdir(parents=True)
        (findings / "F-1.ttl").write_text(
            PREFIX
            + 'triage:f1 a triage:Finding ; triage:findingId "F-1" ; '
            'triage:foundIn triage:S1 ; '
            'triage:foundAt "2026-07-01T12:00:00Z"^^xsd:dateTime ; '
            'triage:severity "medium" ; triage:description "unknown" .\n'
        )

        result = self._run(repo, "F", str(repo / ".sprints" / "F"))

        assert result.returncode == 1, result.stderr
        payload = json.loads(result.stdout)
        assert payload["phase"] == "INVALID_FINDING_SEVERITY"
        assert payload["findings"][0]["raw_severity"] == "medium"

    def test_validate_only_reports_structure_errors(self, tmp_path):
        invalid = PREFIX + """
triage:PhaseF a triage:Phase .
triage:S1 a triage:Sprint ; triage:inPhase triage:PhaseF ;
    triage:order 1 ; triage:criteria "ac/S1.md" .
triage:S2 a triage:Sprint ; triage:inPhase triage:PhaseF ;
    triage:order 1 ; triage:criteria "ac/S2.md" .
"""
        repo = self._make_repo(tmp_path, invalid)

        result = self._run(repo, "F", str(repo / ".sprints" / "F"), "--validate-only")

        assert result.returncode == 1
        assert "ERROR: structure violation" in result.stderr
        payload = json.loads(result.stdout)
        assert payload["kind"] == "validation:fail"
        assert payload["error_code"] == "structure_validation"

    def test_malformed_structure_is_reported_without_traceback(self, tmp_path):
        repo = self._make_repo(tmp_path, "not valid Turtle [")

        result = self._run(repo, "F", str(repo / ".sprints" / "F"))

        assert result.returncode == 1
        assert "Traceback" not in result.stderr
        assert "ERROR: query runner failed to load graph" in result.stderr
        payload = json.loads(result.stdout)
        assert payload["kind"] == "error"
        assert payload["error_code"] == "graph_load"

    def test_broken_sparql_is_reported_without_traceback(self, tmp_path):
        repo = self._make_repo(tmp_path, structure([(1, "S1")]))
        broken_scripts = tmp_path / "scripts"
        broken_scripts.mkdir()
        (broken_scripts / "validate-structure.sparql").write_text(
            "SELECT definitely broken"
        )
        (broken_scripts / "validate-findings.py").write_text(
            (SCRIPTS / "validate-findings.py").read_text()
        )
        (broken_scripts / "validate-findings.sparql").write_text(
            (SCRIPTS / "validate-findings.sparql").read_text()
        )

        result = subprocess.run(
            [
                sys.executable,
                str(SCRIPTS / "query_runner.py"),
                "F",
                str(repo / ".sprints" / "F"),
                str(broken_scripts),
            ],
            cwd=repo,
            text=True,
            capture_output=True,
            check=False,
        )

        assert result.returncode == 1
        assert "Traceback" not in result.stderr
        assert "ERROR: query runner failed to run" in result.stderr
        payload = json.loads(result.stdout)
        assert payload["kind"] == "error"
        assert payload["error_code"] == "sparql_query"

    def test_unknown_optional_argument_is_rejected(self, tmp_path):
        repo = self._make_repo(tmp_path, structure([(1, "S1")]))

        result = self._run(repo, "F", str(repo / ".sprints" / "F"), "--bogus")

        assert result.returncode == 1
        assert "Usage: next-dev-task" in result.stderr


# ── assignee-busy CLI tests ───────────────────────────────────────────────────

class TestAssigneeBusy:
    def _make_repo(self, tmp_path: Path, events_ttl: str | None = None) -> Path:
        repo = tmp_path / "repo"
        ttl_dir = repo / ".sprints" / "F"
        ttl_dir.mkdir(parents=True)
        (ttl_dir / "structure.ttl").write_text(structure([(1, "S1")]))
        if events_ttl is not None:
            (ttl_dir / "events.ttl").write_text(events_ttl)
        (repo / ".triage").mkdir()
        return repo

    def _run(self, repo: Path, *args: str) -> subprocess.CompletedProcess:
        env = os.environ.copy()
        env["GRAPH_ORCH_PYTHON"] = sys.executable
        return subprocess.run(
            [str(SCRIPTS / "assignee-busy"), *args],
            cwd=repo,
            text=True,
            capture_output=True,
            env=env,
            check=False,
        )

    def test_reports_only_truly_in_flight_assignments(self, tmp_path):
        events = PREFIX + """
triage:a1 a triage:Assignment ; triage:ofSprint triage:S1 ;
    triage:assignedTo "arch-ctm" ;
    triage:assignedAt "2026-07-01T10:00:00Z"^^xsd:dateTime .
triage:c1 a triage:Completion ; triage:ofSprint triage:S1 ;
    triage:at "2026-07-01T12:00:00Z"^^xsd:dateTime .
"""
        repo = self._make_repo(tmp_path, events)

        result = self._run(repo, "F", str(repo / ".sprints" / "F"), "arch-ctm")

        assert result.returncode == 0, result.stderr
        assert json.loads(result.stdout) == {
            "busy": False,
            "assignee": "arch-ctm",
            "open_assignments": [],
        }

    def test_rejects_extra_arguments(self, tmp_path):
        repo = self._make_repo(tmp_path)

        result = self._run(
            repo,
            "F",
            str(repo / ".sprints" / "F"),
            "arch-ctm",
            "unexpected",
        )

        assert result.returncode == 1
        assert "Usage: assignee-busy" in result.stderr

    def test_reports_malformed_structure_as_cli_error(self, tmp_path):
        repo = self._make_repo(tmp_path)
        (repo / ".sprints" / "F" / "structure.ttl").write_text("not valid turtle [")

        result = self._run(repo, "F", str(repo / ".sprints" / "F"), "arch-ctm")

        assert result.returncode == 1
        assert "Traceback" not in result.stderr
        assert "ERROR:" in result.stderr
        payload = json.loads(result.stdout)
        assert payload["kind"] == "error"
        assert payload["error_code"] == "assignee_check"
        assert payload["dispatch_blocked"] is True


if __name__ == "__main__":
    sys.exit(subprocess.call(["python3", "-m", "pytest", __file__, "-v"]))
