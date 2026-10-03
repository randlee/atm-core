import copy
import sys
from pathlib import Path
import unittest
import xml.etree.ElementTree as ET

SKILL = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(SKILL / 'scripts'), str(SKILL.parent / 'atm-beads/scripts')]
import sprint_dag as dag


def bead(key, labels=(), deps=(), status='closed', reason=''):
    return {'id': key, 'labels': list(labels), 'status': status, 'close_reason': reason,
            'dependencies': [{'type': 'blocks', 'depends_on_id': dep} for dep in deps]}


class DagTests(unittest.TestCase):
    def setUp(self):
        self.index = {'root_bead_id': 'phase-root', 'sprints': [
            {'dev_bead_id': 'work-1', 'sanity_bead_id': 'gate-1'},
            {'dev_bead_id': 'work-2', 'sanity_bead_id': 'gate-2', 'depends_on_sanity_bead_ids': ['gate-1']},
        ]}
        self.beads = {b['id']: b for b in [
            bead('plan', ['stage:plan-review']),
            bead('work-1', ['stage:dev'], ['plan']),
            bead('gate-1', ['stage:dev-sanity'], ['work-1'], reason='PASS at abc'),
            bead('work-2', ['stage:dev'], ['gate-1', 'finding']),
            bead('gate-2', ['stage:dev-sanity'], ['work-2'], reason='PASS at def'),
            bead('finding', ['qa-finding'], ['hidden-ancestor'], status='open'),
            bead('hidden-ancestor'),
            bead('unrelated-gate', ['stage:plan-review']),
        ]}
        self.graph = dag.build_graph(self.index, self.beads)
        self.snapshot = {'beads': self.beads, 'blocked': {}, 'tasks': {}, 'errors': {},
                         'iterations': {}, 'events': {key: [] for key in self.graph['nodes']}}
        for key in ('work-1', 'work-2'):
            self.snapshot['events'][key] = [{'event': 'completed', 'at': '2026-09-26T10:00:00Z'}]
        for key in ('gate-1', 'gate-2'):
            self.snapshot['events'][key] = [{'event': 'completed', 'at': '2026-09-26T11:00:00Z'}]

    def test_scope_and_canonical_edge_direction(self):
        self.assertEqual(set(self.graph['nodes']), {'work-1', 'work-2', 'gate-1', 'gate-2'})
        self.assertEqual(self.graph['gate_pairs'], {'work-1': 'gate-1', 'work-2': 'gate-2'})
        source = dag.dot_source(self.graph, 'example')
        self.assertIn('"work-1" -> "gate-1";', source)
        self.assertIn('"gate-1" -> "work-2";', source)
        self.assertNotIn('finding', source)
        self.assertNotIn('"work-1" -> "work-2";', source)
        # A mutable live dependency cannot alter the declared plan edge.
        self.beads['work-2']['dependencies'] = [{'type': 'blocks', 'depends_on_id': 'work-1'}]
        g = dag.build_graph(self.index, self.beads)
        self.assertIn(['work-2', 'gate-1'], g['edges'])

    def test_index_gate_mismatch_is_rejected(self):
        self.index['sprints'][1]['depends_on_sanity_bead_ids'] = []
        self.index['sprints'][0]['sanity_bead_id'] = 'wrong-gate'
        with self.assertRaisesRegex(RuntimeError, 'does not match live gate'):
            dag.build_graph(self.index, self.beads)
        self.index['sprints'][0]['title'] = 'Duplicated title'
        with self.assertRaisesRegex(RuntimeError, 'normalized sprint item'):
            dag.build_graph(self.index, self.beads)

    def test_cycle_and_missing_gate_are_errors(self):
        self.index['sprints'][0]['depends_on_sanity_bead_ids'] = ['gate-2']
        with self.assertRaisesRegex(RuntimeError, 'cycle'):
            dag.build_graph(self.index, self.beads)
        del self.index['sprints'][0]['depends_on_sanity_bead_ids']
        del self.beads['gate-1']
        with self.assertRaisesRegex(RuntimeError, 'expected one'):
            dag.build_graph(self.index, self.beads)

    def test_sanity_counts_and_stale_pass(self):
        self.snapshot['iterations']['gate-1'] = 14
        result = dag.states(self.graph, self.snapshot, self.index)
        self.assertEqual(result['gate-1']['sanity_iterations'], 14)
        self.assertEqual(result['gate-1']['state'], 'done')
        self.assertEqual(result['work-1']['state'], 'done')
        self.snapshot['events']['work-1'].append({'event': 'completed', 'at': '2026-09-26T12:00:00Z'})
        result = dag.states(self.graph, self.snapshot, self.index)
        self.assertEqual(result['work-1']['state'], 'uncertain')
        self.assertEqual(result['gate-1']['state'], 'uncertain')

    def test_missing_history_is_not_zero_or_green(self):
        self.snapshot['events']['gate-1'] = None
        result = dag.states(self.graph, self.snapshot, self.index)
        self.assertEqual(result['gate-1']['sanity_iterations'], '?')
        self.assertEqual(result['gate-1']['state'], 'uncertain')
        self.assertEqual(result['work-1']['state'], 'uncertain')

    def test_reopened_active_task_overrides_closed_bead(self):
        self.snapshot['tasks']['work-1'] = {'state': 'active'}
        self.assertEqual(dag.states(self.graph, self.snapshot, self.index)['work-1']['state'], 'active')

    def test_open_sanity_finding_blocks_green_but_is_not_a_node(self):
        self.beads['finding']['dependencies'] = [{'type': 'discovered-from', 'depends_on_id': 'gate-1'}]
        result = dag.states(self.graph, self.snapshot, self.index)
        self.assertEqual(result['gate-1']['state'], 'findings')
        self.assertEqual(result['work-1']['state'], 'findings')
        self.assertNotIn('finding', self.graph['nodes'])

    def test_override_is_not_a_pass(self):
        self.beads['gate-1']['close_reason'] = 'FAIL overridden by user'
        result = dag.states(self.graph, self.snapshot, self.index)
        self.assertEqual(result['gate-1']['state'], 'override')

    def test_qa_counts_all_rounds_and_preserves_failed_verdict(self):
        for number, verdict in [(1, 'FAIL'), (2, 'PASS')]:
            qa = bead(f'qa-{number}', reason=verdict + ': review')
            qa['metadata'] = {'round': number}
            qa['dependencies'] = [{'type': 'validates', 'depends_on_id': 'work-1'}]
            self.beads[qa['id']] = qa
        finding = bead('qa-finding', status='open')
        finding['metadata'] = {'severity': 'blocking'}
        finding['dependencies'] = [
            {'type': 'discovered-from', 'depends_on_id': 'qa-1'},
            {'type': 'parent-child', 'depends_on_id': 'qa-1'}]
        self.beads['qa-finding'] = finding
        result = dag.qa_states(self.graph, self.beads, self.index)
        self.assertEqual(result['work-1']['findings'], '1:0:0')
        self.assertEqual(result['work-1']['icon'], '🚩')
        self.assertEqual(result['work-2']['icon'], '')
        self.assertEqual(result['gate-1']['icon'], '')
        finding['status'] = 'closed'
        self.assertEqual(dag.qa_states(self.graph, self.beads, self.index)['work-1']['icon'], '✅')
        self.beads['qa-2']['close_reason'] = 'FAIL: findings now fixed'
        result = dag.qa_states(self.graph, self.beads, self.index)
        self.assertEqual(result['work-1']['findings'], '0:0:0')
        self.assertEqual(result['work-1']['icon'], '🚩')

    def test_qa_pending_and_active(self):
        qa = bead('qa', status='open')
        qa['dependencies'] = [{'type': 'validates', 'depends_on_id': 'work-1'}]
        self.beads['qa'] = qa
        self.assertEqual(dag.qa_states(self.graph, self.beads, self.index)['work-1']['icon'], '📥')
        qa['status'] = 'in_progress'
        self.assertEqual(dag.qa_states(self.graph, self.beads, self.index)['work-1']['icon'], '🌀')

    def test_overlay_preserves_node_and_edge_geometry(self):
        svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200"><g transform="translate(0 100)"><g class="node"><title>work-1</title><path d="M 10,-40 L 70,-40 L 70,-10 L 10,-10 Z"/><text x="40" y="-20">work-1</text></g><g class="edge"><path d="M 70,-20 L 100,-20"/></g></g></svg>'
        icons = {'work-1': {'state': 'done', 'sanity_iterations': None, 'evidence': 'Passed'}}
        qa = {'work-1': {'icon': '🚩', 'findings': '1:5:13', 'verdict': 'FAIL', 'qa_beads': ['qa-1']}}
        rendered = dag.overlay(svg, icons, '2026-09-26', qa)
        old, new = ET.fromstring(svg), ET.fromstring(rendered)
        for kind in ('node', 'edge'):
            def geometry(root):
                return [[(e.tag, e.attrib) for e in group if e.tag != dag.tag('title')]
                        for group in root.iter(dag.tag('g')) if group.get('class') == kind]
            self.assertEqual(geometry(old), geometry(new))
        self.assertEqual(sum(g.get('class') == 'state-badge' for g in new.iter(dag.tag('g'))), 8)
        bottom = next(g for g in new.iter(dag.tag('g')) if g.get('class') == 'qa-badge')
        self.assertEqual(bottom.find(dag.tag('text')).text, '1:5:13')
        self.assertTrue(bottom.get('transform').endswith(' -19)'))


if __name__ == '__main__':
    unittest.main()
