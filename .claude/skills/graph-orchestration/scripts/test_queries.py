#!/usr/bin/env python3
"""
test_queries.py — Unit tests for graph-orchestration SPARQL queries.

Run from skill root: python3 -m pytest scripts/test_queries.py -v
Requires: rdflib, pytest
"""
import pytest
from pathlib import Path
from rdflib import Graph, URIRef, Namespace
from rdflib.namespace import XSD

SCRIPTS = Path(__file__).parent
TRIAGE = "urn:atm:triage:"
T = Namespace(TRIAGE)

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


if __name__ == "__main__":
    import subprocess, sys
    sys.exit(subprocess.call(["python3", "-m", "pytest", __file__, "-v"]))
