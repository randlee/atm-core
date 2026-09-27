"""Fixture tests for phase_contract_check (bead demo-bo-10 D7): one fixture per rejected invariant, plus valid phases."""
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPTS = HERE.parent / "scripts"
FIXTURES = HERE / "fixtures"
sys.path.insert(0, str(SCRIPTS))
import plan_contract as C  # noqa: E402
from sprint_index_common import validate_index  # noqa: E402

spec = importlib.util.spec_from_file_location("phase_contract_check", SCRIPTS / "phase_contract_check.py")
pcc = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(pcc)

# fixture -> (bead id expected on a problem line, substring of that line); None = must be valid
EXPECT: dict[str, tuple[str, str] | None] = {
    "valid_phase": None,
    "valid_index_with_policy": None,
    "valid_pass_with_qa": None,
    "valid_root_feature_under_epic": None,
    "valid_ordered_overlap": None,
    "qa_under_root_validates": ("x-t-1-qa", "bd dep remove x-t-1-qa x-t-1 (validates), then bd update x-t-1-qa --parent x-t-1"),
    "finding_under_root_caused_by": ("x-t-1-qa-f1", "bd dep remove x-t-1-qa-f1 x-t-1 (caused-by), then bd update x-t-1-qa-f1 --parent x-t-1"),
    "root_task_at_top_level": ("x-phase-t", "phase root is a task; a phase root is an epic or a feature under an epic"),
    "root_feature_under_task": ("x-prog", "phase ancestor is a task, not an epic; only epics may hold phases"),
    "valid_closed_finding_without_difficulty": None,
    "valid_r16_gated": None,
    "valid_waived_reopen": None,
    "valid_closed_finding_p3": None,
    "index_waiver_bad_check": ("x-phase-t", "waivers check must be one of"),
    "index_undeclared_key": ("x-phase-t", "undeclared keys: owner"),
    "finding_parented_on_root": ("x-t-1-qa-f1", "not a listed sprint pair"),
    "listed_pair_missing_from_beads": ("x-t-4", "not in beads"),
    "live_pair_missing_from_index": ("x-t-4", "not a listed sprint pair"),
    "dev_without_sprint_label": ("x-t-2", "lacks label stage:sprint"),
    "deliverables_1_2_4": ("x-t-2", "numbered [1, 2, 4]"),
    "acceptance_key_5_of_3": ("x-t-2", "#5 but there are 3"),
    "difficulty_medium": ("x-t-2", "'medium' is not one of"),
    "sprint_without_difficulty": ("x-t-2", "difficulty missing"),
    "pr_target_sanity_outside_closure": ("x-t-3", "x-t-2-sanity is not in its blocker closure"),
    "owned_path_overlap": ("x-t-2", "overlaps x-t-3's"),
    "handoff_outside_consumer_fence": ("x-t-2", "owned_paths do not include it"),
    "reopened_pass_sanity": ("x-t-1-sanity", "reopened at 2026-09-26T12:00+00:00 after a PASS"),
    "started_before_blocker_closed": ("x-t-2", "before blocker x-t-1-sanity closed"),
    "in_progress_with_open_blocker": ("x-t-2", "in progress while blocker x-t-1-sanity is open"),
    "pass_without_qa": ("x-t-1", "no QA bead"),
    "second_fix_round": ("x-t-1", "ROUND_CAP"),
    "pr_base_mismatch": ("x-t-2", "PR #7 base is integrate/phase-t, not pr_target sprint/t-1"),
    "important_finding_p3": ("x-t-1-qa-f1", "important finding is P3, the severity map says P2"),
    "open_finding_without_difficulty": ("x-t-1-qa-f1", "difficulty missing"),
    "finding_without_severity": ("x-t-1-qa-f1", "no severity label"),
    "sprint_bead_p3": ("x-t-1", "P3 but planned sprint beads are P2"),
    "unlisted_human_gate": ("x-gate-1", "not listed in sprints.json policy.human_gates"),
    "sanity_base_sha_commit_short": ("x-t-1-sanity", "base is a SHA"),
    "r16_downstream_not_gated": ("x-t-2", "not blocked by blocking finding x-t-1-qa-f1's sanity bead x-t-1-qa-f1-sanity"),
    "r16_in_progress_exempt_warns": None,
    "r16_deferred_finding_exempts_upstream": None,
    "r16_blocking_finding_without_sanity": ("x-t-1-qa-f1", "no stage:dev-sanity bead"),
    "r16_gate_not_blocked_by_finding": ("x-t-1-qa-f1-sanity", "does not block on it"),
}


def run(name: str) -> tuple[list[str], list[str]]:
    return pcc.run_fixture(FIXTURES / f"{name}.json")


class FixtureTable(unittest.TestCase):
    def test_every_fixture_file_is_in_the_table(self):
        files = sorted(p.stem for p in FIXTURES.glob("*.json"))
        self.assertEqual(files, sorted(EXPECT))

    def test_fixtures(self):
        for name, want in EXPECT.items():
            with self.subTest(fixture=name):
                problems, _ = run(name)
                if want is None:
                    self.assertEqual(problems, [], f"{name} must be valid")
                    continue
                bead, text = want
                line = C.PROBLEM_LINE.format(bead=bead, message="")
                hits = [p for p in problems if p.startswith(line) and text in p]
                self.assertTrue(hits, f"{name}: no problem line '{bead}: ...{text}...' in {problems}")

    def test_waiver_becomes_warning(self):
        problems, warnings = run("valid_waived_reopen")
        self.assertEqual(problems, [])
        self.assertTrue(any(w.startswith("warning: waived reopened_after_pass on x-t-1-sanity") for w in warnings), warnings)

    def test_r16_warns_but_does_not_fail_on_started_work(self):
        problems, warnings = run("r16_in_progress_exempt_warns")
        self.assertEqual(problems, [])
        self.assertTrue(any("x-t-2 is in progress while blocking finding x-t-1-qa-f1" in w for w in warnings), warnings)
        self.assertTrue(all(w.startswith(C.WARNING_PREFIX) for w in warnings))

    def test_r16_downstream_gates_cover_open_finding_and_skip_started_dev(self):
        problems, _ = run("r16_downstream_not_gated")
        targets = {p.split(":", 1)[0] for p in problems if "not blocked by blocking finding" in p}
        self.assertEqual(targets, {"x-t-2", "x-t-3", "x-t-2-qa-f1"})

    def test_cli_exit_codes_and_line_format(self):
        script = SCRIPTS / "phase_contract_check.py"
        ok = subprocess.run([sys.executable, str(script), "--fixture", str(FIXTURES / "valid_phase.json")], capture_output=True, text=True)
        self.assertEqual((ok.returncode, ok.stdout), (C.EXIT_VALID, ""))
        bad = subprocess.run([sys.executable, str(script), "--fixture", str(FIXTURES / "important_finding_p3.json")], capture_output=True, text=True)
        self.assertEqual(bad.returncode, C.EXIT_PROBLEMS)
        for line in bad.stdout.splitlines():
            self.assertRegex(line, r"^(warning: |[A-Za-z0-9._-]+: )")


class IndexSchema(unittest.TestCase):
    def test_declared_optional_keys_pass(self):
        idx = json.loads((FIXTURES / "valid_index_with_policy.json").read_text())["index"]
        validate_index(idx)  # no raise

    def test_undeclared_key_rejected(self):
        idx = json.loads((FIXTURES / "index_undeclared_key.json").read_text())["index"]
        with self.assertRaises(RuntimeError):
            validate_index(idx)

    def test_policy_only_human_gates(self):
        idx = json.loads((FIXTURES / "valid_phase.json").read_text())["index"]
        idx["policy"] = {"human_gates": [], "auto_merge": True}
        with self.assertRaises(RuntimeError):
            validate_index(idx)

    def test_schema_declares_the_same_optional_keys(self):
        # docs/plans/ in the repository the skill is installed into; the package asset in a checkout
        candidates = [HERE.parents[3] / "docs" / "plans" / "sprints.schema.json",
                      HERE.parents[2] / "assets" / "docs" / "plans" / "sprints.schema.json"]
        schema = json.loads(next(p for p in candidates if p.is_file()).read_text())
        self.assertEqual(set(schema["properties"]), {"root_bead_id", "sprints"} | set(C.INDEX_OPTIONAL_KEYS))
        self.assertFalse(schema.get("additionalProperties", True))


class Constants(unittest.TestCase):
    def test_frozen_interface(self):
        self.assertEqual(C.SEVERITY_PRIORITY, {"blocking": 1, "important": 2, "minor": 4})
        self.assertEqual(C.PRIORITY_SPRINT, 2)
        self.assertEqual(set(C.DIFFICULTY_MODELS), {"hard", "normal", "fast"})
        self.assertIn("luna", C.DIFFICULTY_MODELS["fast"])
        self.assertTrue(C.model_matches("gpt-5.6-terra", "normal"))
        self.assertFalse(C.model_matches("gpt-5.6-luna", "hard"))
        self.assertEqual(C.SPRINT_LABEL, "stage:sprint")
        self.assertEqual((C.EXIT_VALID, C.EXIT_CANNOT_RUN, C.EXIT_PROBLEMS), (0, 2, 5))


class Parsing(unittest.TestCase):
    def test_acceptance_key_styles(self):
        text = "- [ ] #1: a\n- [ ] #2–3: b\n- [ ] (D4) c\n- [ ] Deliverable 5: d\n- [ ] Deliverables 6–7: e\n- [ ] (#8/#9) f\n- [ ] (`x-t-1#10/#11`) g\n- [ ] fixes #88 in prose\n"
        self.assertEqual(pcc.acceptance_refs(text), set(range(1, 12)))

    def test_deliverables_list(self):
        self.assertEqual(pcc.parse_deliverables("## Goal\n1. not here\n## Deliverables\n1. a\n2. b\n\n## Acceptance\n3. no"), [1, 2])

    def test_paths_overlap(self):
        self.assertTrue(pcc.paths_overlap("crates/a/**", "crates/a/src/lib.rs"))
        self.assertFalse(pcc.paths_overlap("crates/a/**", "crates/ab/**"))
        self.assertTrue(pcc.paths_overlap("docs/x.md", "docs/x.md"))


if __name__ == "__main__":
    unittest.main()
