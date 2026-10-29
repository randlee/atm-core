from pathlib import Path
import unittest


class ImportingDocsTests(unittest.TestCase):
    def test_phase_index_instruction_is_an_import_procedure_step(self):
        skill_dir = Path(__file__).resolve().parents[1]
        procedure = (skill_dir / 'resources/importing-md-plan.md').read_text()
        skill = (skill_dir / 'SKILL.md').read_text()
        self.assertIn('9. **Wire the plan gate**', procedure)
        self.assertIn('**Mandatory:** write the plan file `<plans_dir>/phase-<x>.jsonl`', procedure)
        self.assertIn('then commit the plan file, the phase file and the DAG', procedure)
        self.assertLess(procedure.index('sprint-review --root <root>'), procedure.index('then commit the plan file'))
        self.assertIn('phase-config init --phase <x> --lead <appointed-lead>', procedure)
        self.assertIn('in the same commit as the plan', procedure)
        self.assertIn('scripts/validate-plan --phase <x>', procedure)
        self.assertNotIn('never commits or pushes) without', skill)
        planning = (skill_dir / 'resources/planning.md').read_text()
        for doc in (' '.join(procedure.split()), ' '.join(planning.split())):
            self.assertIn('open a PR from the `integration_branch` to the repository trunk the phase lands on, carrying the plan file, the phase file and the DAG, before plan review', doc)
            self.assertIn('re-run `sprint-review` and commit the DAG with the plan file change, which updates the plan PR', doc)
        self.assertNotIn('After importing a phase plan, export the sprint index', skill)

    def test_planning_forbids_hand_written_stack_beads(self):
        skill_dir = Path(__file__).resolve().parents[1]
        planning = (skill_dir / 'resources/planning.md').read_text()
        self.assertIn('A hand-written `bd create` is forbidden for these beads', planning)


if __name__ == '__main__':
    unittest.main()
