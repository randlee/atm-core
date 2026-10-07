from pathlib import Path
import unittest


class ImportingDocsTests(unittest.TestCase):
    def test_phase_index_instruction_is_an_import_procedure_step(self):
        skill_dir = Path(__file__).resolve().parents[1]
        procedure = (skill_dir / 'resources/importing-md-plan.md').read_text()
        skill = (skill_dir / 'SKILL.md').read_text()
        self.assertIn('9. **Wire the plan gate**', procedure)
        self.assertIn('**Mandatory:** write the plan file `<plans_dir>/phase-<x>.jsonl`', procedure)
        self.assertIn('in the same commit as the plan', procedure)
        self.assertIn('scripts/validate-plan --phase <x>', procedure)
        self.assertNotIn('After importing a phase plan, export the sprint index', skill)


if __name__ == '__main__':
    unittest.main()
