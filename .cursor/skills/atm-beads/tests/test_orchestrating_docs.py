from pathlib import Path
import unittest


class OrchestratingDocsTests(unittest.TestCase):
    def test_stack_bead_section_defines_layer_order_authority(self):
        skill_dir = Path(__file__).resolve().parents[1]
        text = (skill_dir / 'resources/orchestrating.md').read_text()
        section = text.split('## Stack Bead', 1)[1].split('\n## ', 1)[0]
        self.assertIn('`layers` list is the only record of layer order', section)
        self.assertIn('the last entry is the top', section)

    def test_stack_bead_section_states_the_claim_timeout_waiting_rule(self):
        skill_dir = Path(__file__).resolve().parents[1]
        text = (skill_dir / 'resources/orchestrating.md').read_text()
        section = text.split('## Stack Bead', 1)[1].split('\n## ', 1)[0]
        for phrase in ('60 seconds by default', "returns the holder's identity", 'keeps its task active',
                       'never refuses the task', 'fails the dev or escalates to a person'):
            self.assertIn(phrase, section)


if __name__ == '__main__':
    unittest.main()
