"""Unit tests for the closing-triage query's sprint and worktree selection.

Requires: rdflib (not a bootstrap dependency, so this file is run directly like
the graph-orchestration script tests: ``python3 <this file>``).
"""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

from rdflib import Graph, Namespace, URIRef

SCRIPT = Path(__file__).with_name("query_open_findings.py")
SPEC = importlib.util.spec_from_file_location("query_open_findings", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
qof = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(qof)

TRIAGE = Namespace("urn:atm:triage:")

BB_STRUCTURE = """\
@prefix triage: <urn:atm:triage:> .
triage:PhaseBB a triage:Phase .
triage:BB3 a triage:Sprint ; triage:inPhase triage:PhaseBB ; triage:order 3 ; triage:branch "fix/bb3-qa2-r2" ; triage:criteria "docs/plans/phase-bb/sprint-BB.3.md" .
triage:BB6 a triage:Sprint ; triage:inPhase triage:PhaseBB ; triage:order 6 ; triage:branch "feature/bb6-prompt-handoffs" ; triage:criteria "docs/plans/phase-bb/sprint-BB.6.md" .
"""

AJ_STRUCTURE = """\
@prefix triage: <urn:atm:triage:> .
triage:PhaseAJ a triage:Phase .
triage:BB6 a triage:Sprint ; triage:inPhase triage:PhaseAJ ; triage:order 1 ; triage:branch "feature/aj-dup" ; triage:criteria "docs/plans/phase-aj/sprint-AJ1.md" .
"""


def _write_structures(root: Path, phases: dict[str, str]) -> None:
    for phase, text in phases.items():
        phase_dir = root / ".sprints" / phase
        phase_dir.mkdir(parents=True)
        (phase_dir / "structure.ttl").write_text(text)


class SprintSelectionTests(unittest.TestCase):
    def test_declared_branch_still_maps_without_sprint_id(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _write_structures(root, {"BB": BB_STRUCTURE})
            phase, _, sprint = qof._sprint_for_branch(root, "feature/bb6-prompt-handoffs", None)
        self.assertEqual(phase, "BB")
        self.assertEqual(sprint, TRIAGE["BB6"])

    def test_undeclared_stacked_layer_fails_closed_and_names_sprint_flag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _write_structures(root, {"BB": BB_STRUCTURE})
            with self.assertRaises(qof.QueryError) as caught:
                qof._sprint_for_branch(root, "fix/bb6-cli-qa2", None)
        self.assertIn("--sprint <ID>", str(caught.exception))

    def test_sprint_id_selects_sprint_for_undeclared_branch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _write_structures(root, {"BB": BB_STRUCTURE})
            phase, _, sprint = qof._sprint_for_branch(root, "fix/bb6-cli-qa2", None, "BB6")
        self.assertEqual((phase, sprint), ("BB", TRIAGE["BB6"]))

    def test_sprint_id_ambiguous_across_phases_needs_phase(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _write_structures(root, {"BB": BB_STRUCTURE, "AJ": AJ_STRUCTURE})
            with self.assertRaises(qof.QueryError) as caught:
                qof._sprint_for_branch(root, "fix/bb6-cli-qa2", None, "BB6")
            self.assertIn("found 2", str(caught.exception))
            phase, _, sprint = qof._sprint_for_branch(root, "fix/bb6-cli-qa2", "BB", "BB6")
        self.assertEqual((phase, sprint), ("BB", TRIAGE["BB6"]))

    def test_unknown_sprint_id_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _write_structures(root, {"BB": BB_STRUCTURE})
            with self.assertRaises(qof.QueryError) as caught:
                qof._sprint_for_branch(root, "fix/bb9-x", "BB", "BB9")
        self.assertIn("sprint 'BB9'", str(caught.exception))


class IntegrationCandidateTests(unittest.TestCase):
    CANDIDATES = [
        (Path("/wt/integrate/phase-aq"), "integrate/phase-aq"),
        (Path("/wt/integrate/phase-az"), "integrate/phase-az"),
        (Path("/wt/integrate/phase-bb"), "integrate/phase-bb"),
    ]

    def test_single_candidate_wins_without_phase(self) -> None:
        self.assertEqual(
            qof.choose_integration_candidate(self.CANDIDATES[:1], None), self.CANDIDATES[0][0]
        )

    def test_multiple_candidates_without_phase_is_ambiguous(self) -> None:
        self.assertIsNone(qof.choose_integration_candidate(self.CANDIDATES, None))

    def test_phase_selects_matching_worktree_case_insensitively(self) -> None:
        for phase in ("BB", "bb", "phase-bb"):
            with self.subTest(phase=phase):
                self.assertEqual(
                    qof.choose_integration_candidate(self.CANDIDATES, phase),
                    Path("/wt/integrate/phase-bb"),
                )

    def test_phase_without_matching_worktree_stays_ambiguous(self) -> None:
        self.assertIsNone(qof.choose_integration_candidate(self.CANDIDATES, "AX"))

    def test_no_candidates_is_ambiguous(self) -> None:
        self.assertIsNone(qof.choose_integration_candidate([], "BB"))


class ClosedFindingTests(unittest.TestCase):
    def _graph(self, body: str) -> tuple[Graph, URIRef]:
        graph = Graph()
        graph.parse(
            data="@prefix triage: <urn:atm:triage:> .\n<urn:atm:triage:finding/X> a triage:Finding ;\n" + body,
            format="turtle",
        )
        return graph, URIRef("urn:atm:triage:finding/X")

    def test_open_finding_is_not_closed(self) -> None:
        graph, finding = self._graph('  triage:status "open" .\n')
        self.assertFalse(qof.is_closed_finding(graph, finding))

    def test_terminal_status_appended_beside_open_is_closed(self) -> None:
        graph, finding = self._graph('  triage:status "open" ;\n  triage:status "fixed" .\n')
        self.assertTrue(qof.is_closed_finding(graph, finding))

    def test_closed_flag_is_closed(self) -> None:
        graph, finding = self._graph('  triage:status "open" ;\n  triage:closed true .\n')
        self.assertTrue(qof.is_closed_finding(graph, finding))

    def test_resolution_record_is_closed(self) -> None:
        graph, finding = self._graph(
            '  triage:status "open" .\n'
            "<urn:atm:triage:resolution/X/1> a triage:Resolution ; triage:resolves <urn:atm:triage:finding/X> .\n"
        )
        self.assertTrue(qof.is_closed_finding(graph, finding))

    def test_non_terminal_status_values_stay_open(self) -> None:
        graph, finding = self._graph('  triage:status "open" ;\n  triage:status "in_progress" ;\n  triage:closed false .\n')
        self.assertFalse(qof.is_closed_finding(graph, finding))


if __name__ == "__main__":
    unittest.main()
