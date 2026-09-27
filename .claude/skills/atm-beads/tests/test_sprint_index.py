import importlib.machinery
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
sys.path.insert(0, str(SCRIPTS))
loader = importlib.machinery.SourceFileLoader('export_sprint_index', str(SCRIPTS / 'export-sprint-index'))
spec = importlib.util.spec_from_loader(loader.name, loader)
export = importlib.util.module_from_spec(spec)
loader.exec_module(export)


class SprintIndexTests(unittest.TestCase):
    def export_rows(self, children, gates=None):
        if gates is None:
            gates = [self.gate(b["id"]) for b in children if export.metadata(b).get("sprint")]
        children = children + gates
        root = {'id': 'example-phase-x', 'metadata': {'integration_branch': 'integrate/phase-x'}}
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'sprints.json'
            with patch.object(sys, 'argv', ['export-sprint-index', '--root', root['id'], '--out', str(out)]), patch.object(export.subprocess, 'check_output', return_value=tmp), patch.object(export, 'run_json', side_effect=[[root], children]):
                self.assertEqual(export.main(), 0)
            return json.loads(out.read_text())

    def gate(self, dev):
        return {'id': 'gate-for-' + dev, 'labels': ['stage:dev-sanity'],
                'dependencies': [{'type': 'blocks', 'depends_on_id': dev}]}

    def bead(self, number):
        return {'id': f'example-x-{number}', 'metadata': {'sprint': f'x-{number}', 'layer': 1, 'owned_paths': ['z', 'a'], 'requirements': ['Z-002', 'A-001'], 'adrs': ['ADR-002', 'ADR-001']}, 'dependencies': [{'depends_on_id': 'example-phase-x', 'type': 'parent-child'}, {'depends_on_id': 'prerequisite', 'type': 'blocks'}]}

    def test_records_only_bead_pairs_in_natural_order(self):
        result = self.export_rows([self.bead(10), self.bead(2)])
        self.assertEqual(set(result), {'root_bead_id', 'sprints'})
        self.assertEqual([r['dev_bead_id'] for r in result['sprints']], ['example-x-2', 'example-x-10'])
        self.assertEqual(set(result['sprints'][0]), {'dev_bead_id', 'sanity_bead_id'})
        self.assertEqual(result['sprints'][0]['dev_bead_id'], 'example-x-2')
        self.assertEqual(result['sprints'][0]['sanity_bead_id'], 'gate-for-example-x-2')

    def test_bead_content_changes_do_not_change_membership(self):
        bead = self.bead(1)
        before = self.export_rows([bead])
        bead.update(status='closed', assignee='new-agent', close_reason='implementation complete')
        bead.update(title='Updated title', description='New scope')
        bead['metadata'].update(layer=999, owned_paths=['new/path'], requirements=['NEW-001'])
        qa = {'id': 'example-x-1-qa-r2', 'status': 'open'}
        self.assertEqual(self.export_rows([bead, qa]), before)


    def test_missing_or_ambiguous_sanity_gate_is_rejected(self):
        dev = self.bead(1)
        with self.assertRaisesRegex(RuntimeError, 'exactly one sanity gate'):
            self.export_rows([dev], gates=[])
        first = self.gate(dev['id'])
        second = {**first, 'id': 'second-gate'}
        with self.assertRaisesRegex(RuntimeError, 'exactly one sanity gate'):
            self.export_rows([dev], gates=[first, second])

    def test_absorbed_work_is_not_a_sprint(self):
        bead = self.bead(1)
        bead['close_reason'] = 'folded into example-x-2 by plan ruling'
        result = self.export_rows([bead, self.bead(2)])
        self.assertEqual([row['dev_bead_id'] for row in result['sprints']], ['example-x-2'])
        self.assertNotIn('folded_into', result['sprints'][0])
        self.assertEqual(self.export_rows([bead], gates=[])['sprints'], [])


if __name__ == '__main__':
    unittest.main()
